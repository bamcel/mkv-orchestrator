use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryAudit {
    pub root: PathBuf,
    #[serde(default)]
    pub groups: Vec<LibraryAuditGroup>,
    pub summary: LibraryAuditSummary,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryAuditGroup {
    pub watch_root: PathBuf,
    pub show_name: String,
    pub season_folder: String,
    pub relative_folder: PathBuf,
    #[serde(default)]
    pub all_file_paths: Vec<PathBuf>,
    #[serde(default)]
    pub issue_file_paths: Vec<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_file_path: Option<PathBuf>,
    pub standard: LibraryStandard,
    #[serde(default)]
    pub issues: Vec<LibraryIssue>,
}

impl LibraryAuditGroup {
    #[must_use]
    pub fn has_issues(&self) -> bool {
        !self.issues.is_empty()
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryStandard {
    pub video: String,
    pub audio: String,
    pub subtitles: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryIssue {
    pub kind: LibraryIssueKind,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    #[serde(default)]
    pub related_paths: Vec<PathBuf>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LibraryIssueKind {
    VideoMismatch,
    AudioMismatch,
    SubtitleMismatch,
    DuplicateEpisode,
    PossibleMissingEpisode,
    UncachedFile,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryAuditSummary {
    pub shows: usize,
    pub season_folders: usize,
    pub files: usize,
    pub issue_groups: usize,
    pub uncached_files: usize,
}
