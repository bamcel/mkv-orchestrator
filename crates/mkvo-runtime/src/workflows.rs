use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use chrono::{Duration, Utc};
use futures::{StreamExt, stream};
use mkvo_application::{
    ApplicationError, FileAccessState, JobSpec, LibraryAuditService, PropertyEditPlanRequest,
    PropertyEditPlanner, RemuxOptions, RemuxPlanRequest, RemuxPlanner, RenamePlanRequest,
    RenamePlanner, RequiredAccess, TextEdit, ToolInvocation, TrackEditIntent,
};
use mkvo_contracts::{
    JobCompletion, JobKind, JobLogLevel, LibraryAuditResponse, LibraryAuditRow,
    LibraryAuditSummary, LogLevel, OperationLogEntry, PropEditTrackConfigRow, RenameApplyResponse,
    RenameBatchEntryDto, RenameBatchListResponse, RenameBatchRecordDto, RenameBatchRestoreMove,
    RenameBatchUndoPreviewResponse, RenameBatchUndoResponse, TitleEditMode,
};
use mkvo_domain::{
    ContainerKind, ContainerMetadata, EpisodeIdentity, ExternalSubtitle, FileFingerprint,
    IdempotencyKey, MediaAttachment, MediaFile, MediaStatus, MediaTrack, MetadataProvider,
    OperationPlan, PlanId, PropertyEditPlan, ProviderMatch, RemuxMode, RemuxPlan, RenameBatchEntry,
    RenameBatchId, RenameBatchRecord, RenamePlan, StoredPlan, ToolFingerprint, ToolFingerprints,
    TrackExtraction, TrackKind, stable_fingerprint,
};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::compat::{
    LibraryAuditRequest, MuxPreviewRequest, MuxPreviewResponse, PropEditPreviewRequest,
    PropEditPreviewResponse, PropEditTemplateRequest, PropEditTemplateResponse, RenameApplyRequest,
    RenamePreviewRequest, RenamePreviewResponse,
};
use crate::runtime::{display_path, parse_provider, provider_name};
use crate::{MkvoRuntime, RuntimeError, RuntimeResult};

mod execution_facade;
mod execution_support;
mod planning_facade;
mod property_edit_workflow;
mod remux_workflow;
mod rename_presentation;
mod rename_workflow;
mod response_mappers;
mod shared_workflow;
use execution_support::{
    append_propedit_arguments, append_track_selection, backup_path, extraction_temp_path,
    require_plan_fields, runtime_application_error, validate_idempotent_job,
};
use rename_presentation::{
    file_name, path_key, remux_mode_label, remux_tool_name, same_path, track_kind_label,
};
use rename_presentation::{match_episode_for_file, scope_rows, selected_seasons};
use response_mappers::{
    mux_preview_response, propedit_preview_response, rename_apply_response, rename_preview_response,
};

fn rename_batch_dto(record: &RenameBatchRecord) -> RenameBatchRecordDto {
    RenameBatchRecordDto {
        id: record.id.to_string(),
        created_at: record.created_at,
        undone_at: record.undone_at,
        provider: record
            .provider
            .map_or_else(String::new, |value| provider_name(value).to_owned()),
        template: record.template.clone(),
        total_files: record.entries.len(),
        entries: record
            .entries
            .iter()
            .map(|entry| RenameBatchEntryDto {
                original_path: display_path(&entry.original_path),
                renamed_path: display_path(&entry.renamed_path),
                original_file_name: file_name(&entry.original_path),
                renamed_file_name: file_name(&entry.renamed_path),
            })
            .collect(),
        is_undone: record.undone_at.is_some(),
        display_name: format!(
            "{} - {} file(s)",
            record.created_at.format("%Y-%m-%d %H:%M"),
            record.entries.len()
        ),
    }
}

