use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use mkvo_domain::{
    AppSettings, IdempotencyKey, LibraryAudit, MediaFile, MediaServerKind, MetadataProvider,
    PlanId, PropertyEditPlan, RemuxMode, RemuxPlan, RenameBatchId, RenamePlan, TrackKind,
};
use serde::{Deserialize, Serialize};

use crate::{JobSnapshot, JobStatus, MediaAttachmentDto, MediaFileDto};
use mkvo_domain::JobId;

/// Compatibility projection consumed by the existing Dashboard components.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackRow {
    pub id: u64,
    pub track_number: u32,
    #[serde(rename = "type")]
    pub track_type: String,
    pub codec: String,
    pub language: String,
    pub name: String,
    pub default: bool,
    pub forced: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaFileRow {
    pub path: String,
    pub file_name: String,
    pub extension: String,
    pub status: String,
    pub reader: String,
    pub codec: String,
    pub resolution: String,
    pub bit_depth: String,
    pub hdr: String,
    pub video_summary: String,
    pub audio_summary: String,
    pub subtitle_summary: String,
    pub attachment_summary: String,
    #[serde(default)]
    pub tracks: Vec<TrackRow>,
    #[serde(default)]
    pub attachments: Vec<MediaAttachmentDto>,
}

impl From<&MediaFile> for MediaFileRow {
    fn from(file: &MediaFile) -> Self {
        let dto = MediaFileDto::from(file);
        Self {
            path: dto.path,
            file_name: dto.file_name,
            extension: dto.extension,
            status: compatibility_status(file),
            reader: dto.reader,
            codec: dto.codec,
            resolution: dto.resolution,
            bit_depth: dto.bit_depth,
            hdr: dto.hdr,
            video_summary: dto.video_summary,
            audio_summary: dto.audio_summary,
            subtitle_summary: dto.subtitle_summary,
            attachment_summary: dto.attachment_summary,
            tracks: file
                .tracks
                .iter()
                .map(|track| TrackRow {
                    id: track.mkvmerge_id,
                    track_number: track.propedit_track_number,
                    track_type: match track.kind {
                        TrackKind::Video => "video",
                        TrackKind::Audio => "audio",
                        TrackKind::Subtitle => "subtitles",
                        TrackKind::Buttons => "buttons",
                        TrackKind::Other if track.codec.eq_ignore_ascii_case("bin_data") => "data",
                        TrackKind::Other => "other",
                    }
                    .to_owned(),
                    codec: track.codec.clone(),
                    language: track.language_or_undetermined().to_owned(),
                    name: track.name.clone().unwrap_or_default(),
                    default: track.default,
                    forced: track.forced,
                })
                .collect(),
            attachments: dto.attachments,
        }
    }
}

fn compatibility_status(file: &MediaFile) -> String {
    if file.status == mkvo_domain::MediaStatus::Failed {
        return "Failed".to_owned();
    }
    if file.status == mkvo_domain::MediaStatus::Scanning {
        return "Scanning".to_owned();
    }
    if file.status == mkvo_domain::MediaStatus::Cached {
        return "Cached".to_owned();
    }
    let mut value = if file.tracks.is_empty() {
        "Scanned - no tracks found".to_owned()
    } else if file
        .tracks
        .iter()
        .any(|track| track.kind == TrackKind::Video)
    {
        "Scanned".to_owned()
    } else {
        "Scanned - no video track".to_owned()
    };
    if file.status == mkvo_domain::MediaStatus::Warning {
        value.push_str(" / mkvmerge warning");
    }
    value
}

// Dashboard and scan contracts ------------------------------------------------

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
    #[serde(default)]
    pub sources: Vec<String>,
    #[serde(default)]
    pub ignored_folder_names: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mkv_merge_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ff_probe_path: Option<String>,
    #[serde(default)]
    pub force_refresh: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_workers: Option<usize>,
}

