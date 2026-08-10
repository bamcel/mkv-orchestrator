use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{FileFingerprint, OperationPlan, PlanConflict, TrackKind};

pub type RemuxPlan = OperationPlan<RemuxPlanPayload>;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemuxPlanPayload {
    pub mode: RemuxMode,
    #[serde(default)]
    pub items: Vec<RemuxPlanItem>,
}

impl RemuxPlanPayload {
    #[must_use]
    pub fn runnable_count(&self) -> usize {
        self.items.iter().filter(|item| item.can_apply()).count()
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemuxMode {
    Remux,
    ConvertToMkv,
    MuxSubtitles,
    ExtractSubtitles,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemuxPlanItem {
    pub source: PathBuf,
    pub source_fingerprint: FileFingerprint,
    pub temporary_output: PathBuf,
    pub final_output: PathBuf,
    pub mode: RemuxMode,
    #[serde(default)]
    pub selected_track_ids: Vec<u64>,
    #[serde(default)]
    pub external_subtitles: Vec<ExternalSubtitle>,
    #[serde(default)]
    pub extract_tracks: Vec<TrackExtraction>,
    #[serde(default = "default_true")]
    pub preserve_chapters: bool,
    #[serde(default = "default_true")]
    pub preserve_attachments: bool,
    #[serde(default)]
    pub delete_source_after_success: bool,
    #[serde(default)]
    pub delete_external_subtitles_after_success: bool,
    #[serde(default)]
    pub conflicts: Vec<PlanConflict>,
}

impl RemuxPlanItem {
    #[must_use]
    pub fn can_apply(&self) -> bool {
        self.temporary_output != self.final_output
            && !self.conflicts.iter().any(|conflict| {
                conflict.blocking || conflict.kind == crate::PlanConflictKind::NoChange
            })
    }
}

const fn default_true() -> bool {
    true
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalSubtitle {
    pub path: PathBuf,
    pub language: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default)]
    pub default: bool,
    #[serde(default)]
    pub forced: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackExtraction {
    pub track_id: u64,
    pub kind: TrackKind,
    pub output: PathBuf,
}
