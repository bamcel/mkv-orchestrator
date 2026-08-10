use chrono::{DateTime, Utc};
use mkvo_domain::{
    FileFingerprint, MediaAttachment, MediaFile, MediaStatus, MediaTrack, TrackKind,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaFileDto {
    pub path: String,
    pub file_name: String,
    pub extension: String,
    pub status: MediaStatus,
    pub reader: String,
    pub container_title: String,
    pub codec: String,
    pub resolution: String,
    pub bit_depth: String,
    pub hdr: String,
    pub video_summary: String,
    pub audio_summary: String,
    pub subtitle_summary: String,
    pub attachment_summary: String,
    pub fingerprint: FileFingerprintDto,
    #[serde(default)]
    pub tracks: Vec<MediaTrackDto>,
    #[serde(default)]
    pub attachments: Vec<MediaAttachmentDto>,
}

impl From<&MediaFile> for MediaFileDto {
    fn from(file: &MediaFile) -> Self {
        let videos: Vec<_> = file.tracks_of_kind(TrackKind::Video).collect();
        let audios: Vec<_> = file.tracks_of_kind(TrackKind::Audio).collect();
        let subtitles: Vec<_> = file.tracks_of_kind(TrackKind::Subtitle).collect();
        let first_video = videos.first().copied();
        let codec = first_video.map_or_else(String::new, |track| track.codec.clone());
        let resolution = first_video
            .and_then(|track| track.resolution)
            .map_or_else(String::new, |resolution| resolution.to_string());
        let bit_depth = first_video
            .and_then(|track| track.bit_depth)
            .map_or_else(String::new, |value| format!("{value}bit"));
        let extension = file.extension();
        Self {
            path: file.path.to_string_lossy().into_owned(),
            file_name: file.file_name(),
            extension: if extension.is_empty() {
                String::new()
            } else {
                format!(".{extension}")
            },
            status: file.status,
            reader: reader_name(file),
            container_title: file.container.title.clone().unwrap_or_default(),
            video_summary: [codec.as_str(), resolution.as_str(), bit_depth.as_str()]
                .into_iter()
                .filter(|value| !value.trim().is_empty() && !value.eq_ignore_ascii_case("unknown"))
                .collect::<Vec<_>>()
                .join(" | "),
            codec,
            resolution,
            bit_depth,
            hdr: first_video
                .and_then(|track| track.hdr.clone())
                .unwrap_or_default(),
            audio_summary: language_summary(&audios),
            subtitle_summary: language_summary(&subtitles),
            attachment_summary: attachment_summary(&file.attachments),
            fingerprint: (&file.fingerprint).into(),
            tracks: file.tracks.iter().map(Into::into).collect(),
            attachments: file.attachments.iter().map(Into::into).collect(),
        }
    }
}

fn reader_name(file: &MediaFile) -> String {
    match file.extension().to_ascii_lowercase().as_str() {
        "mkv" | "mka" | "webm" => "mkvmerge".to_owned(),
        "mp4" | "m4v" => "ffprobe".to_owned(),
        _ => "rust".to_owned(),
    }
}

fn language_summary(tracks: &[&MediaTrack]) -> String {
    let mut groups: Vec<(String, usize)> = Vec::new();
    for track in tracks {
        let language = track.language_or_undetermined();
        if let Some((_, count)) = groups.iter_mut().find(|(value, _)| value == language) {
            *count += 1;
        } else {
            groups.push((language.to_owned(), 1));
        }
    }
    groups
        .into_iter()
        .map(|(language, count)| format!("{language} x{count}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn attachment_summary(attachments: &[MediaAttachment]) -> String {
    if attachments.is_empty() {
        return "None".to_owned();
    }
    let font_count = attachments
        .iter()
        .filter(|attachment| {
            let file_name = attachment.file_name.to_ascii_lowercase();
            let value = format!(
                "{} {}",
                attachment.file_name,
                attachment.content_type.as_deref().unwrap_or_default()
            )
            .to_ascii_lowercase();
            value.contains("font")
                || file_name.ends_with(".ttf")
                || file_name.ends_with(".otf")
                || file_name.ends_with(".ttc")
        })
        .count();
    let other_count = attachments.len() - font_count;
    let mut parts = Vec::new();
    if font_count > 0 {
        parts.push(format!("Fonts x{font_count}"));
    }
    if other_count > 0 {
        parts.push(format!("Other x{other_count}"));
    }
    parts.join(", ")
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileFingerprintDto {
    pub path: String,
    pub size_bytes: u64,
    pub modified_utc: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quick_hash: Option<String>,
}

impl From<&FileFingerprint> for FileFingerprintDto {
    fn from(value: &FileFingerprint) -> Self {
        Self {
            path: value.path.to_string_lossy().into_owned(),
            size_bytes: value.size_bytes,
            modified_utc: value.modified_at,
            quick_hash: value.quick_hash.clone(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaTrackDto {
    pub id: u64,
    pub track_number: u32,
    #[serde(rename = "type")]
    pub kind: TrackKind,
    pub codec: String,
    pub language: String,
    pub name: String,
    pub default: bool,
    pub forced: bool,
}

impl From<&MediaTrack> for MediaTrackDto {
    fn from(value: &MediaTrack) -> Self {
        Self {
            id: value.mkvmerge_id,
            track_number: value.propedit_track_number,
            kind: value.kind,
            codec: value.codec.clone(),
            language: value.language.clone().unwrap_or_default(),
            name: value.name.clone().unwrap_or_default(),
            default: value.default,
            forced: value.forced,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaAttachmentDto {
    pub id: u64,
    pub file_name: String,
    pub content_type: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
}

impl From<&MediaAttachment> for MediaAttachmentDto {
    fn from(value: &MediaAttachment) -> Self {
        Self {
            id: value.id,
            file_name: value.file_name.clone(),
            content_type: value.content_type.clone().unwrap_or_default(),
            description: value.description.clone().unwrap_or_default(),
            size_bytes: value.size_bytes,
        }
    }
}