fn legacy_rename_batch_dto(
    record: &mkvo_infra_sqlite::LegacyRenameBatchRecord,
) -> Option<RenameBatchRecordDto> {
    let created_at = chrono::DateTime::parse_from_rfc3339(&record.created_at)
        .ok()?
        .with_timezone(&Utc);
    let undone_at = record
        .undone_at
        .as_deref()
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc));
    let entries = record
        .entries
        .iter()
        .map(|entry| RenameBatchEntryDto {
            original_path: display_path(&entry.original_path),
            renamed_path: display_path(&entry.renamed_path),
            original_file_name: file_name(&entry.original_path),
            renamed_file_name: file_name(&entry.renamed_path),
        })
        .collect();
    Some(RenameBatchRecordDto {
        id: record.id.clone(),
        created_at,
        undone_at,
        provider: record.provider.clone(),
        template: record.template.clone(),
        total_files: record.total_files.max(record.entries.len()),
        entries,
        is_undone: record.undone_at.is_some(),
        display_name: format!(
            "Legacy (read-only) • {} • {} file(s)",
            created_at.format("%Y-%m-%d %H:%M"),
            record.total_files.max(record.entries.len())
        ),
    })
}

fn media_from_row(row: &mkvo_contracts::MediaFileRow, fingerprint: FileFingerprint) -> MediaFile {
    let tracks = tracks_from_row(row);
    let attachments = row
        .attachments
        .iter()
        .map(|attachment| MediaAttachment {
            id: attachment.id,
            file_name: attachment.file_name.clone(),
            content_type: (!attachment.content_type.is_empty())
                .then(|| attachment.content_type.clone()),
            description: (!attachment.description.is_empty())
                .then(|| attachment.description.clone()),
            size_bytes: attachment.size_bytes,
        })
        .collect();
    MediaFile {
        path: PathBuf::from(&row.path),
        original_file_name: Some(row.file_name.clone()),
        watch_root: None,
        relative_path: None,
        fingerprint,
        container: ContainerMetadata {
            kind: match row
                .extension
                .trim_start_matches('.')
                .to_ascii_lowercase()
                .as_str()
            {
                "mkv" | "mka" => ContainerKind::Matroska,
                "webm" => ContainerKind::WebM,
                "mp4" | "m4v" => ContainerKind::Mp4,
                other => ContainerKind::Other(other.to_owned()),
            },
            title: None,
            duration_millis: None,
            muxing_application: None,
            writing_application: None,
        },
        tracks,
        attachments,
        episode: None,
        provider_match: None,
        status: MediaStatus::Ready,
    }
}

fn tracks_from_row(row: &mkvo_contracts::MediaFileRow) -> Vec<MediaTrack> {
    row.tracks
        .iter()
        .map(|track| MediaTrack {
            mkvmerge_id: track.id,
            propedit_track_number: track.track_number,
            kind: parse_track_kind(&track.track_type),
            codec: track.codec.clone(),
            codec_id: None,
            language: (!track.language.is_empty()).then(|| track.language.clone()),
            name: (!track.name.is_empty()).then(|| track.name.clone()),
            resolution: None,
            bit_depth: None,
            hdr: None,
            channels: track.channels,
            sampling_frequency_hz: None,
            default: track.default,
            forced: track.forced,
            enabled: true,
        })
        .collect()
}

fn parse_track_kind(value: &str) -> TrackKind {
    match value.trim().to_ascii_lowercase().as_str() {
        "video" => TrackKind::Video,
        "audio" => TrackKind::Audio,
        "subtitle" | "subtitles" => TrackKind::Subtitle,
        "buttons" => TrackKind::Buttons,
        _ => TrackKind::Other,
    }
}

