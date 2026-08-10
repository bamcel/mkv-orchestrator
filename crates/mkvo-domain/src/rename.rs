use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{FileFingerprint, MetadataProvider, OperationPlan, PlanConflict, RenameBatchId};

pub type RenamePlan = OperationPlan<RenamePlanPayload>;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenamePlanPayload {
    pub template: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<MetadataProvider>,
    #[serde(default)]
    pub items: Vec<RenamePlanItem>,
}

impl RenamePlanPayload {
    #[must_use]
    pub fn rename_count(&self) -> usize {
        self.items.iter().filter(|item| item.can_apply()).count()
    }

    #[must_use]
    pub fn skip_count(&self) -> usize {
        self.items.len().saturating_sub(self.rename_count())
    }

    #[must_use]
    pub fn has_blocking_issues(&self) -> bool {
        self.items
            .iter()
            .flat_map(|item| &item.conflicts)
            .any(|conflict| conflict.blocking)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenamePlanItem {
    pub source: PathBuf,
    pub target: PathBuf,
    pub source_fingerprint: FileFingerprint,
    pub new_file_name: String,
    #[serde(default)]
    pub conflicts: Vec<PlanConflict>,
}

impl RenamePlanItem {
    #[must_use]
    pub fn can_apply(&self) -> bool {
        self.source != self.target && !self.conflicts.iter().any(|conflict| conflict.blocking)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameTokens {
    pub title: String,
    pub series: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub season: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub episode: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub absolute: Option<u32>,
    pub episode_title: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameBatchRecord {
    pub id: RenameBatchId,
    pub created_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub undone_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<MetadataProvider>,
    pub template: String,
    #[serde(default)]
    pub entries: Vec<RenameBatchEntry>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameBatchEntry {
    pub original_path: PathBuf,
    pub renamed_path: PathBuf,
    pub original_fingerprint: FileFingerprint,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub renamed_fingerprint: Option<FileFingerprint>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameUndoPreview {
    pub batch_id: RenameBatchId,
    #[serde(default)]
    pub items: Vec<RenameUndoItem>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameUndoItem {
    pub current_path: PathBuf,
    pub restore_path: PathBuf,
    #[serde(default)]
    pub conflicts: Vec<PlanConflict>,
}
