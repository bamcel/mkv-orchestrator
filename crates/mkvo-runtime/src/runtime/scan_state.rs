use std::collections::BTreeSet;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use mkvo_application::ScanOutcome;
use mkvo_contracts::{CurrentScanResponse, JobSnapshot, MediaFileRow, ScanSummary};
use mkvo_domain::MediaFile;
use serde::{Deserialize, Serialize};

use super::display_path;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ScanResultState {
    pub(super) files: Vec<MediaFile>,
    pub(super) rows: Vec<MediaFileRow>,
    pub(super) skipped: Vec<String>,
    pub(super) summary: ScanSummary,
}

#[derive(Clone, Debug, Default)]
pub(super) struct CurrentScanState {
    pub(super) updated_utc: Option<DateTime<Utc>>,
    pub(super) files: Vec<MediaFile>,
    pub(super) summary: ScanSummary,
    /// Selected paths are normalized so equivalent Windows spellings match.
    pub(super) selected: BTreeSet<String>,
}

impl CurrentScanState {
    /// Move the authoritative working set and selection with completed renames.
    pub(super) fn apply_renames(&mut self, renames: &[(PathBuf, PathBuf)]) {
        for (source, target) in renames {
            let source_key = mkvo_application::paths::path_key(source);
            for file in &mut self.files {
                if mkvo_application::paths::path_key(&file.path) == source_key {
                    file.path = target.clone();
                }
            }
            if self.selected.remove(&source_key) {
                self.selected
                    .insert(mkvo_application::paths::path_key(target));
            }
        }
        self.files.sort_by(|left, right| left.path.cmp(&right.path));
    }

    pub(super) fn reconcile_selection(&mut self) {
        if self.files.is_empty() {
            return;
        }
        let available: BTreeSet<String> = self
            .files
            .iter()
            .map(|file| mkvo_application::paths::path_key(&file.path))
            .collect();
        self.selected.retain(|path| available.contains(path));
    }

    pub(super) fn selected_display_paths(&self) -> Vec<String> {
        self.files
            .iter()
            .filter(|file| {
                self.selected
                    .contains(&mkvo_application::paths::path_key(&file.path))
            })
            .map(|file| display_path(&file.path))
            .collect()
    }
}

pub(super) fn scan_result_state(outcome: &ScanOutcome) -> ScanResultState {
    ScanResultState {
        files: outcome.files.clone(),
        rows: outcome.files.iter().map(MediaFileRow::from).collect(),
        skipped: outcome
            .skipped
            .iter()
            .map(|skip| format!("{}: {}", skip.path.display(), skip.reason))
            .collect(),
        summary: outcome.summary,
    }
}

pub(super) fn current_scan_response(state: &CurrentScanState) -> CurrentScanResponse {
    CurrentScanResponse {
        updated_utc: state.updated_utc,
        files: state.files.iter().map(MediaFileRow::from).collect(),
        summary: state.summary,
        selected_paths: state.selected_display_paths(),
    }
}

pub(super) fn scan_state_from_snapshot(snapshot: &JobSnapshot) -> Option<ScanResultState> {
    serde_json::from_value(snapshot.result.clone()?).ok()
}