async fn discover_external_subtitles(
    files: &[MediaFile],
    formats: &str,
    default_language: &str,
    track_name_template: Option<&str>,
) -> BTreeMap<PathBuf, Vec<ExternalSubtitle>> {
    let formats = split_strings(formats);
    let mut result = BTreeMap::new();
    for file in files {
        let Some(parent) = file.path.parent() else {
            continue;
        };
        let stem = file
            .path
            .file_stem()
            .map_or_else(String::new, |value| value.to_string_lossy().into_owned());
        let Ok(mut entries) = tokio::fs::read_dir(parent).await else {
            continue;
        };
        let mut subtitles = Vec::new();
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            let extension = path.extension().map_or_else(String::new, |value| {
                value.to_string_lossy().to_ascii_lowercase()
            });
            let candidate_stem = path
                .file_stem()
                .map_or_else(String::new, |value| value.to_string_lossy().into_owned());
            let candidate_lower = candidate_stem.to_ascii_lowercase();
            let stem_lower = stem.to_ascii_lowercase();
            let matching_suffix = candidate_lower.strip_prefix(&stem_lower);
            if formats.contains(&extension)
                && matching_suffix.is_some_and(|suffix| {
                    suffix.is_empty()
                        || suffix.starts_with('.')
                        || suffix.starts_with(' ')
                        || suffix.starts_with('-')
                        || suffix.starts_with('_')
                })
            {
                let suffix = candidate_stem
                    .get(stem.len()..)
                    .unwrap_or_default()
                    .trim_start_matches(['.', ' ', '-', '_']);
                let mut parts = suffix.split('.').filter(|value| !value.trim().is_empty());
                let first = parts.next();
                let (language, tag) = first.map_or_else(
                    || (default_language.trim().to_owned(), String::new()),
                    |first| {
                        if looks_like_language(first) {
                            (
                                first.to_ascii_lowercase(),
                                parts.collect::<Vec<_>>().join("."),
                            )
                        } else {
                            (default_language.trim().to_owned(), suffix.to_owned())
                        }
                    },
                );
                let rendered_name = track_name_template
                    .unwrap_or("{tag}")
                    .replace("{tag}", tag.trim())
                    .replace("{language}", &language)
                    .replace("{file}", &candidate_stem);
                subtitles.push(ExternalSubtitle {
                    path,
                    language,
                    name: (!rendered_name.trim().is_empty())
                        .then(|| rendered_name.trim().to_owned()),
                    default: false,
                    forced: candidate_stem.to_ascii_lowercase().contains("forced"),
                });
            }
        }
        if !subtitles.is_empty() {
            result.insert(file.path.clone(), subtitles);
        }
    }
    result
}

fn looks_like_language(value: &str) -> bool {
    (2..=3).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_alphabetic())
}

fn build_extractions(
    files: &[MediaFile],
    languages: &str,
    _overwrite: bool,
) -> BTreeMap<PathBuf, Vec<TrackExtraction>> {
    let languages = split_strings(languages);
    files
        .iter()
        .map(|file| {
            let tracks = file
                .tracks
                .iter()
                .filter(|track| track.kind == TrackKind::Subtitle)
                .filter(|track| {
                    languages.is_empty()
                        || languages
                            .contains(&track.language_or_undetermined().to_ascii_lowercase())
                })
                .map(|track| {
                    let language = track.language_or_undetermined();
                    let output = file.path.with_extension(format!(
                        "{language}.{}.{}",
                        track.mkvmerge_id,
                        subtitle_extension(&track.codec)
                    ));
                    TrackExtraction {
                        track_id: track.mkvmerge_id,
                        kind: TrackKind::Subtitle,
                        output,
                    }
                })
                .collect();
            (file.path.clone(), tracks)
        })
        .collect()
}

fn subtitle_extension(codec: &str) -> &'static str {
    let codec = codec.to_ascii_lowercase();
    if codec.contains("ass") || codec.contains("ssa") {
        "ass"
    } else if codec.contains("pgs") || codec.contains("hdmv") {
        "sup"
    } else {
        "srt"
    }
}