impl ScanRequest {
    #[must_use]
    pub fn all_sources(&self) -> Vec<&str> {
        self.source_path
            .iter()
            .map(String::as_str)
            .chain(self.sources.iter().map(String::as_str))
            .collect()
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanSummary {
    pub total: u64,
    pub mkv: u64,
    pub mp4: u64,
    pub failed: u64,
    pub cached: u64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentScanResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_utc: Option<DateTime<Utc>>,
    #[serde(default)]
    pub files: Vec<MediaFileRow>,
    pub summary: ScanSummary,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResponse {
    pub files: Vec<MediaFileDto>,
    pub skipped: Vec<String>,
    pub summary: ScanSummary,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanJobResponse {
    pub id: JobId,
    pub status: JobStatus,
    pub created_utc: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_utc: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_utc: Option<DateTime<Utc>>,
    pub current_source: String,
    pub completed: u64,
    pub total: u64,
    #[serde(default)]
    pub files: Vec<MediaFileRow>,
    #[serde(default)]
    pub skipped: Vec<String>,
    pub summary: ScanSummary,
    pub error: String,
}

impl ScanJobResponse {
    #[must_use]
    pub fn from_snapshot(
        job: &JobSnapshot,
        files: Vec<MediaFileRow>,
        skipped: Vec<String>,
        summary: ScanSummary,
    ) -> Self {
        Self {
            id: job.id,
            status: job.status,
            created_utc: job.created_utc,
            started_utc: job.started_utc,
            completed_utc: job.completed_utc,
            current_source: job.current_file.clone(),
            completed: job.completed,
            total: job.total,
            files,
            skipped,
            summary,
            error: job.error.clone().unwrap_or_default(),
        }
    }
}

// Settings and media-server contracts ----------------------------------------

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsResponse {
    pub settings: AppSettings,
    pub revision: u64,
    #[serde(default)]
    pub secret_status: Vec<SecretStatus>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveSettingsRequest {
    pub settings: AppSettings,
    #[serde(default)]
    pub secrets: Vec<SecretUpdate>,
    #[serde(default)]
    pub expected_revision: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretStatus {
    pub key: String,
    pub configured: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub masked_hint: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretUpdate {
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(default)]
    pub clear: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeDefinition {
    pub name: String,
    #[serde(default)]
    pub colors: BTreeMap<String, String>,
}

const fn default_max_scan_workers() -> usize {
    4
}

const fn default_max_edit_workers() -> usize {
    2
}

const fn default_max_remux_workers() -> usize {
    1
}

const fn default_watch_debounce_millis() -> u64 {
    750
}

const fn default_watch_reconciliation_interval_minutes() -> u64 {
    30
}

fn default_selected_theme_name() -> String {
    "Dark".to_owned()
}

/// Compatibility view matching the existing React settings page.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebSettings {
    pub has_tvdb_api_key: bool,
    pub has_tvdb_pin: bool,
    pub has_tmdb_api_key: bool,
    pub tvdb_language: String,
    pub rename_lookup_provider: String,
    pub rename_template: String,
    #[serde(default)]
    pub rename_templates: Vec<String>,
    #[serde(default)]
    pub audio_name_presets: Vec<String>,
    #[serde(default)]
    pub subtitle_name_presets: Vec<String>,
    #[serde(default)]
    pub language_presets: Vec<String>,
    pub mkv_merge_default_audio_languages: String,
    pub mkv_merge_default_subtitle_languages: String,
    #[serde(default)]
    pub mkv_tool_nix_directory: Option<String>,
    #[serde(default)]
    pub ffmpeg_directory: Option<String>,
    #[serde(default)]
    pub default_root: Option<String>,
    #[serde(default)]
    pub ignored_scan_folder_names: Vec<String>,
    #[serde(default)]
    pub use_quick_hash_on_unreliable_timestamps: bool,
    #[serde(default)]
    pub rename_preview_compact_view: bool,
    #[serde(default = "default_max_scan_workers")]
    pub max_scan_workers: usize,
    #[serde(default = "default_max_edit_workers")]
    pub max_edit_workers: usize,
    #[serde(default = "default_max_remux_workers")]
    pub max_remux_workers: usize,
    #[serde(default)]
    pub watch_folders: Vec<String>,
    pub enable_live_watch_folder_monitoring: bool,
    #[serde(default = "default_watch_debounce_millis")]
    pub watch_debounce_millis: u64,
    #[serde(default = "default_watch_reconciliation_interval_minutes")]
    pub watch_reconciliation_interval_minutes: u64,
    #[serde(default)]
    pub watch_force_polling: bool,
    #[serde(default = "default_selected_theme_name")]
    pub selected_theme_name: String,
    #[serde(default)]
    pub custom_themes: Vec<ThemeDefinition>,
    #[serde(default)]
    pub media_servers: Vec<WebMediaServer>,
    #[serde(default)]
    pub media_server_path_mappings: Vec<WebMediaServerPathMapping>,
}

impl Default for WebSettings {
    fn default() -> Self {
        Self {
            has_tvdb_api_key: false,
            has_tvdb_pin: false,
            has_tmdb_api_key: false,
            tvdb_language: String::new(),
            rename_lookup_provider: String::new(),
            rename_template: String::new(),
            rename_templates: Vec::new(),
            audio_name_presets: Vec::new(),
            subtitle_name_presets: Vec::new(),
            language_presets: Vec::new(),
            mkv_merge_default_audio_languages: String::new(),
            mkv_merge_default_subtitle_languages: String::new(),
            mkv_tool_nix_directory: None,
            ffmpeg_directory: None,
            default_root: None,
            ignored_scan_folder_names: Vec::new(),
            use_quick_hash_on_unreliable_timestamps: false,
            rename_preview_compact_view: false,
            max_scan_workers: default_max_scan_workers(),
            max_edit_workers: default_max_edit_workers(),
            max_remux_workers: default_max_remux_workers(),
            watch_folders: Vec::new(),
            enable_live_watch_folder_monitoring: false,
            watch_debounce_millis: default_watch_debounce_millis(),
            watch_reconciliation_interval_minutes: default_watch_reconciliation_interval_minutes(),
            watch_force_polling: false,
            selected_theme_name: default_selected_theme_name(),
            custom_themes: Vec::new(),
            media_servers: Vec::new(),
            media_server_path_mappings: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebSettingsRequest {
    pub tvdb_api_key: Option<String>,
    pub tvdb_pin: Option<String>,
    pub tmdb_api_key: Option<String>,
    pub tvdb_language: Option<String>,
    pub rename_lookup_provider: Option<String>,
    pub rename_template: Option<String>,
    pub rename_templates: Option<Vec<String>>,
    pub audio_name_presets: Option<Vec<String>>,
    pub subtitle_name_presets: Option<Vec<String>>,
    pub language_presets: Option<Vec<String>>,
    pub mkv_merge_default_audio_languages: Option<String>,
    pub mkv_merge_default_subtitle_languages: Option<String>,
    pub mkv_tool_nix_directory: Option<Option<String>>,
    pub ffmpeg_directory: Option<Option<String>>,
    pub default_root: Option<Option<String>>,
    pub ignored_scan_folder_names: Option<Vec<String>>,
    pub use_quick_hash_on_unreliable_timestamps: Option<bool>,
    pub rename_preview_compact_view: Option<bool>,
    pub max_scan_workers: Option<usize>,
    pub max_edit_workers: Option<usize>,
    pub max_remux_workers: Option<usize>,
    pub watch_folders: Option<Vec<String>>,
    pub enable_live_watch_folder_monitoring: Option<bool>,
    pub watch_debounce_millis: Option<u64>,
    pub watch_reconciliation_interval_minutes: Option<u64>,
    pub watch_force_polling: Option<bool>,
    pub selected_theme_name: Option<String>,
    pub custom_themes: Option<Vec<ThemeDefinition>>,
    pub media_servers: Option<Vec<WebMediaServerRequest>>,
    pub media_server_path_mappings: Option<Vec<WebMediaServerPathMapping>>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebMediaServerLibraryPath {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub media_type: String,
    pub server_path: String,
    pub container_path: String,
    pub is_enabled: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebMediaServer {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub server_type: String,
    pub server_url: String,
    pub has_api_key: bool,
    pub is_default: bool,
    pub last_synced_utc: Option<DateTime<Utc>>,
    #[serde(default)]
    pub libraries: Vec<WebMediaServerLibraryPath>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebMediaServerRequest {
    pub id: Option<String>,
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub server_type: Option<String>,
    pub server_url: Option<String>,
    pub api_key: Option<String>,
    pub is_default: bool,
    pub libraries: Option<Vec<WebMediaServerLibraryPath>>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebMediaServerPathMapping {
    pub server_path_prefix: String,
    pub container_path_prefix: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaServerConnectionRequest {
    pub id: Option<String>,
    pub name: Option<String>,
    pub kind: MediaServerKind,
    pub server_url: String,
    pub api_key: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaServerTestResponse {
    pub success: bool,
    pub status: String,
    pub library_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaServerSyncResponse {
    pub server: WebMediaServer,
    pub libraries: Vec<WebMediaServerLibraryPath>,
    pub status: String,
}

// Rename contracts ------------------------------------------------------------

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameSearchRequest {
    pub provider: MetadataProvider,
    pub query: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameSearchResult {
    pub id: String,
    pub name: String,
    pub year: String,
    pub overview: String,
    pub provider: String,
    pub format: String,
    pub database_url: String,
    pub display_name: String,
    pub provider_display: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameScopesRequest {
    pub provider: MetadataProvider,
    pub media_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameScopeRow {
    pub key: String,
    pub label: String,
    pub is_selected: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenamePreviewRequest {
    #[serde(default)]
    pub files: Vec<MediaFileDto>,
    #[serde(default)]
    pub selected_paths: Vec<String>,
    pub template: String,
    pub provider: Option<MetadataProvider>,
    #[serde(default)]
    pub check_existing_files: bool,
    #[serde(default = "default_plan_ttl_seconds")]
    pub expires_in_seconds: u64,
    pub idempotency_key: Option<IdempotencyKey>,
}

const fn default_plan_ttl_seconds() -> u64 {
    900
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenamePreviewRow {
    pub selected: bool,
    pub source_path: String,
    pub current_file_name: String,
    pub detected: String,
    pub episode_name: String,
    pub new_file_name: String,
    pub confidence: String,
    pub status: String,
    pub can_apply: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenamePreviewResponse {
    pub items: Vec<RenamePreviewRow>,
    pub summary: String,
    pub scopes: Vec<RenameScopeRow>,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_id: Option<PlanId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_fingerprint: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenamePlanResponse {
    pub plan: RenamePlan,
    pub summary: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyPlanRequest {
    pub plan_id: PlanId,
    pub fingerprint: String,
    pub idempotency_key: IdempotencyKey,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameBatchListRequest {
    #[serde(default = "default_batch_limit")]
    pub limit: usize,
}

const fn default_batch_limit() -> usize {
    50
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameUndoRequest {
    pub batch_id: RenameBatchId,
    pub idempotency_key: IdempotencyKey,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameProviderTestResponse {
    pub success: bool,
    pub status: String,
}

// Mux, remux, conversion, and extraction contracts ---------------------------

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MuxPreviewRequest {
    #[serde(default)]
    pub files: Vec<MediaFileDto>,
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
    pub external_subtitle_formats: String,
    pub preserve_external_subtitle_files: bool,
    pub skip_mux_if_subtitle_already_exists: bool,
    pub extract_subtitles: bool,
    pub extract_subtitle_languages: String,
    pub extract_overwrite_existing_files: bool,
    pub convert_mp4_to_mkv: bool,
    pub delete_mp4_after_convert: bool,
    #[serde(default = "default_plan_ttl_seconds")]
    pub expires_in_seconds: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<IdempotencyKey>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemuxPreviewRequest {
    pub mode: RemuxMode,
    #[serde(default)]
    pub files: Vec<MediaFileDto>,
    #[serde(default)]
    pub selected_paths: Vec<String>,
    #[serde(default)]
    pub keep_audio_languages: Vec<String>,
    #[serde(default)]
    pub keep_subtitle_languages: Vec<String>,
    #[serde(default)]
    pub remove_track_ids: Vec<u64>,
    #[serde(default)]
    pub preserve_chapters: bool,
    #[serde(default)]
    pub preserve_attachments: bool,
    #[serde(default)]
    pub delete_source_after_success: bool,
    #[serde(default = "default_plan_ttl_seconds")]
    pub expires_in_seconds: u64,
    pub idempotency_key: Option<IdempotencyKey>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MuxActionRow {
    pub index: usize,
    pub file_path: String,
    pub file_name: String,
    pub operation: String,
    pub tool_name: String,
    pub description: String,
    /// Display-only redacted command. It is never executable frontend input.
    pub command: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MuxPreviewResponse {
    #[serde(default)]
    pub actions: Vec<MuxActionRow>,
    #[serde(default)]
    pub no_change_files: Vec<String>,
    pub summary: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_id: Option<PlanId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_fingerprint: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemuxPlanResponse {
    pub plan: RemuxPlan,
    pub summary: String,
}

// Track-properties contracts --------------------------------------------------

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PropEditTrackConfigRow {
    pub track_number: u32,
    pub track_label: String,
    #[serde(rename = "type")]
    pub track_type: String,
    pub current_name: String,
    pub current_language: String,
    pub current_default: bool,
    pub edited_name: String,
    pub edited_language: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PropEditTemplateRequest {
    pub template_path: String,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TitleEditMode {
    Keep,
    File,
    Custom,
    Remove,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PropEditPreviewRequest {
    #[serde(default)]
    pub files: Vec<MediaFileDto>,
    #[serde(default)]
    pub selected_paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
    #[serde(default = "default_plan_ttl_seconds")]
    pub expires_in_seconds: u64,
    pub idempotency_key: Option<IdempotencyKey>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PropEditActionRow {
    pub index: usize,
    pub file_path: String,
    pub file_name: String,
    pub description: String,
    pub command: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PropEditSkippedRow {
    pub file_path: String,
    pub file_name: String,
    pub reason: String,
}

pub type PropEditNoChangeRow = PropEditSkippedRow;

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_id: Option<PlanId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_fingerprint: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PropertyEditPlanResponse {
    pub plan: PropertyEditPlan,
    pub summary: String,
}

// Library audit contracts -----------------------------------------------------

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryAuditRequest {
    pub root: String,
    #[serde(default)]
    pub ignored_folder_names: Vec<String>,
    #[serde(default)]
    pub include_uncached: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryAuditDomainResponse {
    pub audit: LibraryAudit,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryAuditSummary {
    pub groups: usize,
    pub files: usize,
    pub issue_groups: usize,
    pub standard_groups: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryAuditRow {
    pub folder_path: String,
    pub folder_name: String,
    pub file_count: usize,
    pub standard_video: String,
    pub standard_audio: String,
    pub standard_subtitles: String,
    pub template_file_path: String,
    pub template_file_name: String,
    pub has_issues: bool,
    pub issue_summary: String,
    #[serde(default)]
    pub issues: Vec<String>,
    #[serde(default)]
    pub issue_file_paths: Vec<String>,
    #[serde(default)]
    pub all_file_paths: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryAuditResponse {
    pub summary: LibraryAuditSummary,
    #[serde(default)]
    pub items: Vec<LibraryAuditRow>,
}

// Compatibility result/history contracts -------------------------------------

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameApplyResponse {
    #[serde(default)]
    pub items: Vec<RenamePreviewRow>,
    pub summary: String,
    pub status: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameBatchEntryDto {
    pub original_path: String,
    pub renamed_path: String,
    pub original_file_name: String,
    pub renamed_file_name: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameBatchRecordDto {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub undone_at: Option<DateTime<Utc>>,
    pub provider: String,
    pub template: String,
    pub total_files: usize,
    #[serde(default)]
    pub entries: Vec<RenameBatchEntryDto>,
    pub is_undone: bool,
    pub display_name: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameBatchListResponse {
    #[serde(default)]
    pub batches: Vec<RenameBatchRecordDto>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameBatchUndoPreviewResponse {
    pub restorable: usize,
    pub skipped: usize,
    #[serde(default)]
    pub lines: Vec<String>,
    pub has_skipped_files: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameBatchRestoreMove {
    pub original_path: String,
    pub renamed_path: String,
    pub original_file_name: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameBatchUndoResponse {
    pub renamed: usize,
    pub skipped: usize,
    #[serde(default)]
    pub lines: Vec<String>,
    #[serde(default)]
    pub restored: Vec<RenameBatchRestoreMove>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationJobResponse {
    #[serde(flatten)]
    pub job: JobSnapshot,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mux_result: Option<MuxPreviewResponse>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prop_edit_result: Option<PropEditPreviewResponse>,
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn scan_request_uses_camel_case_compatibility_names() {
        let request = ScanRequest {
            source_path: Some("C:/media".to_owned()),
            force_refresh: true,
            ..ScanRequest::default()
        };
        let json = serde_json::to_value(request).unwrap();
        assert_eq!(json["sourcePath"], "C:/media");
        assert_eq!(json["forceRefresh"], true);
    }

    #[test]
    fn settings_never_require_secret_values_in_a_response() {
        let view = WebSettings {
            has_tvdb_api_key: true,
            ..WebSettings::default()
        };
        let json = serde_json::to_string(&view).unwrap();
        assert!(json.contains("hasTvdbApiKey"));
        assert!(!json.contains("tvdbApiKey"));
    }

    #[test]
    fn media_row_matches_legacy_scan_summary_semantics() {
        let file = MediaFile {
            path: PathBuf::from("Example Show - S01E02.mkv"),
            original_file_name: None,
            watch_root: None,
            relative_path: None,
            fingerprint: mkvo_domain::FileFingerprint {
                path: PathBuf::from("Example Show - S01E02.mkv"),
                size_bytes: 42,
                modified_at: Utc::now(),
                quick_hash: None,
            },
            container: mkvo_domain::ContainerMetadata::default(),
            tracks: vec![
                track(
                    0,
                    TrackKind::Video,
                    "und",
                    "HEVC/H.265",
                    Some((1920, 1080)),
                    Some(10),
                ),
                track(1, TrackKind::Audio, "jpn", "AAC", None, None),
                track(2, TrackKind::Audio, "eng", "AC-3", None, None),
                track(3, TrackKind::Subtitle, "eng", "SubStationAlpha", None, None),
                track(4, TrackKind::Subtitle, "ja", "SubRip/SRT", None, None),
            ],
            attachments: vec![
                mkvo_domain::MediaAttachment {
                    id: 1,
                    file_name: "Example-Regular.ttf".to_owned(),
                    content_type: Some("application/x-truetype-font".to_owned()),
                    description: None,
                    size_bytes: Some(2048),
                },
                mkvo_domain::MediaAttachment {
                    id: 2,
                    file_name: "cover.jpg".to_owned(),
                    content_type: Some("image/jpeg".to_owned()),
                    description: None,
                    size_bytes: Some(1024),
                },
            ],
            episode: None,
            provider_match: None,
            status: mkvo_domain::MediaStatus::Ready,
        };

        let row = MediaFileRow::from(&file);
        assert_eq!(row.status, "Scanned");
        assert_eq!(row.reader, "mkvmerge");
        assert_eq!(row.codec, "HEVC/H.265");
        assert_eq!(row.resolution, "1920x1080");
        assert_eq!(row.bit_depth, "10bit");
        assert_eq!(row.video_summary, "HEVC/H.265 | 1920x1080 | 10bit");
        assert_eq!(row.audio_summary, "jpn x1, eng x1");
        assert_eq!(row.subtitle_summary, "eng x1, ja x1");
        assert_eq!(row.attachment_summary, "Fonts x1, Other x1");
    }

    fn track(
        id: u64,
        kind: TrackKind,
        language: &str,
        codec: &str,
        resolution: Option<(u32, u32)>,
        bit_depth: Option<u8>,
    ) -> mkvo_domain::MediaTrack {
        mkvo_domain::MediaTrack {
            mkvmerge_id: id,
            propedit_track_number: u32::try_from(id + 1).expect("fixture track number"),
            kind,
            codec: codec.to_owned(),
            codec_id: None,
            language: Some(language.to_owned()),
            name: None,
            resolution: resolution
                .map(|(width, height)| mkvo_domain::VideoResolution { width, height }),
            bit_depth,
            hdr: None,
            channels: None,
            sampling_frequency_hz: None,
            default: false,
            forced: false,
            enabled: true,
        }
    }
}
