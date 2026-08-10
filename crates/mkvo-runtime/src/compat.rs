//! Compatibility DTOs consumed by the existing React application.
//!
//! Domain contracts intentionally remain stricter. These projections preserve
//! the legacy camel-case/PascalCase JSON surface while adding immutable-plan
//! identifiers to every mutation request and preview response.

use chrono::{DateTime, Utc};
use mkvo_contracts::{
    JobSnapshot, JobStatus, MediaFileRow, MuxActionRow, PropEditActionRow, PropEditNoChangeRow,
    PropEditSkippedRow, PropEditTrackConfigRow, RenamePreviewRow, RenameScopeRow, TitleEditMode,
};
use mkvo_domain::{IdempotencyKey, PlanId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaServerConnectionRequest {
    pub id: Option<String>,
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub server_type: Option<String>,
    pub server_url: Option<String>,
    pub api_key: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameSearchRequest {
    pub query: String,
    pub provider: Option<String>,
    pub language: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameSearchResult {
    pub id: serde_json::Value,
    pub name: String,
    pub year: String,
    pub overview: String,
    pub provider: String,
    pub format: String,
    pub database_url: String,
    pub display_name: String,
    pub provider_display: String,
}

impl RenameSearchResult {
    #[must_use]
    pub fn media_id(&self) -> String {
        self.id
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| self.id.to_string())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameScopesRequest {
    pub selected_result: RenameSearchResult,
    pub provider: Option<String>,
    pub language: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameProviderTestRequest {
    pub provider: Option<String>,
    pub language: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenamePreviewRequest {
    #[serde(default)]
    pub files: Vec<MediaFileRow>,
    pub selected_result: RenameSearchResult,
    pub provider: Option<String>,
    pub language: Option<String>,
    #[serde(default)]
    pub scope_keys: Vec<String>,
    pub template: Option<String>,
    pub idempotency_key: Option<IdempotencyKey>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenamePreviewResponse {
    #[serde(default)]
    pub items: Vec<RenamePreviewRow>,
    pub summary: String,
    #[serde(default)]
    pub scopes: Vec<RenameScopeRow>,
    pub status: String,
    pub plan_id: Option<PlanId>,
    pub plan_fingerprint: Option<String>,
    pub idempotency_key: Option<IdempotencyKey>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameApplyRequest {
    #[serde(default)]
    pub items: Vec<RenamePreviewRow>,
    pub provider: Option<String>,
    pub template: Option<String>,
    pub plan_id: Option<PlanId>,
    pub plan_fingerprint: Option<String>,
    pub idempotency_key: Option<IdempotencyKey>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MuxPreviewRequest {
    #[serde(default)]
    pub files: Vec<MediaFileRow>,
    #[serde(default)]
    pub selected_paths: Vec<String>,
    pub remove_unwanted_audio_languages: bool,
    pub keep_audio_languages: String,
    pub remove_unwanted_subtitle_languages: bool,
    pub keep_subtitle_languages: String,
    pub remove_unwanted_track_ids: bool,
    pub remove_track_ids_text: String,
    pub preserve_chapters: bool,
    pub preserve_attachments: bool,
    pub mux_matching_external_subtitles: bool,
    pub external_subtitle_language: String,
    #[serde(default)]
    pub external_subtitle_track_name: Option<String>,
    pub external_subtitle_formats: String,
    pub preserve_external_subtitle_files: bool,
    pub skip_mux_if_subtitle_already_exists: bool,
    pub extract_subtitles: bool,
    pub extract_subtitle_languages: String,
    pub extract_overwrite_existing_files: bool,
    pub convert_mp4_to_mkv: bool,
    pub delete_mp4_after_convert: bool,
    pub plan_id: Option<PlanId>,
    pub plan_fingerprint: Option<String>,
    pub idempotency_key: Option<IdempotencyKey>,
}

pub type MuxApplyRequest = MuxPreviewRequest;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MuxPreviewResponse {
    #[serde(default)]
    pub actions: Vec<MuxActionRow>,
    #[serde(default)]
    pub no_change_files: Vec<String>,
    pub summary: String,
    pub status: String,
    pub plan_id: Option<PlanId>,
    pub plan_fingerprint: Option<String>,
    pub idempotency_key: Option<IdempotencyKey>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PropEditTemplateRequest {
    #[serde(default)]
    pub files: Vec<MediaFileRow>,
    pub template_path: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PropEditPreviewRequest {
    #[serde(default)]
    pub files: Vec<MediaFileRow>,
    #[serde(default)]
    pub selected_paths: Vec<String>,
    pub template_path: Option<String>,
    pub container_title_mode: TitleEditMode,
    pub custom_container_title: String,
    pub video_title_mode: TitleEditMode,
    pub custom_video_title: String,
    #[serde(default)]
    pub audio_tracks: Vec<PropEditTrackConfigRow>,
    #[serde(default)]
    pub subtitle_tracks: Vec<PropEditTrackConfigRow>,
    pub selected_default_audio: String,
    pub selected_forced_audio: String,
    pub selected_default_subtitle: String,
    pub selected_forced_subtitle: String,
    pub plan_id: Option<PlanId>,
    pub plan_fingerprint: Option<String>,
    pub idempotency_key: Option<IdempotencyKey>,
}

pub type PropEditApplyRequest = PropEditPreviewRequest;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PropEditPreviewResponse {
    #[serde(default)]
    pub actions: Vec<PropEditActionRow>,
    #[serde(default)]
    pub skipped: Vec<PropEditSkippedRow>,
    #[serde(default)]
    pub no_change: Vec<PropEditNoChangeRow>,
    pub summary: String,
    pub status: String,
    pub plan_id: Option<PlanId>,
    pub plan_fingerprint: Option<String>,
    pub idempotency_key: Option<IdempotencyKey>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryAuditRequest {
    #[serde(default)]
    pub files: Vec<MediaFileRow>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationJobResponse {
    pub id: String,
    pub kind: String,
    pub status: JobStatus,
    pub created_utc: DateTime<Utc>,
    pub started_utc: Option<DateTime<Utc>>,
    pub completed_utc: Option<DateTime<Utc>>,
    pub completed: u64,
    pub failed: u64,
    pub skipped: u64,
    pub total: u64,
    pub current_file: String,
    pub current_file_percent: u8,
    #[serde(default)]
    pub lines: Vec<String>,
    pub mux_result: Option<MuxPreviewResponse>,
    pub prop_edit_result: Option<PropEditPreviewResponse>,
    pub error: String,
}

impl OperationJobResponse {
    #[must_use]
    pub fn from_snapshot(snapshot: &JobSnapshot) -> Self {
        let (mux_result, prop_edit_result) =
            snapshot.result.as_ref().map_or((None, None), |value| {
                (
                    serde_json::from_value(value.clone()).ok(),
                    serde_json::from_value(value.clone()).ok(),
                )
            });
        let kind = match snapshot.kind {
            mkvo_contracts::JobKind::PropertyEdit => "propedit",
            _ => "mux",
        };
        Self {
            id: snapshot.id.to_string(),
            kind: kind.to_owned(),
            status: snapshot.status,
            created_utc: snapshot.created_utc,
            started_utc: snapshot.started_utc,
            completed_utc: snapshot.completed_utc,
            completed: snapshot.completed,
            failed: snapshot.failed,
            skipped: snapshot.skipped,
            total: snapshot.total,
            current_file: snapshot.current_file.clone(),
            current_file_percent: snapshot.current_file_percent,
            lines: snapshot.lines.clone(),
            mux_result,
            prop_edit_result,
            error: snapshot.error.clone().unwrap_or_default(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PropEditTemplateResponse {
    pub template_path: String,
    pub template_file_name: String,
    #[serde(default)]
    pub audio_tracks: Vec<PropEditTrackConfigRow>,
    #[serde(default)]
    pub subtitle_tracks: Vec<PropEditTrackConfigRow>,
    pub default_audio: String,
    pub forced_audio: String,
    pub default_subtitle: String,
    pub forced_subtitle: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationLogResponse {
    #[serde(default)]
    pub entries: Vec<mkvo_contracts::OperationLogEntry>,
}