fn split_strings(value: &str) -> BTreeSet<String> {
    value
        .split([',', ';', ' '])
        .map(|value| value.trim().trim_start_matches('.').to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .collect()
}

fn split_u64s(value: &str) -> BTreeSet<u64> {
    value
        .split([',', ';', ' '])
        .filter_map(|value| value.trim().parse().ok())
        .collect()
}

fn title_edit(mode: TitleEditMode, custom: &str) -> TextEdit {
    match mode {
        TitleEditMode::Keep => TextEdit::Keep,
        TitleEditMode::File => TextEdit::FromFileName,
        TitleEditMode::EpisodeTitle => TextEdit::FromEpisodeTitle,
        TitleEditMode::Custom => TextEdit::Set(custom.trim().to_owned()),
        TitleEditMode::Remove => TextEdit::Delete,
    }
}

fn build_track_edits(request: &PropEditPreviewRequest) -> Vec<TrackEditIntent> {
    let mut edits = Vec::new();
    let video_language = request
        .video_track_language
        .as_deref()
        .map(|language| language.trim().to_owned());
    let video_default = track_flag_edit(&request.selected_default_video, "Default");
    if video_language.is_some() || video_default.is_some() {
        edits.push(TrackEditIntent {
            kind: TrackKind::Video,
            ordinal: 1,
            name: TextEdit::Keep,
            language: video_language,
            default: video_default,
            forced: None,
        });
    }
    append_track_edits(
        &mut edits,
        TrackKind::Audio,
        &request.audio_tracks,
        &request.selected_default_audio,
        &request.selected_forced_audio,
    );
    append_track_edits(
        &mut edits,
        TrackKind::Subtitle,
        &request.subtitle_tracks,
        &request.selected_default_subtitle,
        &request.selected_forced_subtitle,
    );
    edits
}

fn append_track_edits(
    edits: &mut Vec<TrackEditIntent>,
    kind: TrackKind,
    rows: &[PropEditTrackConfigRow],
    selected_default: &str,
    selected_forced: &str,
) {
    for (index, row) in rows.iter().enumerate() {
        // Rows are seeded from the template. Their values are desired values
        // for every target, even when the user did not alter the template row.
        // Encoding an unchanged template value as Keep made every mismatching
        // target keep its own value instead of converging on the template.
        let name = if kind == TrackKind::Audio && row.name_from_channels == Some(true) {
            TextEdit::FromTrackChannels
        } else if row.name_from_metadata {
            TextEdit::FromTrackMetadata
        } else if row.edited_name.trim().is_empty() {
            TextEdit::Delete
        } else {
            TextEdit::Set(row.edited_name.trim().to_owned())
        };
        let language = Some(if row.edited_language.trim().is_empty() {
            "und".to_owned()
        } else {
            row.edited_language.trim().to_owned()
        });
        let default = track_flag_edit(selected_default, &row.track_label);
        let forced = track_flag_edit(selected_forced, &row.track_label);
        edits.push(TrackEditIntent {
            kind,
            ordinal: u32::try_from(index + 1).unwrap_or(u32::MAX),
            name,
            language,
            default,
            forced,
        });
    }
}

fn track_flag_edit(selection: &str, track_label: &str) -> Option<bool> {
    if selection.is_empty() || selection.eq_ignore_ascii_case("Keep existing") {
        None
    } else if selection.eq_ignore_ascii_case("None") {
        Some(false)
    } else {
        Some(selection == track_label)
    }
}

fn prop_track_row(index: usize, track: &MediaTrack) -> PropEditTrackConfigRow {
    let has_name = track
        .name
        .as_deref()
        .is_some_and(|name| !name.trim().is_empty());
    PropEditTrackConfigRow {
        track_number: u32::try_from(index + 1).unwrap_or(u32::MAX),
        track_label: format!("{} {}", track_kind_label(track.kind), index + 1),
        track_type: track_kind_label(track.kind).to_ascii_lowercase(),
        current_name: track.name.clone().unwrap_or_default(),
        current_language: track.language_or_undetermined().to_owned(),
        current_codec: track.codec.clone(),
        current_channels: track.channels,
        current_default: track.default,
        edited_name: track.name.clone().unwrap_or_default(),
        name_from_metadata: track.kind == TrackKind::Audio && !has_name,
        name_from_channels: None,
        edited_language: track.language_or_undetermined().to_owned(),
    }
}

fn selected_track_label(
    file: &MediaFile,
    kind: TrackKind,
    predicate: impl Fn(&MediaTrack) -> bool,
) -> String {
    file.tracks
        .iter()
        .filter(|track| track.kind == kind)
        .enumerate()
        .find(|(_, track)| predicate(track))
        .map_or_else(String::new, |(index, _)| {
            format!("{} {}", track_kind_label(kind), index + 1)
        })
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet, HashMap};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use async_trait::async_trait;
    use chrono::Utc;
    use mkvo_application::{
        AuthorizedPathPolicy, FileSystem, MediaCatalog, MediaEnumerationRequest, MediaProbe,
        PortError, ToolExecutionResult, ToolExecutor, ToolRegistry,
    };
    use mkvo_contracts::{ApiErrorCode, ToolStatus};
    use mkvo_domain::{AppSettings, FileFingerprint, MediaFile, MediaStatus, RemuxMode};
    use mkvo_infra_sqlite::{SqliteRepositories, SqliteStore};
    use tokio::sync::RwLock;
    use tokio_util::sync::CancellationToken;

    use super::*;
    use crate::{MemorySecretStore, RuntimeConfig, RuntimeDependencies};

    #[derive(Default)]
    struct EmptyCatalog;

    #[async_trait]
    impl MediaCatalog for EmptyCatalog {
        async fn enumerate(
            &self,
            _request: &MediaEnumerationRequest,
            _cancel: CancellationToken,
        ) -> Result<Vec<FileFingerprint>, PortError> {
            Ok(Vec::new())
        }
    }

    struct FailingProbe;

    #[async_trait]
    impl MediaProbe for FailingProbe {
        async fn inspect(
            &self,
            _path: &Path,
            _cancel: CancellationToken,
        ) -> Result<MediaFile, PortError> {
            Err(PortError::Other("probe not used".to_owned()))
        }
    }

    #[derive(Default)]
    struct AllowPaths;

    #[async_trait]
    impl AuthorizedPathPolicy for AllowPaths {
        async fn authorize_read(&self, path: &Path) -> Result<PathBuf, PortError> {
            Ok(path.to_path_buf())
        }

        async fn authorize_write(&self, path: &Path) -> Result<PathBuf, PortError> {
            Ok(path.to_path_buf())
        }
    }

    #[derive(Default)]
    struct FakeFileSystem {
        files: RwLock<HashMap<PathBuf, FileFingerprint>>,
        moves: AtomicUsize,
    }

    impl FakeFileSystem {
        async fn add(&self, fingerprint: FileFingerprint) {
            self.files
                .write()
                .await
                .insert(fingerprint.path.clone(), fingerprint);
        }
    }

    #[async_trait]
    impl FileSystem for FakeFileSystem {
        async fn exists(&self, path: &Path) -> Result<bool, PortError> {
            Ok(self.files.read().await.contains_key(path))
        }

        async fn is_directory(&self, _path: &Path) -> Result<bool, PortError> {
            Ok(true)
        }

        async fn fingerprint(&self, path: &Path) -> Result<FileFingerprint, PortError> {
            self.files
                .read()
                .await
                .get(path)
                .cloned()
                .ok_or_else(|| PortError::NotFound(path.display().to_string()))
        }

        async fn probe_access(
            &self,
            _path: &Path,
            _access: RequiredAccess,
        ) -> Result<FileAccessState, PortError> {
            Ok(FileAccessState::Available)
        }

        async fn move_file(&self, source: &Path, target: &Path) -> Result<(), PortError> {
            self.moves.fetch_add(1, Ordering::SeqCst);
            let mut files = self.files.write().await;
            let mut fingerprint = files
                .remove(source)
                .ok_or_else(|| PortError::NotFound(source.display().to_string()))?;
            if files.contains_key(target) {
                return Err(PortError::Conflict(target.display().to_string()));
            }
            fingerprint.path = target.to_path_buf();
            files.insert(target.to_path_buf(), fingerprint);
            Ok(())
        }

        async fn remove_file(&self, path: &Path) -> Result<(), PortError> {
            self.files.write().await.remove(path);
            Ok(())
        }
    }

    struct FakeTools;

    #[async_trait]
    impl ToolRegistry for FakeTools {
        async fn status(&self, logical_name: &str) -> Result<ToolStatus, PortError> {
            Ok(ToolStatus {
                name: logical_name.to_owned(),
                command: logical_name.to_owned(),
                resolved_path: format!("/tools/{logical_name}"),
                available: true,
                version: "test".to_owned(),
                error: None,
            })
        }

        async fn all_statuses(&self) -> Result<Vec<ToolStatus>, PortError> {
            Ok(Vec::new())
        }
    }

    #[derive(Default)]
    struct FailingExecutor {
        calls: AtomicUsize,
    }

    #[async_trait]
    impl ToolExecutor for FailingExecutor {
        async fn execute(
            &self,
            _invocation: &ToolInvocation,
            _cancel: CancellationToken,
        ) -> Result<ToolExecutionResult, PortError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Err(PortError::Other("intentional test stop".to_owned()))
        }
    }

    struct Fixture {
        runtime: MkvoRuntime,
        file_system: Arc<FakeFileSystem>,
        executor: Arc<FailingExecutor>,
    }

    fn fixture() -> Fixture {
        let repositories = Arc::new(SqliteRepositories::from_store(
            SqliteStore::open_in_memory().expect("SQLite"),
        ));
        let file_system = Arc::new(FakeFileSystem::default());
        let executor = Arc::new(FailingExecutor::default());
        let dependencies = RuntimeDependencies {
            catalog: Arc::new(EmptyCatalog),
            probe: Arc::new(FailingProbe),
            cache: repositories.clone(),
            paths: Arc::new(AllowPaths),
            file_system: file_system.clone(),
            tools: Arc::new(FakeTools),
            tool_executor: executor.clone(),
            settings: repositories.clone(),
            secrets: Arc::new(MemorySecretStore::new()),
            plans: repositories.clone(),
            jobs: repositories.clone(),
            rename_history: repositories.clone(),
            journal: repositories.clone(),
            logs: repositories,
            watcher: None,
            process_tools: None,
            authorized_roots: None,
        };
        let config = RuntimeConfig::new(PathBuf::from("/media"), PathBuf::from("/config"));
        Fixture {
            runtime: MkvoRuntime::from_parts(config, dependencies),
            file_system,
            executor,
        }
    }

    fn fingerprint(path: &str) -> FileFingerprint {
        FileFingerprint {
            path: PathBuf::from(path),
            size_bytes: 10,
            modified_at: Utc::now(),
            quick_hash: Some("hash".to_owned()),
        }
    }

    fn media(path: &str) -> MediaFile {
        MediaFile {
            path: PathBuf::from(path),
            original_file_name: None,
            watch_root: None,
            relative_path: None,
            fingerprint: fingerprint(path),
            container: ContainerMetadata::default(),
            tracks: vec![MediaTrack {
                mkvmerge_id: 0,
                propedit_track_number: 1,
                kind: TrackKind::Video,
                codec: "AVC".to_owned(),
                codec_id: None,
                language: None,
                name: None,
                resolution: None,
                bit_depth: None,
                hdr: None,
                channels: None,
                sampling_frequency_hz: None,
                default: true,
                forced: false,
                enabled: true,
            }],
            attachments: Vec::new(),
            episode: None,
            provider_match: None,
            status: MediaStatus::Ready,
        }
    }

    fn mux_request(plan: Option<&RemuxPlan>) -> MuxPreviewRequest {
        MuxPreviewRequest {
            files: Vec::new(),
            selected_paths: Vec::new(),
            remove_unwanted_audio_languages: false,
            keep_audio_languages: String::new(),
            remove_unwanted_subtitle_languages: false,
            keep_subtitle_languages: String::new(),
            remove_unwanted_track_ids: false,
            remove_track_ids_text: String::new(),
            preserve_chapters: true,
            preserve_attachments: true,
            mux_matching_external_subtitles: false,
            external_subtitle_language: "eng".to_owned(),
            external_subtitle_track_name: Some("{tag}".to_owned()),
            external_subtitle_formats: "srt".to_owned(),
            preserve_external_subtitle_files: true,
            skip_mux_if_subtitle_already_exists: false,
            extract_subtitles: false,
            extract_subtitle_languages: "eng".to_owned(),
            extract_overwrite_existing_files: false,
            convert_mp4_to_mkv: false,
            delete_mp4_after_convert: false,
            plan_id: plan.map(|plan| plan.metadata.id),
            plan_fingerprint: plan.map(|plan| plan.metadata.fingerprint.clone()),
            idempotency_key: plan.map(|plan| plan.metadata.idempotency_key.clone()),
        }
    }

    fn remux_plan(path: &str, key: IdempotencyKey) -> RemuxPlan {
        let settings = AppSettings::default();
        RemuxPlanner
            .build_plan(RemuxPlanRequest {
                source_access: BTreeMap::new(),
                mode: RemuxMode::ConvertToMkv,
                files: vec![media(path)],
                options: RemuxOptions::default(),
                external_subtitles: BTreeMap::new(),
                extractions: BTreeMap::new(),
                existing_paths: BTreeSet::new(),
                authorized_roots: Vec::new(),
                settings_fingerprint: stable_fingerprint(&settings).expect("settings fingerprint"),
                tool_fingerprints: BTreeMap::new(),
                expires_in_seconds: 60,
                idempotency_key: key,
            })
            .expect("remux plan")
    }

    #[test]
    fn template_track_values_are_desired_batch_values() {
        let mut request = PropEditPreviewRequest {
            files: Vec::new(),
            selected_paths: Vec::new(),
            template_path: None,
            container_title_mode: TitleEditMode::Keep,
            custom_container_title: String::new(),
            video_title_mode: TitleEditMode::Keep,
            custom_video_title: String::new(),
            video_track_language: None,
            selected_default_video: "Keep existing".to_owned(),
            audio_tracks: vec![PropEditTrackConfigRow {
                track_number: 1,
                track_label: "Audio 1".to_owned(),
                track_type: "audio".to_owned(),
                current_name: "English".to_owned(),
                current_language: "eng".to_owned(),
                current_codec: "AAC".to_owned(),
                current_channels: Some(2),
                current_default: true,
                edited_name: "English".to_owned(),
                name_from_metadata: false,
                name_from_channels: None,
                edited_language: "eng".to_owned(),
            }],
            subtitle_tracks: Vec::new(),
            selected_default_audio: "Audio 1".to_owned(),
            selected_forced_audio: "Keep existing".to_owned(),
            selected_default_subtitle: "Keep existing".to_owned(),
            selected_forced_subtitle: "Keep existing".to_owned(),
            plan_id: None,
            plan_fingerprint: None,
            idempotency_key: None,
        };

        let edits = build_track_edits(&request);
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].name, TextEdit::Set("English".to_owned()));
        assert_eq!(edits[0].language.as_deref(), Some("eng"));
        assert_eq!(edits[0].default, Some(true));
        assert_eq!(edits[0].forced, None);
        request.audio_tracks[0].name_from_channels = Some(true);
        assert_eq!(build_track_edits(&request)[0].name, TextEdit::FromTrackChannels);
    }

    #[test]
    fn unnamed_audio_tracks_default_to_metadata_names() {
        let mut audio = media("episode.mkv").tracks.remove(0);
        audio.kind = TrackKind::Audio;
        audio.codec = "AAC".to_owned();
        audio.language = Some("eng".to_owned());
        audio.channels = Some(6);

        let unnamed = prop_track_row(0, &audio);
        assert!(unnamed.name_from_metadata);

        audio.name = Some("Main Audio".to_owned());
        let named = prop_track_row(0, &audio);
        assert!(!named.name_from_metadata);
        assert_eq!(named.edited_name, "Main Audio");
    }

    #[test]
    fn video_language_is_opt_in_and_targets_the_first_video_track() {
        let mut request = PropEditPreviewRequest {
            files: Vec::new(),
            selected_paths: Vec::new(),
            template_path: None,
            container_title_mode: TitleEditMode::Keep,
            custom_container_title: String::new(),
            video_title_mode: TitleEditMode::Keep,
            custom_video_title: String::new(),
            video_track_language: None,
            selected_default_video: "Keep existing".to_owned(),
            audio_tracks: Vec::new(),
            subtitle_tracks: Vec::new(),
            selected_default_audio: "Keep existing".to_owned(),
            selected_forced_audio: "Keep existing".to_owned(),
            selected_default_subtitle: "Keep existing".to_owned(),
            selected_forced_subtitle: "Keep existing".to_owned(),
            plan_id: None,
            plan_fingerprint: None,
            idempotency_key: None,
        };

        assert!(build_track_edits(&request).is_empty());

        request.video_track_language = Some("und".to_owned());
        let edits = build_track_edits(&request);
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].kind, TrackKind::Video);
        assert_eq!(edits[0].ordinal, 1);
        assert_eq!(edits[0].language.as_deref(), Some("und"));

        request.video_track_language = None;
        request.selected_default_video = "Default".to_owned();
        let edits = build_track_edits(&request);
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].kind, TrackKind::Video);
        assert_eq!(edits[0].default, Some(true));

        request.selected_default_video = "None".to_owned();
        let edits = build_track_edits(&request);
        assert_eq!(edits[0].default, Some(false));
    }

    #[tokio::test]
    async fn mutation_is_rejected_without_an_immutable_plan() {
        let fixture = fixture();
        let rename_error = fixture
            .runtime
            .apply_rename_preview(RenameApplyRequest::default())
            .await
            .expect_err("rename without plan");
        assert_eq!(rename_error.code, ApiErrorCode::InvalidRequest);

        let mux_error = fixture
            .runtime
            .start_mux_apply(mux_request(None))
            .await
            .expect_err("mux without plan");
        assert_eq!(mux_error.code, ApiErrorCode::InvalidRequest);
        assert_eq!(fixture.file_system.moves.load(Ordering::SeqCst), 0);
        assert_eq!(fixture.executor.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn tampered_plan_fingerprint_never_reaches_file_system() {
        let fixture = fixture();
        let source = media("/media/Episode.mkv");
        fixture.file_system.add(source.fingerprint.clone()).await;
        let settings_fingerprint = stable_fingerprint(&AppSettings::default()).expect("settings");
        let plan = RenamePlanner
            .build_plan(RenamePlanRequest {
                source_access: BTreeMap::new(),
                existing_parents: BTreeSet::new(),
                files: vec![source],
                template: "Renamed".to_owned(),
                provider: None,
                check_existing_files: false,
                existing_paths: BTreeSet::new(),
                authorized_roots: Vec::new(),
                settings_fingerprint,
                expires_in_seconds: 60,
                idempotency_key: IdempotencyKey::generate(),
            })
            .expect("rename plan");
        fixture.runtime.persist_plan(&plan).await.expect("persist");
        let error = fixture
            .runtime
            .apply_rename_preview(RenameApplyRequest {
                plan_id: Some(plan.metadata.id),
                plan_fingerprint: Some("tampered".to_owned()),
                idempotency_key: Some(plan.metadata.idempotency_key.clone()),
                ..RenameApplyRequest::default()
            })
            .await
            .expect_err("tampered plan");
        assert_eq!(error.code, ApiErrorCode::PlanTampered);
        assert_eq!(fixture.file_system.moves.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn repeated_job_submission_is_idempotent_and_different_plan_conflicts() {
        let fixture = fixture();
        let key = IdempotencyKey::parse("same-operation").expect("key");
        let plan = remux_plan("/media/movie.mp4", key.clone());
        fixture
            .file_system
            .add(plan.context.input_fingerprints[0].clone())
            .await;
        fixture.runtime.persist_plan(&plan).await.expect("persist");

        let first = fixture
            .runtime
            .start_mux_apply(mux_request(Some(&plan)))
            .await
            .expect("first submission");
        let second = fixture
            .runtime
            .start_mux_apply(mux_request(Some(&plan)))
            .await
            .expect("idempotent replay");
        assert_eq!(first.id, second.id);

        let other = remux_plan("/media/other.mp4", key);
        fixture
            .file_system
            .add(other.context.input_fingerprints[0].clone())
            .await;
        fixture
            .runtime
            .persist_plan(&other)
            .await
            .expect("persist other");
        let conflict = fixture
            .runtime
            .start_mux_apply(mux_request(Some(&other)))
            .await
            .expect_err("same key for another plan");
        assert_eq!(conflict.code, ApiErrorCode::Conflict);

        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        assert_eq!(fixture.executor.calls.load(Ordering::SeqCst), 1);
    }
}
