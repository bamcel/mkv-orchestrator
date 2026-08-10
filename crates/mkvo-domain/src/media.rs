use std::path::{Path, PathBuf};

use chrono::Duration;
use serde::{Deserialize, Serialize};

use crate::FileFingerprint;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaFile {
    pub path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_file_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watch_root: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relative_path: Option<PathBuf>,
    pub fingerprint: FileFingerprint,
    pub container: ContainerMetadata,
    #[serde(default)]
    pub tracks: Vec<MediaTrack>,
    #[serde(default)]
    pub attachments: Vec<MediaAttachment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub episode: Option<EpisodeIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_match: Option<ProviderMatch>,
    #[serde(default)]
    pub status: MediaStatus,
}

impl MediaFile {
    #[must_use]
    pub fn file_name(&self) -> String {
        self.path
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned())
    }

    #[must_use]
    pub fn extension(&self) -> String {
        self.path
            .extension()
            .map_or_else(String::new, |value| value.to_string_lossy().into_owned())
    }

    pub fn tracks_of_kind(&self, kind: TrackKind) -> impl Iterator<Item = &MediaTrack> {
        self.tracks.iter().filter(move |track| track.kind == kind)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerMetadata {
    pub kind: ContainerKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_millis: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub muxing_application: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub writing_application: Option<String>,
}

impl Default for ContainerMetadata {
    fn default() -> Self {
        Self {
            kind: ContainerKind::Unknown,
            title: None,
            duration_millis: None,
            muxing_application: None,
            writing_application: None,
        }
    }
}

impl ContainerMetadata {
    #[must_use]
    pub fn duration(&self) -> Option<Duration> {
        self.duration_millis.map(Duration::milliseconds)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContainerKind {
    Matroska,
    WebM,
    Mp4,
    Other(String),
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaTrack {
    pub mkvmerge_id: u64,
    pub propedit_track_number: u32,
    pub kind: TrackKind,
    pub codec: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codec_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<VideoResolution>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bit_depth: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hdr: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channels: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sampling_frequency_hz: Option<u32>,
    #[serde(default)]
    pub default: bool,
    #[serde(default)]
    pub forced: bool,
    #[serde(default)]
    pub enabled: bool,
}

impl MediaTrack {
    #[must_use]
    pub fn language_or_undetermined(&self) -> &str {
        self.language
            .as_deref()
            .filter(|v| !v.is_empty())
            .unwrap_or("und")
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackKind {
    Video,
    Audio,
    Subtitle,
    Buttons,
    Other,
}

impl TrackKind {
    #[must_use]
    pub const fn propedit_prefix(self) -> &'static str {
        match self {
            Self::Video => "v",
            Self::Audio => "a",
            Self::Subtitle => "s",
            Self::Buttons | Self::Other => "",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoResolution {
    pub width: u32,
    pub height: u32,
}

impl std::fmt::Display for VideoResolution {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}x{}", self.width, self.height)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaAttachment {
    pub id: u64,
    pub file_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EpisodeIdentity {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub series_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub season: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub episode: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub absolute_episode: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub episode_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<u16>,
    #[serde(default)]
    pub is_movie: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderMatch {
    pub provider: MetadataProvider,
    pub media_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub episode_id: Option<String>,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub episode_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<u8>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetadataProvider {
    Tvdb,
    Tmdb,
    AniDb,
    AniList,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSearchResult {
    pub provider: MetadataProvider,
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overview: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EpisodeMetadata {
    pub id: String,
    pub season: u32,
    pub episode: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub absolute_episode: Option<u32>,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aired_at: Option<chrono::NaiveDate>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaStatus {
    #[default]
    Ready,
    Cached,
    Scanning,
    Warning,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VisualState {
    Normal,
    Template,
    Warning,
    Error,
    Muted,
}

#[must_use]
pub fn is_supported_media_path(path: &Path) -> bool {
    path.extension().is_some_and(|extension| {
        extension.eq_ignore_ascii_case("mkv")
            || extension.eq_ignore_ascii_case("mka")
            || extension.eq_ignore_ascii_case("webm")
            || extension.eq_ignore_ascii_case("mp4")
            || extension.eq_ignore_ascii_case("m4v")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_extensions_are_case_insensitive() {
        assert!(is_supported_media_path(Path::new("Episode.MKV")));
        assert!(is_supported_media_path(Path::new("movie.mp4")));
        assert!(!is_supported_media_path(Path::new("notes.txt")));
    }
}
