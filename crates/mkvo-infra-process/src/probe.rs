use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio_util::sync::CancellationToken;

use crate::{ProcessError, ProcessRunner, ProcessSpec, ToolKind, ToolRegistry};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackType {
    Video,
    Audio,
    Subtitle,
    Data,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ContainerMetadata {
    pub title: Option<String>,
    pub format: Option<String>,
    pub duration_millis: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackMetadata {
    pub id: u32,
    pub track_number: Option<u32>,
    pub kind: TrackType,
    pub codec: String,
    pub language: Option<String>,
    pub name: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub bit_depth: Option<u32>,
    pub pixel_format: Option<String>,
    pub color_transfer: Option<String>,
    pub color_primaries: Option<String>,
    pub default: bool,
    pub forced: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentMetadata {
    pub id: u32,
    pub file_name: Option<String>,
    pub content_type: Option<String>,
    pub description: Option<String>,
    pub size_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScannedMedia {
    pub path: PathBuf,
    pub container: ContainerMetadata,
    pub tracks: Vec<TrackMetadata>,
    pub attachments: Vec<AttachmentMetadata>,
    pub reader: ToolKind,
    pub warning: Option<String>,
}

#[derive(Debug, Error)]
pub enum ProbeError {
    #[error("required media tool `{0}` is unavailable")]
    ToolUnavailable(&'static str),
    #[error(transparent)]
    Process(#[from] ProcessError),
    #[error("media probe exited with code {code:?}: {message}")]
    Exit { code: Option<i32>, message: String },
    #[error("media probe returned invalid JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),
    #[error("media probe returned no tracks for `{0}`")]
    NoTracks(PathBuf),
    #[error("unsupported media extension for `{0}`")]
    Unsupported(PathBuf),
}

#[async_trait]
pub trait MediaProbe: Send + Sync {
    async fn inspect(
        &self,
        path: &Path,
        cancellation: CancellationToken,
    ) -> Result<ScannedMedia, ProbeError>;
}

#[derive(Debug, Clone)]
pub struct MediaScanAdapter {
    registry: ToolRegistry,
    runner: ProcessRunner,
}

impl MediaScanAdapter {
    pub const fn new(registry: ToolRegistry, runner: ProcessRunner) -> Self {
        Self { registry, runner }
    }

    async fn inspect_with(
        &self,
        tool: ToolKind,
        path: &Path,
        cancellation: CancellationToken,
    ) -> Result<ScannedMedia, ProbeError> {
        let resolved = self
            .registry
            .resolve(tool)
            .ok_or_else(|| ProbeError::ToolUnavailable(tool.command_name()))?;
        let spec = match tool {
            ToolKind::MkvMerge => ProcessSpec::new(resolved.path)
                .args(["--identification-format", "json", "--identify"])
                .arg(path.as_os_str())
                .timeout(Duration::from_secs(120)),
            ToolKind::Ffprobe => ProcessSpec::new(resolved.path)
                .args([
                    "-v",
                    "error",
                    "-show_format",
                    "-show_streams",
                    "-of",
                    "json",
                ])
                .arg(path.as_os_str())
                .timeout(Duration::from_secs(120)),
            _ => return Err(ProbeError::Unsupported(path.to_path_buf())),
        };

        let output = self.runner.run(spec, cancellation).await?;
        let accepted_exit = output.exit_code == Some(0)
            || (tool == ToolKind::MkvMerge && output.exit_code == Some(1));
        if !accepted_exit {
            return Err(ProbeError::Exit {
                code: output.exit_code,
                message: probe_message(tool, &output.stdout, &output.stderr),
            });
        }
        let mut scanned = match tool {
            ToolKind::MkvMerge => parse_mkvmerge_json(path, &output.stdout)?,
            ToolKind::Ffprobe => parse_ffprobe_json(path, &output.stdout)?,
            _ => unreachable!("only probing tools are accepted"),
        };
        scanned.reader = tool;
        if output.exit_code == Some(1) {
            scanned.warning = Some(probe_message(tool, &output.stdout, &output.stderr));
        }
        Ok(scanned)
    }
}

#[async_trait]
impl MediaProbe for MediaScanAdapter {
    async fn inspect(
        &self,
        path: &Path,
        cancellation: CancellationToken,
    ) -> Result<ScannedMedia, ProbeError> {
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        if extension.eq_ignore_ascii_case("mkv")
            || extension.eq_ignore_ascii_case("mka")
            || extension.eq_ignore_ascii_case("webm")
        {
            self.inspect_with(ToolKind::MkvMerge, path, cancellation)
                .await
        } else if extension.eq_ignore_ascii_case("mp4") || extension.eq_ignore_ascii_case("m4v") {
            self.inspect_with(ToolKind::Ffprobe, path, cancellation)
                .await
        } else {
            Err(ProbeError::Unsupported(path.to_path_buf()))
        }
    }
}

fn concise_error(stderr: &str) -> String {
    let message = stderr.trim();
    if message.is_empty() {
        "the tool did not provide an error message".to_owned()
    } else {
        message.chars().take(2_000).collect()
    }
}

#[derive(Debug, Default, Deserialize)]
struct MkvMergeMessages {
    #[serde(default)]
    errors: Vec<String>,
    #[serde(default)]
    warnings: Vec<String>,
}

fn probe_message(tool: ToolKind, stdout: &str, stderr: &str) -> String {
    let stderr = stderr.trim();
    if !stderr.is_empty() {
        return stderr.chars().take(2_000).collect();
    }

    if tool == ToolKind::MkvMerge
        && let Ok(messages) = serde_json::from_str::<MkvMergeMessages>(stdout)
    {
        let message = messages
            .errors
            .into_iter()
            .chain(messages.warnings)
            .map(|message| message.trim().to_owned())
            .filter(|message| !message.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        if !message.is_empty() {
            return message.chars().take(2_000).collect();
        }
    }

    concise_error("")
}

#[derive(Debug, Deserialize)]
pub struct MkvMergeDocument {
    #[serde(default)]
    container: MkvContainer,
    #[serde(default)]
    tracks: Vec<MkvTrack>,
    #[serde(default)]
    attachments: Vec<MkvAttachment>,
}

#[derive(Debug, Default, Deserialize)]
struct MkvContainer {
    #[serde(default)]
    properties: MkvContainerProperties,
}

#[derive(Debug, Default, Deserialize)]
struct MkvContainerProperties {
    title: Option<String>,
    container_type: Option<serde_json::Value>,
    duration: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct MkvTrack {
    id: u32,
    #[serde(rename = "type")]
    kind: String,
    codec: String,
    #[serde(default)]
    properties: MkvTrackProperties,
}

#[derive(Debug, Default, Deserialize)]
struct MkvTrackProperties {
    number: Option<serde_json::Value>,
    #[serde(default, deserialize_with = "lenient_string")]
    language: Option<String>,
    #[serde(default, deserialize_with = "lenient_string")]
    language_ietf: Option<String>,
    #[serde(default, deserialize_with = "lenient_string")]
    track_name: Option<String>,
    #[serde(default, deserialize_with = "lenient_string")]
    pixel_dimensions: Option<String>,
    #[serde(default, deserialize_with = "lenient_string")]
    display_dimensions: Option<String>,
    pixel_width: Option<serde_json::Value>,
    display_width: Option<serde_json::Value>,
    video_pixel_width: Option<serde_json::Value>,
    pixel_height: Option<serde_json::Value>,
    display_height: Option<serde_json::Value>,
    video_pixel_height: Option<serde_json::Value>,
    bits_per_channel: Option<serde_json::Value>,
    color_bits_per_channel: Option<serde_json::Value>,
    video_color_bits_per_channel: Option<serde_json::Value>,
    bit_depth: Option<serde_json::Value>,
    #[serde(default, deserialize_with = "lenient_string")]
    color_transfer_characteristics: Option<String>,
    #[serde(default, deserialize_with = "lenient_string")]
    color_primaries: Option<String>,
    default_track: Option<serde_json::Value>,
    forced_track: Option<serde_json::Value>,
}

/// `mkvmerge -J` types some properties differently than its documentation
/// suggests (`display_unit` is an enum code, not a string) and adds properties
/// between releases. Deserializing a cosmetic property strictly would make one
/// unexpected value reject the whole file, so a non-string is read as absent
/// rather than as a parse error.
fn lenient_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(
        match Option::<serde_json::Value>::deserialize(deserializer)? {
            Some(serde_json::Value::String(value)) => Some(value),
            _ => None,
        },
    )
}

#[derive(Debug, Deserialize)]
struct MkvAttachment {
    id: u32,
    file_name: Option<String>,
    name: Option<String>,
    content_type: Option<String>,
    mime_type: Option<String>,
    description: Option<String>,
    size: Option<serde_json::Value>,
    #[serde(default)]
    properties: MkvAttachmentProperties,
}

#[derive(Debug, Default, Deserialize)]
struct MkvAttachmentProperties {
    size: Option<serde_json::Value>,
}

pub fn parse_mkvmerge_json(path: &Path, json: &str) -> Result<ScannedMedia, ProbeError> {
    let document: MkvMergeDocument = serde_json::from_str(json)?;
    let tracks = document
        .tracks
        .into_iter()
        .map(|track| {
            let properties = track.properties;
            let (width, height) = properties
                .pixel_dimensions
                .as_deref()
                .or(properties.display_dimensions.as_deref())
                .and_then(parse_dimensions)
                .unwrap_or_else(|| {
                    (
                        first_u32([
                            properties.pixel_width.as_ref(),
                            properties.display_width.as_ref(),
                            properties.video_pixel_width.as_ref(),
                        ]),
                        first_u32([
                            properties.pixel_height.as_ref(),
                            properties.display_height.as_ref(),
                            properties.video_pixel_height.as_ref(),
                        ]),
                    )
                });
            let kind = parse_track_type(&track.kind);
            TrackMetadata {
                id: track.id,
                track_number: properties.number.as_ref().and_then(value_u32),
                kind,
                codec: normalize_video_codec(kind, &track.codec),
                language: properties.language.or(properties.language_ietf),
                name: properties.track_name,
                width,
                height,
                bit_depth: first_u32([
                    properties.bits_per_channel.as_ref(),
                    properties.color_bits_per_channel.as_ref(),
                    properties.video_color_bits_per_channel.as_ref(),
                    properties.bit_depth.as_ref(),
                ]),
                pixel_format: None,
                color_transfer: properties.color_transfer_characteristics,
                color_primaries: properties.color_primaries,
                default: properties
                    .default_track
                    .as_ref()
                    .and_then(value_bool)
                    .unwrap_or(false),
                forced: properties
                    .forced_track
                    .as_ref()
                    .and_then(value_bool)
                    .unwrap_or(false),
            }
        })
        .collect();
    let attachments = document
        .attachments
        .into_iter()
        .map(|attachment| AttachmentMetadata {
            id: attachment.id,
            file_name: attachment.file_name.or(attachment.name),
            content_type: attachment.content_type.or(attachment.mime_type),
            description: attachment.description,
            size_bytes: attachment
                .properties
                .size
                .as_ref()
                .and_then(value_u64)
                .or_else(|| attachment.size.as_ref().and_then(value_u64)),
        })
        .collect();

    Ok(ScannedMedia {
        path: path.to_path_buf(),
        container: ContainerMetadata {
            title: document.container.properties.title,
            format: document
                .container
                .properties
                .container_type
                .as_ref()
                .and_then(container_type),
            duration_millis: document
                .container
                .properties
                .duration
                .as_ref()
                .and_then(value_u64)
                .map(|ns| ns / 1_000_000),
        },
        tracks,
        attachments,
        reader: ToolKind::MkvMerge,
        warning: None,
    })
}

#[derive(Debug, Deserialize)]
pub struct FfprobeDocument {
    #[serde(default)]
    streams: Vec<FfStream>,
    format: Option<FfFormat>,
}

#[derive(Debug, Deserialize)]
struct FfStream {
    index: u32,
    codec_type: Option<String>,
    codec_name: Option<String>,
    codec_long_name: Option<String>,
    profile: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    bits_per_raw_sample: Option<serde_json::Value>,
    bits_per_sample: Option<serde_json::Value>,
    pix_fmt: Option<String>,
    color_transfer: Option<String>,
    color_primaries: Option<String>,
    #[serde(default)]
    tags: FfTags,
    #[serde(default)]
    disposition: FfDisposition,
}

#[derive(Debug, Default, Deserialize)]
struct FfTags {
    #[serde(alias = "LANGUAGE")]
    language: Option<String>,
    #[serde(alias = "TITLE")]
    title: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct FfDisposition {
    default: Option<serde_json::Value>,
    forced: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct FfFormat {
    format_name: Option<String>,
    duration: Option<String>,
    #[serde(default)]
    tags: FfTags,
}

pub fn parse_ffprobe_json(path: &Path, json: &str) -> Result<ScannedMedia, ProbeError> {
    let document: FfprobeDocument = serde_json::from_str(json)?;
    let container = document
        .format
        .map_or_else(ContainerMetadata::default, |format| {
            let duration_millis = format
                .duration
                .as_deref()
                .and_then(|value| value.parse::<f64>().ok())
                .filter(|value| value.is_finite() && *value >= 0.0)
                .map(|seconds| (seconds * 1_000.0).round() as u64);
            ContainerMetadata {
                title: format.tags.title,
                format: format.format_name,
                duration_millis,
            }
        });
    let tracks = document
        .streams
        .into_iter()
        .map(|stream| {
            let kind = parse_track_type(stream.codec_type.as_deref().unwrap_or_default());
            let codec = stream
                .codec_name
                .clone()
                .or_else(|| stream.codec_long_name.clone())
                .unwrap_or_default();
            let bit_depth = first_u32([
                stream.bits_per_raw_sample.as_ref(),
                stream.bits_per_sample.as_ref(),
            ])
            .filter(|value| *value > 0)
            .or_else(|| infer_bit_depth(&stream, path));
            TrackMetadata {
                id: stream.index,
                track_number: Some(stream.index + 1),
                kind,
                codec: normalize_video_codec(kind, &codec),
                language: stream.tags.language,
                name: stream.tags.title,
                width: stream.width,
                height: stream.height,
                bit_depth,
                pixel_format: stream.pix_fmt,
                color_transfer: stream.color_transfer,
                color_primaries: stream.color_primaries,
                default: stream.disposition.default.as_ref().and_then(value_u32) == Some(1),
                forced: stream.disposition.forced.as_ref().and_then(value_u32) == Some(1),
            }
        })
        .collect();

    Ok(ScannedMedia {
        path: path.to_path_buf(),
        container,
        tracks,
        attachments: Vec::new(),
        reader: ToolKind::Ffprobe,
        warning: None,
    })
}

fn parse_track_type(value: &str) -> TrackType {
    if value.eq_ignore_ascii_case("video") {
        TrackType::Video
    } else if value.eq_ignore_ascii_case("audio") {
        TrackType::Audio
    } else if value.eq_ignore_ascii_case("subtitles") || value.eq_ignore_ascii_case("subtitle") {
        TrackType::Subtitle
    } else if value.eq_ignore_ascii_case("data") {
        TrackType::Data
    } else {
        TrackType::Other
    }
}

fn parse_dimensions(value: &str) -> Option<(Option<u32>, Option<u32>)> {
    let normalized = value.replace('X', "x");
    let (width, height) = normalized.split_once('x')?;
    let width = width
        .trim()
        .chars()
        .filter(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .ok();
    let height = height
        .trim()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .ok();
    Some((width, height))
}

fn value_u64(value: &serde_json::Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
}

fn value_u32(value: &serde_json::Value) -> Option<u32> {
    value_u64(value).and_then(|value| u32::try_from(value).ok())
}

fn value_bool(value: &serde_json::Value) -> Option<bool> {
    value.as_bool().or_else(|| {
        value_u64(value).map(|value| value != 0).or_else(|| {
            match value.as_str()?.trim().to_ascii_lowercase().as_str() {
                "true" => Some(true),
                "false" => Some(false),
                _ => None,
            }
        })
    })
}

fn first_u32<const N: usize>(values: [Option<&serde_json::Value>; N]) -> Option<u32> {
    values.into_iter().flatten().find_map(value_u32)
}

fn container_type(value: &serde_json::Value) -> Option<String> {
    if let Some(value) = value.as_str() {
        return Some(value.to_owned());
    }
    match value.as_u64() {
        Some(17) => Some("Matroska".to_owned()),
        Some(value) => Some(value.to_string()),
        None => None,
    }
}

fn normalize_video_codec(kind: TrackType, codec: &str) -> String {
    if kind != TrackType::Video {
        return codec.trim().to_owned();
    }
    let clean = codec.trim();
    let key = clean.to_ascii_lowercase().replace(['_', '-', ' '], "");
    if contains_any(&key, &["hevc", "h265", "h.265", "mpegh"]) {
        "HEVC/H.265".to_owned()
    } else if contains_any(&key, &["avc", "h264", "h.264", "mpeg4avc"]) {
        "AVC/H.264".to_owned()
    } else if key.contains("av1") {
        "AV1".to_owned()
    } else if key.contains("vp9") {
        "VP9".to_owned()
    } else if key.contains("vp8") {
        "VP8".to_owned()
    } else if contains_any(&key, &["mpeg2video", "mpeg2"]) {
        "MPEG-2".to_owned()
    } else if contains_any(&key, &["mpeg4", "xvid", "divx"]) {
        "MPEG-4".to_owned()
    } else if contains_any(&key, &["vc1", "wvc1"]) {
        "VC-1".to_owned()
    } else if key.contains("prores") {
        "ProRes".to_owned()
    } else if key.contains("theora") {
        "Theora".to_owned()
    } else {
        clean.to_owned()
    }
}

fn contains_any(value: &str, candidates: &[&str]) -> bool {
    candidates.iter().any(|candidate| value.contains(candidate))
}

fn infer_bit_depth(stream: &FfStream, path: &Path) -> Option<u32> {
    let combined = format!(
        "{} {} {} {} {}",
        stream.pix_fmt.as_deref().unwrap_or_default(),
        stream.profile.as_deref().unwrap_or_default(),
        stream.codec_name.as_deref().unwrap_or_default(),
        stream.codec_long_name.as_deref().unwrap_or_default(),
        path.file_stem()
            .map_or_else(String::new, |value| value.to_string_lossy().into_owned())
    )
    .to_ascii_lowercase();
    if contains_any(
        &combined,
        &[
            "p10",
            "yuv420p10",
            "yuv422p10",
            "yuv444p10",
            "10bit",
            "10-bit",
            "10 bit",
            "hi10p",
            "main 10",
        ],
    ) {
        Some(10)
    } else if contains_any(
        &combined,
        &[
            "p12",
            "yuv420p12",
            "yuv422p12",
            "yuv444p12",
            "12bit",
            "12-bit",
            "12 bit",
        ],
    ) {
        Some(12)
    } else if contains_any(&combined, &["8bit", "8-bit", "8 bit"])
        || combined
            .split(|character: char| !character.is_ascii_alphanumeric())
            .any(|word| matches!(word, "yuv420p" | "yuv422p" | "yuv444p"))
    {
        Some(8)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_cases(name: &str) -> Vec<serde_json::Value> {
        let source = match name {
            "mkvmerge" => {
                include_str!("../../../tests/parity-fixtures/media-mkvmerge-identify.json")
            }
            "ffprobe" => include_str!("../../../tests/parity-fixtures/media-ffprobe-identify.json"),
            _ => unreachable!("known fixture"),
        };
        serde_json::from_str::<serde_json::Value>(source)
            .expect("fixture JSON")
            .get("cases")
            .and_then(serde_json::Value::as_array)
            .expect("fixture cases")
            .clone()
    }

    #[test]
    fn parses_mkvmerge_identification_json() {
        let json = r#"{
          "container":{"properties":{"container_type":"Matroska","title":"Episode","duration":1234000000}},
          "tracks":[
            {"id":0,"type":"video","codec":"AVC/H.264","properties":{"number":1,"pixel_dimensions":"1920x1080","bit_depth":10,"default_track":true}},
            {"id":1,"type":"audio","codec":"AAC","properties":{"number":2,"language":"eng","track_name":"English"}}
          ],
          "attachments":[{"id":2,"file_name":"cover.jpg","content_type":"image/jpeg","size":42}]
        }"#;

        let media = parse_mkvmerge_json(Path::new("episode.mkv"), json).expect("valid JSON");
        assert_eq!(media.container.title.as_deref(), Some("Episode"));
        assert_eq!(media.container.duration_millis, Some(1_234));
        assert_eq!(media.tracks[0].width, Some(1920));
        assert_eq!(media.tracks[0].bit_depth, Some(10));
        assert_eq!(media.tracks[1].language.as_deref(), Some("eng"));
        assert_eq!(media.attachments.len(), 1);
    }

    #[test]
    fn reports_mkvmerge_json_errors_written_to_stdout() {
        let output = serde_json::json!({
            "errors": ["The file could not be opened for reading: open file error.\n"],
            "warnings": []
        })
        .to_string();

        assert_eq!(
            probe_message(ToolKind::MkvMerge, &output, ""),
            "The file could not be opened for reading: open file error."
        );
    }

    #[test]
    fn probe_diagnostics_prefer_nonempty_stderr() {
        let output = serde_json::json!({ "errors": ["stdout error"] }).to_string();
        assert_eq!(
            probe_message(ToolKind::MkvMerge, &output, "stderr error\n"),
            "stderr error"
        );
    }

    #[test]
    fn parses_ffprobe_json() {
        let json = r#"{
          "streams":[
            {"index":0,"codec_type":"video","codec_name":"hevc","width":3840,"height":2160,"pix_fmt":"yuv420p10le","bits_per_raw_sample":"10","color_transfer":"smpte2084","disposition":{"default":1}},
            {"index":1,"codec_type":"subtitle","codec_name":"subrip","tags":{"language":"eng","title":"English Forced"},"disposition":{"forced":1}}
          ],
          "format":{"format_name":"mov,mp4","duration":"12.345","tags":{"title":"Movie"}}
        }"#;

        let media = parse_ffprobe_json(Path::new("movie.mp4"), json).expect("valid JSON");
        assert_eq!(media.container.duration_millis, Some(12_345));
        assert_eq!(media.tracks[0].kind, TrackType::Video);
        assert_eq!(media.tracks[0].bit_depth, Some(10));
        assert!(media.tracks[1].forced);
    }

    #[test]
    fn mkvmerge_golden_fixture_is_supported() {
        let cases = fixture_cases("mkvmerge");
        let first = &cases[0]["input"];
        let media = parse_mkvmerge_json(
            Path::new("fixture.mkv"),
            &serde_json::to_string(&first["payload"]).expect("payload"),
        )
        .expect("golden mkvmerge payload");

        assert_eq!(
            media.container.title.as_deref(),
            Some("Example Show - S01E02")
        );
        assert_eq!(media.container.format.as_deref(), Some("Matroska"));
        assert_eq!(media.tracks[0].codec, "HEVC/H.265");
        assert_eq!(
            (media.tracks[0].width, media.tracks[0].height),
            (Some(1920), Some(1080))
        );
        assert_eq!(media.tracks[0].bit_depth, Some(10));
        assert_eq!(media.tracks[4].language.as_deref(), Some("ja"));
        assert_eq!(
            media.attachments[1].file_name.as_deref(),
            Some("Example-Bold.otf")
        );
        assert_eq!(
            media.attachments[1].content_type.as_deref(),
            Some("font/otf")
        );
        assert_eq!(media.attachments[1].size_bytes, Some(4096));

        let warning = &cases[1]["input"];
        let warning_media = parse_mkvmerge_json(
            Path::new("warning.mkv"),
            &serde_json::to_string(&warning["payload"]).expect("payload"),
        )
        .expect("warning payload remains valid JSON");
        assert_eq!(warning_media.tracks.len(), 1);
        assert_eq!(warning_media.tracks[0].kind, TrackType::Audio);
    }

    #[test]
    fn ffprobe_golden_fixture_is_supported() {
        let cases = fixture_cases("ffprobe");
        let first = &cases[0]["input"];
        let media = parse_ffprobe_json(
            Path::new("Example Movie (2024).mp4"),
            &serde_json::to_string(&first["payload"]).expect("payload"),
        )
        .expect("golden ffprobe payload");

        assert_eq!(media.tracks[0].codec, "AVC/H.264");
        assert_eq!(media.tracks[0].bit_depth, Some(10));
        assert!(media.tracks[0].default);
        assert!(!media.tracks[2].default);
        assert!(media.tracks[3].forced);
        assert_eq!(media.tracks[4].kind, TrackType::Data);

        let empty = &cases[1]["input"];
        let empty_media = parse_ffprobe_json(
            Path::new("Empty.mp4"),
            &serde_json::to_string(&empty["payload"]).expect("payload"),
        )
        .expect("missing streams are an empty successful scan");
        assert!(empty_media.tracks.is_empty());
    }

    /// Real `mkvmerge -J` output emits `display_unit` as an enum code, not a
    /// string. Typing it as a string rejected the whole document, so every MKV
    /// with a video track failed to scan while MP4s succeeded.
    #[test]
    fn integer_display_unit_does_not_reject_the_document() {
        let json = serde_json::json!({
            "container": { "properties": { "title": "Example" } },
            "tracks": [{
                "id": 0,
                "type": "video",
                "codec": "AVC/H.264",
                "properties": {
                    "number": 1,
                    "language": "und",
                    "pixel_dimensions": "320x240",
                    "display_dimensions": "320x240",
                    "display_unit": 0,
                    "default_track": false,
                    "forced_track": false
                }
            }]
        })
        .to_string();

        let media = parse_mkvmerge_json(Path::new("ep1.mkv"), &json).expect("real mkvmerge output");
        assert_eq!(media.tracks.len(), 1);
        assert_eq!(media.tracks[0].width, Some(320));
        assert_eq!(media.tracks[0].height, Some(240));
    }

    /// A cosmetic property that changes type between MKVToolNix releases must
    /// degrade to "absent", never to a skipped file.
    #[test]
    fn unexpected_property_types_degrade_to_absent() {
        let json = serde_json::json!({
            "container": { "properties": {} },
            "tracks": [{
                "id": 0,
                "type": "audio",
                "codec": "AAC",
                "properties": {
                    "language": "eng",
                    "track_name": 42,
                    "pixel_dimensions": ["unexpected"]
                }
            }]
        })
        .to_string();

        let media = parse_mkvmerge_json(Path::new("ep1.mkv"), &json).expect("tolerant parse");
        assert_eq!(media.tracks[0].language.as_deref(), Some("eng"));
        assert_eq!(media.tracks[0].name, None);
    }
}
