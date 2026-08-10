use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use chrono::{DateTime, Utc};
use mkvo_application::{
    ApplicationError, JobSpec, JobSupervisor, JournalStatus, MediaServerClient,
    MediaServerConnection, MetadataProviderClient, ScanOutcome, ScanService, SettingsService,
};
use mkvo_contracts::{
    AppStatus, CurrentScanResponse, FileSystemEntry, FileSystemEntryKind, FileSystemResponse,
    JobCompletion, JobEventEnvelope, JobKind, JobSnapshot, JobStatus, LogQuery, MediaFileRow,
    MediaServerSyncResponse, MediaServerTestResponse, RenameProviderTestResponse, RenameScopeRow,
    ScanJobResponse, ScanRequest, ScanSummary, SecretUpdate, SourceRoot, WebMediaServer,
    WebMediaServerLibraryPath, WebMediaServerPathMapping, WebSettings, WebSettingsRequest,
};
use mkvo_domain::{
    AppSettings, CredentialState, IdempotencyKey, JobId, MediaFile, MediaServerId, MediaServerKind,
    MediaServerLibrary, MediaServerSettings, MetadataProvider, PathMapping, stable_fingerprint,
};
use mkvo_infra_media_servers::{
    ConfiguredMediaServerClient, MediaServerDiscoveryClient, MediaServerPathMapping,
};
use mkvo_infra_netshare::{UncTarget, classify_unc, list_server_shares};
use mkvo_infra_providers::{
    AniDbClient, AniListClient, ConfiguredAniDbProvider, ConfiguredAniListProvider,
    ConfiguredTmdbProvider, ConfiguredTvdbProvider, ProviderCredentials, SecretString, TmdbClient,
    TvdbClient,
};
use mkvo_infra_sqlite::{
    LegacyImportOutcome, LegacyRenameBatchRecord, SqliteStore, import_legacy_settings,
    read_legacy_rename_history,
};
use serde::{Deserialize, Serialize};
use tokio::sync::{RwLock, broadcast, oneshot};
use tokio_util::sync::CancellationToken;

use crate::compat::{
    MediaServerConnectionRequest, OperationJobResponse, OperationLogResponse,
    RenameProviderTestRequest, RenameScopesRequest, RenameSearchRequest, RenameSearchResult,
};
use crate::{BrowseScope, RuntimeConfig, RuntimeDependencies, RuntimeError, RuntimeResult};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScanResultState {
    files: Vec<MediaFile>,
    rows: Vec<MediaFileRow>,
    skipped: Vec<String>,
    summary: ScanSummary,
}

#[derive(Clone, Debug, Default)]
struct CurrentScanState {
    updated_utc: Option<DateTime<Utc>>,
    files: Vec<MediaFile>,
    summary: ScanSummary,
    /// Selected paths, stored by normalized key so a selection made against one
    /// spelling of a path still matches the file it names.
    selected: BTreeSet<String>,
}

impl CurrentScanState {
    /// Drop selections whose file is no longer in the working set.
    ///
    /// A job that remuxes, converts, or renames changes which paths exist, and a
    /// selection left pointing at a path that is gone would otherwise be handed
    /// to the next operation.
    fn reconcile_selection(&mut self) {
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

    /// Selected paths rendered the way the UI shows them, ordered by the working
    /// set so the list is stable between reads.
    fn selected_display_paths(&self) -> Vec<String> {
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

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CompatibilitySettings {
    audio_name_presets: Vec<String>,
    subtitle_name_presets: Vec<String>,
    language_presets: Vec<String>,
    mkv_merge_default_audio_languages: String,
    mkv_merge_default_subtitle_languages: String,
}

impl Default for CompatibilitySettings {
    fn default() -> Self {
        Self {
            audio_name_presets: [
                "English",
                "Japanese",
                "Commentary",
                "Director Commentary",
                "Signs & Songs",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
            subtitle_name_presets: [
                "English",
                "English Forced",
                "English SDH",
                "Signs & Songs",
                "Commentary",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
            language_presets: [
                "eng", "jpn", "spa", "fre", "ger", "und", "en", "ja", "es", "fr", "de",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
            mkv_merge_default_audio_languages: "eng,jpn".to_owned(),
            mkv_merge_default_subtitle_languages: "eng".to_owned(),
        }
    }
}

struct RuntimeInner {
    config: RuntimeConfig,
    dependencies: RuntimeDependencies,
    jobs: Arc<JobSupervisor>,
    scan: Arc<ScanService>,
    current_scan: Arc<RwLock<CurrentScanState>>,
    scan_results: Arc<RwLock<HashMap<JobId, ScanResultState>>>,
    compatibility_settings: RwLock<CompatibilitySettings>,
    legacy_rename_history: Vec<LegacyRenameBatchRecord>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacyMigrationStatus {
    SourceMissing,
    SkippedExisting,
    Imported,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyMigrationReport {
    pub status: LegacyMigrationStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings_revision: Option<u64>,
    pub secrets_imported: usize,
    pub cache_rebuild_required: bool,
    pub legacy_rename_batches: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryDisposition {
    Completed,
    CleanRetry,
    ManualReview,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupRecoveryItem {
    pub job_id: JobId,
    pub job_kind: JobKind,
    pub previous_status: JobStatus,
    pub disposition: RecoveryDisposition,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub journal_status: Option<JournalStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub journal_step: Option<u64>,
    pub persisted_status_updated: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupRecoveryReport {
    pub inspected_utc: DateTime<Utc>,
    pub completed: usize,
    pub clean_retry: usize,
    pub manual_review: usize,
    #[serde(default)]
    pub items: Vec<StartupRecoveryItem>,
    /// The current journal port supports lookup by idempotency key, not global
    /// enumeration; orphan journals without a job row cannot be discovered.
    pub journal_enumeration_supported: bool,
    #[serde(default)]
    pub limitations: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogExport {
    pub file_name: String,
    pub entry_count: usize,
    pub content: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentJobsResponse {
    pub jobs: Vec<JobSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorizedRootGrant {
    pub path: String,
    pub writable: bool,
}

/// Shared transport-neutral facade used by Tauri commands and Axum handlers.
#[derive(Clone)]
pub struct MkvoRuntime {
    inner: Arc<RuntimeInner>,
}

impl MkvoRuntime {
    pub(crate) fn from_parts(config: RuntimeConfig, dependencies: RuntimeDependencies) -> Self {
        let jobs = Arc::new(JobSupervisor::new(Arc::clone(&dependencies.jobs)));
        let scan = Arc::new(ScanService::new(
            Arc::clone(&dependencies.catalog),
            Arc::clone(&dependencies.probe),
            Arc::clone(&dependencies.cache),
            Arc::clone(&dependencies.paths),
            4,
        ));
        let compatibility_settings = load_compatibility_settings(&config.config_root);
        let legacy_rename_history =
            config
                .resolved_legacy_rename_history_path()
                .map_or_else(Vec::new, |path| match read_legacy_rename_history(&path) {
                    Ok(records) => records,
                    Err(error) => {
                        tracing::warn!(
                            path = %path.display(),
                            error = %error,
                            "legacy rename history could not be loaded"
                        );
                        Vec::new()
                    }
                });
        Self {
            inner: Arc::new(RuntimeInner {
                config,
                dependencies,
                jobs,
                scan,
                current_scan: Arc::new(RwLock::new(CurrentScanState::default())),
                scan_results: Arc::new(RwLock::new(HashMap::new())),
                compatibility_settings: RwLock::new(compatibility_settings),
                legacy_rename_history,
            }),
        }
    }

    /// Imports legacy .NET settings through any injected async secret store.
    ///
    /// The default file-backed composition performs this during `build`. This
    /// method is for hosts that inject an OS keychain. Secrets are committed
    /// before the optimistic settings write, so a failed secret handoff never
    /// leaves a settings row that suppresses the next migration attempt.
    pub async fn migrate_legacy_data(&self) -> RuntimeResult<LegacyMigrationReport> {
        let legacy_batches = self.inner.legacy_rename_history.len();
        let Some(source) = self.inner.config.resolved_legacy_settings_path() else {
            return Ok(LegacyMigrationReport {
                status: LegacyMigrationStatus::SourceMissing,
                source_path: None,
                backup_path: None,
                settings_revision: None,
                secrets_imported: 0,
                cache_rebuild_required: false,
                legacy_rename_batches: legacy_batches,
            });
        };
        let (_, current_revision) = self.inner.dependencies.settings.load().await?;
        if current_revision > 0 {
            return Ok(LegacyMigrationReport {
                status: LegacyMigrationStatus::SkippedExisting,
                source_path: Some(display_path(&source)),
                backup_path: None,
                settings_revision: Some(current_revision),
                secrets_imported: 0,
                cache_rebuild_required: false,
                legacy_rename_batches: legacy_batches,
            });
        }

        let import_source = source.clone();
        let prepared = tokio::task::spawn_blocking(move || {
            let store = SqliteStore::open_in_memory()?;
            let mut secrets = std::collections::BTreeMap::new();
            let outcome = import_legacy_settings(&store, &import_source, |key, value| {
                secrets.insert(key.to_owned(), value.to_owned());
                Ok::<_, std::convert::Infallible>(())
            })?;
            let settings = store
                .get_setting_with_revision::<AppSettings>("app")?
                .map(|(_, _, settings)| settings);
            Ok::<_, mkvo_infra_sqlite::LegacyImportError>((outcome, settings, secrets))
        })
        .await
        .map_err(|error| RuntimeError::internal(format!("legacy import task failed: {error}")))?
        .map_err(|error| RuntimeError::internal(error.to_string()))?;

        let (outcome, settings, secrets) = prepared;
        let (backup_path, cache_rebuild_required) = match &outcome {
            LegacyImportOutcome::Imported {
                backup_path,
                cache_rebuild_required,
                ..
            } => (Some(display_path(backup_path)), *cache_rebuild_required),
            LegacyImportOutcome::SourceMissing => {
                return Ok(LegacyMigrationReport {
                    status: LegacyMigrationStatus::SourceMissing,
                    source_path: Some(display_path(&source)),
                    backup_path: None,
                    settings_revision: None,
                    secrets_imported: 0,
                    cache_rebuild_required: false,
                    legacy_rename_batches: legacy_batches,
                });
            }
            LegacyImportOutcome::SkippedExisting => unreachable!(
                "the isolated conversion store cannot already contain application settings"
            ),
        };

        let mut secrets_imported = 0;
        for (key, value) in secrets {
            if self.inner.dependencies.secrets.get(&key).await?.is_none() {
                self.inner.dependencies.secrets.set(&key, &value).await?;
                secrets_imported += 1;
            }
        }
        let settings = settings.ok_or_else(|| {
            RuntimeError::internal("legacy conversion completed without converted settings")
        })?;
        let revision = self
            .inner
            .dependencies
            .settings
            .save(&settings, Some(0))
            .await?;
        Ok(LegacyMigrationReport {
            status: LegacyMigrationStatus::Imported,
            source_path: Some(display_path(&source)),
            backup_path,
            settings_revision: Some(revision),
            secrets_imported,
            cache_rebuild_required,
            legacy_rename_batches: legacy_batches,
        })
    }

    /// Classifies non-terminal jobs left behind by a previous process without
    /// changing persisted state.
    pub async fn classify_startup_recovery(&self) -> RuntimeResult<StartupRecoveryReport> {
        self.startup_recovery(false).await
    }

    /// Terminalizes stale persisted jobs according to their durable journal.
    /// Completed mutations become completed jobs; clean retries and ambiguous
    /// partial mutations become failed jobs with an explicit recovery reason.
    pub async fn recover_startup_state(&self) -> RuntimeResult<StartupRecoveryReport> {
        self.startup_recovery(true).await
    }

    async fn startup_recovery(&self, persist: bool) -> RuntimeResult<StartupRecoveryReport> {
        let inspected_utc = Utc::now();
        let mut report = StartupRecoveryReport {
            inspected_utc,
            completed: 0,
            clean_retry: 0,
            manual_review: 0,
            items: Vec::new(),
            journal_enumeration_supported: false,
            limitations: vec![
                "The operation journal can only be queried by idempotency key; orphan journal rows without a corresponding job cannot be enumerated."
                    .to_owned(),
            ],
        };
        for mut snapshot in self.inner.dependencies.jobs.list_recent(1_000).await? {
            if snapshot.status.is_terminal() {
                continue;
            }
            let previous_status = snapshot.status;
            let journal = self
                .inner
                .dependencies
                .journal
                .get(&snapshot.idempotency_key)
                .await?;
            let (disposition, reason) = classify_recovery(&snapshot, journal.as_ref());
            match disposition {
                RecoveryDisposition::Completed => report.completed += 1,
                RecoveryDisposition::CleanRetry => report.clean_retry += 1,
                RecoveryDisposition::ManualReview => report.manual_review += 1,
            }
            let mut persisted_status_updated = false;
            if persist {
                snapshot.status = match disposition {
                    RecoveryDisposition::Completed => JobStatus::Completed,
                    RecoveryDisposition::CleanRetry | RecoveryDisposition::ManualReview => {
                        JobStatus::Failed
                    }
                };
                snapshot.completed_utc = Some(inspected_utc);
                snapshot.current_file_percent = 0;
                snapshot.lines.push(format!("Startup recovery: {reason}"));
                if disposition == RecoveryDisposition::Completed {
                    snapshot.error = None;
                } else {
                    snapshot.error = Some(reason.clone());
                }
                snapshot.revision = snapshot.revision.saturating_add(1);
                self.inner.dependencies.jobs.update(&snapshot).await?;
                persisted_status_updated = true;
            }
            report.items.push(StartupRecoveryItem {
                job_id: snapshot.id,
                job_kind: snapshot.kind,
                previous_status,
                disposition,
                reason,
                journal_status: journal.as_ref().map(|record| record.status),
                journal_step: journal.as_ref().map(|record| record.step),
                persisted_status_updated,
            });
        }
        Ok(report)
    }

    pub async fn get_status(&self) -> RuntimeResult<AppStatus> {
        let tools = self.inner.dependencies.tools.all_statuses().await?;
        Ok(AppStatus {
            name: self.inner.config.app_name.clone(),
            version: self.inner.config.version.clone(),
            media_root: display_path(&self.inner.config.media_root),
            config_root: display_path(&self.inner.config.config_root),
            source_roots: self
                .inner
                .config
                .source_roots
                .iter()
                .map(|(name, path)| SourceRoot {
                    name: name.clone(),
                    path: display_path(path),
                })
                .collect(),
            tools,
            contract_version: mkvo_domain::CONTRACT_VERSION,
        })
    }

    pub async fn browse_file_system(
        &self,
        path: Option<String>,
    ) -> RuntimeResult<FileSystemResponse> {
        // An explicitly empty path means "the top" — the volume list — and is
        // distinct from no path at all, which means "wherever the host starts".
        // Collapsing the two would make the volume list unreachable, since the
        // media root would answer for both.
        let unrestricted = self.inner.config.browse_scope == BrowseScope::Unrestricted;
        if unrestricted && path.as_deref().is_some_and(|value| value.trim().is_empty()) {
            return Ok(volume_listing());
        }

        // A bare `\\server` is not a directory, so `read_dir` would only ever
        // report that the network name cannot be found. Its shares have to be
        // enumerated instead, which is how the user reaches a share that no
        // drive letter points at.
        if unrestricted
            && let Some(requested) = path.as_deref()
            && let Some(UncTarget::Server(server)) = classify_unc(requested)
        {
            return server_listing(&server);
        }

        let requested = path
            .filter(|value| !value.trim().is_empty())
            .map_or_else(|| self.inner.config.media_root.clone(), PathBuf::from);
        // Listing is read-only. An unrestricted host still validates every
        // mutation against the authorized roots, so widening what the browser
        // may *show* does not widen what it may change.
        let authorized = if unrestricted {
            requested.clone()
        } else {
            self.inner
                .dependencies
                .paths
                .authorize_read(&requested)
                .await?
        };

        if !authorized.is_dir() {
            return Err(RuntimeError::invalid(format!(
                "browse path is not a directory: {}",
                display_path(&authorized)
            )));
        }

        let mut entries = Vec::new();
        let mut directory = tokio::fs::read_dir(&authorized).await?;
        while let Some(entry) = directory.next_entry().await? {
            let metadata = match entry.metadata().await {
                Ok(metadata) => metadata,
                Err(_) => continue,
            };
            let kind = if metadata.is_dir() {
                FileSystemEntryKind::Folder
            } else if metadata.is_file() {
                FileSystemEntryKind::File
            } else {
                continue;
            };
            entries.push(FileSystemEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                path: display_path(&entry.path()),
                kind,
                size_bytes: metadata.is_file().then_some(metadata.len()),
                modified_utc: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH).into(),
            });
        }
        entries.sort_by(|left, right| {
            let left_folder = left.kind == FileSystemEntryKind::Folder;
            let right_folder = right.kind == FileSystemEntryKind::Folder;
            right_folder
                .cmp(&left_folder)
                .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
        });
        let parent_path = match self.inner.config.browse_scope {
            // Confined hosts stop at their roots: offering a parent the caller
            // cannot then list would be a dead end.
            BrowseScope::AuthorizedRootsOnly => authorized.parent().and_then(|parent| {
                self.inner
                    .config
                    .authorized_roots
                    .iter()
                    .any(|(root, _)| parent.starts_with(root))
                    .then(|| display_path(parent))
            }),
            // Above a volume root is the volume list, addressed as the empty
            // path, so the user can always navigate all the way out.
            BrowseScope::Unrestricted => Some(match authorized.parent() {
                Some(parent) => display_path(parent),
                // Rust folds `\\server\share` into a single path prefix, so a
                // share root reports no parent even though the server above it
                // is real and listable. Falling through to the volume list here
                // would skip a level that a file manager shows.
                None => match classify_unc(&display_path(&authorized)) {
                    Some(UncTarget::Share { server }) => format!(r"\\{server}"),
                    _ => String::new(),
                },
            }),
        };
        Ok(FileSystemResponse {
            path: display_path(&authorized),
            parent_path,
            entries,
        })
    }

    pub async fn start_scan(&self, mut request: ScanRequest) -> RuntimeResult<ScanJobResponse> {
        if request.all_sources().is_empty() {
            request.source_path = Some(display_path(&self.inner.config.media_root));
        }
        let request_fingerprint = stable_fingerprint(&request)
            .map_err(|error| RuntimeError::internal(error.to_string()))?;
        let key = IdempotencyKey::generate();
        let scan = Arc::clone(&self.inner.scan);
        let current = Arc::clone(&self.inner.current_scan);
        let results = Arc::clone(&self.inner.scan_results);
        // Scanning is the most-run operation, so it has to reach the operation
        // log too; otherwise the Logs page stays empty through normal use.
        let logs = Arc::clone(&self.inner.dependencies.logs);
        let (result_sender, result_receiver) = oneshot::channel();
        let spec = JobSpec {
            kind: JobKind::Scan,
            idempotency_key: key,
            request_fingerprint,
            plan_id: None,
            total: 0,
            resources: request
                .all_sources()
                .into_iter()
                .map(mkvo_domain::ResourceClaim::read)
                .collect(),
        };
        let accepted = self
            .inner
            .jobs
            .start(spec, move |context| async move {
                let outcome = scan
                    .scan(&request, context.cancellation_token(), &context)
                    .await?;
                let state = scan_result_state(&outcome);
                {
                    let mut current = current.write().await;
                    current.updated_utc = Some(Utc::now());
                    current.files.clone_from(&outcome.files);
                    current.summary = outcome.summary;
                    // A rescan after a mutating job changes which paths exist,
                    // so a selection naming a file that is gone is dropped here
                    // rather than being handed to the next operation.
                    current.reconcile_selection();
                }
                let summary = outcome.summary;
                let detail = format!(
                    "{} file(s): {} MKV, {} MP4, {} cached, {} failed",
                    summary.total, summary.mkv, summary.mp4, summary.cached, summary.failed
                );
                if let Err(error) = logs
                    .append(&mkvo_contracts::OperationLogEntry {
                        timestamp_utc: Utc::now(),
                        correlation_id: context.correlation_id(),
                        area: "Scan".to_owned(),
                        level: if summary.failed > 0 {
                            mkvo_contracts::LogLevel::Warning
                        } else {
                            mkvo_contracts::LogLevel::Information
                        },
                        message: "Scan completed".to_owned(),
                        detail,
                    })
                    .await
                {
                    tracing::warn!(%error, "scan completion could not be logged");
                }
                let persisted_state = state.clone();
                let _ = result_sender.send(state);
                Ok(JobCompletion {
                    result: Some(serde_json::to_value(persisted_state).map_err(|error| {
                        ApplicationError::Internal(format!(
                            "scan result serialization failed: {error}"
                        ))
                    })?),
                    message: Some("Scan completed".to_owned()),
                })
            })
            .await?;
        let accepted_id = accepted.id;
        tokio::spawn(async move {
            if let Ok(state) = result_receiver.await {
                results.write().await.insert(accepted_id, state);
            }
        });
        let snapshot = self
            .inner
            .jobs
            .get(accepted.id)
            .await?
            .ok_or_else(|| RuntimeError::internal("new scan job was not persisted"))?;
        self.scan_job_response(&snapshot).await
    }

    pub async fn get_scan_job(&self, id: &str) -> RuntimeResult<ScanJobResponse> {
        let id = parse_job_id(id)?;
        let snapshot = self
            .inner
            .jobs
            .get(id)
            .await?
            .ok_or_else(|| RuntimeError::not_found(format!("scan job {id}")))?;
        if snapshot.kind != JobKind::Scan {
            return Err(RuntimeError::not_found(format!("scan job {id}")));
        }
        self.scan_job_response(&snapshot).await
    }

    pub async fn cancel_scan(&self, id: &str) -> RuntimeResult<ScanJobResponse> {
        let id = parse_job_id(id)?;
        let snapshot = self.inner.jobs.cancel(id).await?;
        if snapshot.kind != JobKind::Scan {
            return Err(RuntimeError::not_found(format!("scan job {id}")));
        }
        self.scan_job_response(&snapshot).await
    }

    pub async fn get_current_scan_files(&self) -> RuntimeResult<CurrentScanResponse> {
        let state = self.inner.current_scan.read().await;
        Ok(current_scan_response(&state))
    }

    /// Replace the selected working set.
    ///
    /// Paths outside the current scan are rejected rather than stored: a
    /// selection is only meaningful against files the backend actually knows
    /// about, and accepting an unknown path would let a stale or crafted
    /// request reach an operation.
    ///
    /// The selection lives exactly as long as the working set it refers to.
    /// Both are in-memory, so a page reload keeps them and a host restart
    /// clears them together — persisting the selection alone would restore a
    /// set of paths with no files behind them.
    pub async fn set_file_selection(
        &self,
        request: mkvo_contracts::FileSelectionRequest,
    ) -> RuntimeResult<CurrentScanResponse> {
        let mut state = self.inner.current_scan.write().await;
        let available: BTreeSet<String> = state
            .files
            .iter()
            .map(|file| mkvo_application::paths::path_key(&file.path))
            .collect();

        let mut selected = BTreeSet::new();
        let mut unknown = Vec::new();
        for path in &request.paths {
            let key = mkvo_application::paths::path_key(std::path::Path::new(path));
            if available.contains(&key) {
                selected.insert(key);
            } else {
                unknown.push(path.clone());
            }
        }
        if !unknown.is_empty() {
            return Err(RuntimeError::invalid(format!(
                "{} selected path(s) are not in the current scan: {}",
                unknown.len(),
                unknown.join(", ")
            )));
        }

        state.selected = selected;
        Ok(current_scan_response(&state))
    }

    pub async fn clear_current_scan_files(&self) -> RuntimeResult<CurrentScanResponse> {
        let mut state = self.inner.current_scan.write().await;
        *state = CurrentScanState::default();
        Ok(current_scan_response(&state))
    }

    async fn scan_job_response(&self, snapshot: &JobSnapshot) -> RuntimeResult<ScanJobResponse> {
        let state = self
            .inner
            .scan_results
            .read()
            .await
            .get(&snapshot.id)
            .cloned()
            .or_else(|| scan_state_from_snapshot(snapshot))
            .unwrap_or_default();
        Ok(ScanJobResponse::from_snapshot(
            snapshot,
            if state.rows.is_empty() {
                state.files.iter().map(MediaFileRow::from).collect()
            } else {
                state.rows
            },
            state.skipped,
            state.summary,
        ))
    }

    pub async fn get_web_settings(&self) -> RuntimeResult<WebSettings> {
        let response = self.settings_service().load().await?;
        let compatibility = self.inner.compatibility_settings.read().await.clone();
        let mut view = web_settings(&response.settings, &response.secret_status, &compatibility);
        view.has_tvdb_api_key = self
            .secret_alias(&["provider.tvdb.api_key", "tvdbApiKey"])
            .await?
            .is_some();
        view.has_tvdb_pin = self
            .secret_alias(&["provider.tvdb.pin", "tvdbPin"])
            .await?
            .is_some();
        view.has_tmdb_api_key = self
            .secret_alias(&["provider.tmdb.api_key", "tmdbApiKey"])
            .await?
            .is_some();
        view.has_anidb_client = self
            .secret_alias(&["provider.anidb.client", "anidbClient"])
            .await?
            .is_some();
        Ok(view)
    }

    pub async fn save_web_settings(
        &self,
        request: WebSettingsRequest,
    ) -> RuntimeResult<WebSettings> {
        let current_compatibility = self.inner.compatibility_settings.read().await.clone();
        let compatibility = updated_compatibility_settings(&current_compatibility, &request);
        let loaded = self.settings_service().load().await?;
        let mut settings = loaded.settings;
        let mut secrets = Vec::new();
        apply_web_settings_request(&mut settings, request, &mut secrets)?;
        let response = self
            .settings_service()
            .save(mkvo_contracts::SaveSettingsRequest {
                settings,
                secrets,
                expected_revision: Some(loaded.revision),
            })
            .await?;
        persist_compatibility_settings(&self.inner.config.config_root, &compatibility).await?;
        *self.inner.compatibility_settings.write().await = compatibility.clone();
        self.authorize_configured_roots(&response.settings);
        let mut view = web_settings(&response.settings, &response.secret_status, &compatibility);
        view.has_tvdb_api_key = self
            .secret_alias(&["provider.tvdb.api_key", "tvdbApiKey"])
            .await?
            .is_some();
        view.has_tvdb_pin = self
            .secret_alias(&["provider.tvdb.pin", "tvdbPin"])
            .await?
            .is_some();
        view.has_tmdb_api_key = self
            .secret_alias(&["provider.tmdb.api_key", "tmdbApiKey"])
            .await?
            .is_some();
        view.has_anidb_client = self
            .secret_alias(&["provider.anidb.client", "anidbClient"])
            .await?
            .is_some();
        Ok(view)
    }

    pub async fn test_media_server_connection(
        &self,
        request: MediaServerConnectionRequest,
    ) -> RuntimeResult<MediaServerTestResponse> {
        let loaded = self.settings_service().load().await?;
        let stored = request.id.as_deref().and_then(|id| {
            loaded
                .settings
                .media_servers
                .iter()
                .find(|server| server.id.to_string().eq_ignore_ascii_case(id))
        });
        let kind = request
            .server_type
            .as_deref()
            .map(parse_server_kind)
            .transpose()?
            .or_else(|| stored.map(|server| server.kind))
            .unwrap_or(MediaServerKind::Jellyfin);
        let url = request
            .server_url
            .filter(|value| !value.trim().is_empty())
            .or_else(|| stored.map(|server| server.server_url.clone()))
            .ok_or_else(|| RuntimeError::invalid("Enter a server URL."))?;
        let credential = match request.api_key {
            Some(value) => value,
            None => {
                let key = stored
                    .and_then(|server| server.credential.secret_reference.as_deref())
                    .unwrap_or("media_server.temporary.api_key");
                self.secret_alias(&[key]).await?.unwrap_or_default()
            }
        };
        let client = media_server_client(kind, &loaded.settings);
        let connection = MediaServerConnection {
            kind,
            base_url: url,
            credential,
        };
        match client
            .discover_libraries(&connection, CancellationToken::new())
            .await
        {
            Ok(libraries) => Ok(MediaServerTestResponse {
                success: true,
                status: if libraries.is_empty() {
                    "Connection succeeded, but no library paths were returned.".to_owned()
                } else {
                    format!(
                        "Connection succeeded. Found {} library path(s).",
                        libraries.len()
                    )
                },
                library_count: libraries.len(),
            }),
            Err(error) => Ok(MediaServerTestResponse {
                success: false,
                status: error.to_string(),
                library_count: 0,
            }),
        }
    }

    pub async fn sync_media_server_libraries(
        &self,
        id: &str,
    ) -> RuntimeResult<MediaServerSyncResponse> {
        let loaded = self.settings_service().load().await?;
        let mut settings = loaded.settings;
        let index = settings
            .media_servers
            .iter()
            .position(|server| server.id.to_string().eq_ignore_ascii_case(id))
            .ok_or_else(|| RuntimeError::not_found(format!("media server {id}")))?;
        let server = settings.media_servers[index].clone();
        let key = server
            .credential
            .secret_reference
            .as_deref()
            .unwrap_or("mediaServer:missing");
        let legacy_key = format!("mediaServer:{}", server.id);
        let credential = self
            .secret_alias(&[key, &legacy_key])
            .await?
            .unwrap_or_default();
        let connection = MediaServerConnection {
            kind: server.kind,
            base_url: server.server_url.clone(),
            credential,
        };
        let libraries = media_server_client(server.kind, &settings)
            .discover_libraries(&connection, CancellationToken::new())
            .await?;
        settings.media_servers[index]
            .libraries
            .clone_from(&libraries);
        settings.media_servers[index].last_synced_at = Some(Utc::now());
        let saved = self
            .settings_service()
            .save(mkvo_contracts::SaveSettingsRequest {
                settings,
                secrets: Vec::new(),
                expected_revision: Some(loaded.revision),
            })
            .await?;
        let server = web_media_server(&saved.settings.media_servers[index]);
        let status = format!(
            "Sync complete for {}: {} library path(s).",
            server.name,
            libraries.len()
        );
        Ok(MediaServerSyncResponse {
            libraries: server.libraries.clone(),
            server,
            status,
        })
    }

    pub async fn search_rename_metadata(
        &self,
        request: RenameSearchRequest,
    ) -> RuntimeResult<Vec<RenameSearchResult>> {
        if request.query.trim().is_empty() {
            return Err(RuntimeError::invalid("Enter a title to search."));
        }
        let provider = self
            .provider_client(request.provider.as_deref(), request.language.as_deref())
            .await?;
        let results = provider
            .search(
                request.query.trim(),
                request.language.as_deref(),
                CancellationToken::new(),
            )
            .await?;
        Ok(results.into_iter().map(rename_search_result).collect())
    }

    pub async fn load_rename_scopes(
        &self,
        request: RenameScopesRequest,
    ) -> RuntimeResult<Vec<RenameScopeRow>> {
        let provider = self
            .provider_client(request.provider.as_deref(), request.language.as_deref())
            .await?;
        let episodes = provider
            .episodes(
                &request.selected_result.media_id(),
                request.language.as_deref(),
                CancellationToken::new(),
            )
            .await?;
        let mut seasons: Vec<_> = episodes.iter().map(|episode| episode.season).collect();
        seasons.sort_unstable();
        seasons.dedup();
        let mut scopes = vec![RenameScopeRow {
            key: "all".to_owned(),
            label: format!("All episodes ({})", episodes.len()),
            is_selected: true,
        }];
        scopes.extend(seasons.into_iter().map(|season| RenameScopeRow {
            key: format!("season:{season}"),
            label: format!("Season {season}"),
            is_selected: false,
        }));
        Ok(scopes)
    }

    pub async fn test_rename_provider(
        &self,
        request: RenameProviderTestRequest,
    ) -> RuntimeResult<RenameProviderTestResponse> {
        let provider = self
            .provider_client(request.provider.as_deref(), request.language.as_deref())
            .await?;
        match provider.test(CancellationToken::new()).await {
            Ok(()) => Ok(RenameProviderTestResponse {
                success: true,
                status: "Metadata provider connection successful.".to_owned(),
            }),
            Err(error) => Ok(RenameProviderTestResponse {
                success: false,
                status: format!("Metadata provider connection failed: {error}"),
            }),
        }
    }

    pub async fn get_operation_job(&self, id: &str) -> RuntimeResult<OperationJobResponse> {
        let id = parse_job_id(id)?;
        let snapshot = self
            .inner
            .jobs
            .get(id)
            .await?
            .ok_or_else(|| RuntimeError::not_found(format!("operation job {id}")))?;
        if snapshot.kind == JobKind::Scan {
            return Err(RuntimeError::not_found(format!("operation job {id}")));
        }
        Ok(OperationJobResponse::from_snapshot(&snapshot))
    }

    pub async fn cancel_operation_job(&self, id: &str) -> RuntimeResult<OperationJobResponse> {
        let id = parse_job_id(id)?;
        let snapshot = self.inner.jobs.cancel(id).await?;
        if snapshot.kind == JobKind::Scan {
            return Err(RuntimeError::not_found(format!("operation job {id}")));
        }
        Ok(OperationJobResponse::from_snapshot(&snapshot))
    }

    pub async fn subscribe_job_events(
        &self,
        id: &str,
    ) -> RuntimeResult<broadcast::Receiver<JobEventEnvelope>> {
        self.inner
            .jobs
            .subscribe(parse_job_id(id)?)
            .await
            .map_err(Into::into)
    }

    pub async fn get_logs(&self) -> RuntimeResult<OperationLogResponse> {
        Ok(OperationLogResponse {
            entries: self
                .inner
                .dependencies
                .logs
                .query(&LogQuery::default())
                .await?,
        })
    }

    pub async fn clear_logs(&self) -> RuntimeResult<OperationLogResponse> {
        self.inner.dependencies.logs.clear().await?;
        self.get_logs().await
    }

    /// Render the operation log as plain text for download.
    ///
    /// The host writes the file, so this returns the content and a suggested
    /// name rather than touching the filesystem itself — the browser and the
    /// desktop save it in different ways.
    pub async fn export_logs(&self) -> RuntimeResult<LogExport> {
        let entries = self
            .inner
            .dependencies
            .logs
            .query(&LogQuery::default())
            .await?;
        let mut content = String::new();
        for entry in &entries {
            content.push_str(&format!(
                "{timestamp}\t{level:?}\t{area}\t{correlation}\t{message}",
                timestamp = entry.timestamp_utc.to_rfc3339(),
                level = entry.level,
                area = entry.area,
                correlation = entry.correlation_id,
                message = entry.message,
            ));
            if !entry.detail.trim().is_empty() {
                // Keep one entry on one line so the export stays greppable.
                content.push('\t');
                content.push_str(&entry.detail.replace(['\r', '\n'], " "));
            }
            content.push('\n');
        }
        Ok(LogExport {
            file_name: format!("mkvo-logs-{}.txt", Utc::now().format("%Y%m%d-%H%M%S")),
            entry_count: entries.len(),
            content,
        })
    }

    /// Recent jobs across kinds, newest first, for the Logs and Jobs views.
    pub async fn list_recent_jobs(
        &self,
        limit: Option<usize>,
    ) -> RuntimeResult<RecentJobsResponse> {
        let limit = limit.unwrap_or(50).clamp(1, 500);
        let jobs = self.inner.dependencies.jobs.list_recent(limit).await?;
        Ok(RecentJobsResponse { jobs })
    }

    pub(crate) fn settings_service(&self) -> SettingsService {
        SettingsService::new(
            Arc::clone(&self.inner.dependencies.settings),
            Arc::clone(&self.inner.dependencies.secrets),
            self.inner.dependencies.watcher.clone(),
        )
    }

    pub(crate) async fn provider_client(
        &self,
        requested: Option<&str>,
        requested_language: Option<&str>,
    ) -> RuntimeResult<Arc<dyn MetadataProviderClient>> {
        let loaded = self.settings_service().load().await?;
        let provider = requested
            .map(parse_provider)
            .transpose()?
            .unwrap_or(loaded.settings.rename.provider);
        let language = requested_language
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(&loaded.settings.rename.language)
            .to_owned();
        match provider {
            MetadataProvider::Tvdb => {
                let api_key = self
                    .secret_alias(&["provider.tvdb.api_key", "tvdbApiKey"])
                    .await?
                    .ok_or_else(|| RuntimeError::invalid("TVDB API key is not configured"))?;
                let pin = self
                    .secret_alias(&["provider.tvdb.pin", "tvdbPin"])
                    .await?
                    .map(SecretString::new);
                Ok(Arc::new(ConfiguredTvdbProvider::new(
                    TvdbClient::new(),
                    ProviderCredentials {
                        api_key: SecretString::new(api_key),
                        pin,
                    },
                    language,
                )))
            }
            MetadataProvider::Tmdb => {
                let api_key = self
                    .secret_alias(&["provider.tmdb.api_key", "tmdbApiKey"])
                    .await?
                    .ok_or_else(|| RuntimeError::invalid("TMDB API key is not configured"))?;
                Ok(Arc::new(ConfiguredTmdbProvider::new(
                    TmdbClient::new(),
                    ProviderCredentials::api_key(api_key),
                    language,
                )))
            }
            // AniList is a public GraphQL API and needs no credentials.
            MetadataProvider::AniList => Ok(Arc::new(ConfiguredAniListProvider::new(
                AniListClient::new(),
                language,
            ))),
            // AniDB identifies callers by a registered client name rather than a
            // key, stored as `name/version`. Searching only reads the public
            // title dump, so a missing client name is reported by the episode
            // call rather than blocking search.
            MetadataProvider::AniDb => {
                let client = self
                    .secret_alias(&["provider.anidb.client", "anidbClient"])
                    .await?
                    .unwrap_or_default();
                Ok(Arc::new(ConfiguredAniDbProvider::new(
                    AniDbClient::new(),
                    ProviderCredentials::api_key(client),
                    language,
                )))
            }
        }
    }

    pub(crate) async fn current_domain_files(&self) -> Vec<MediaFile> {
        self.inner.current_scan.read().await.files.clone()
    }

    pub(crate) fn dependencies(&self) -> &RuntimeDependencies {
        &self.inner.dependencies
    }

    pub(crate) fn jobs(&self) -> &Arc<JobSupervisor> {
        &self.inner.jobs
    }

    pub(crate) fn config(&self) -> &RuntimeConfig {
        &self.inner.config
    }

    /// Authorize a folder the user picked in the browser.
    ///
    /// Browsing may range wider than the authorized roots, so a folder found
    /// that way is not yet usable as a scan source. Choosing it is the user's
    /// own explicit act and carries the same weight as the native folder
    /// picker, but it still goes through the same canonicalization and
    /// validation as any other grant — the path is not trusted for having come
    /// from the browser.
    ///
    /// Hosts that confine browsing refuse this: on a network service the caller
    /// is not necessarily the machine's owner.
    pub fn authorize_browsed_root(&self, path: &str) -> RuntimeResult<AuthorizedRootGrant> {
        if self.inner.config.browse_scope != BrowseScope::Unrestricted {
            return Err(RuntimeError::invalid(
                "this host only allows sources inside its configured roots",
            ));
        }
        self.grant_authorized_root(Path::new(path), true)
    }

    /// Grant the roots a saved settings revision just made authoritative.
    ///
    /// Composition grants persisted roots at startup, so without this a scan
    /// root or watch folder chosen in Settings stays unauthorized until the next
    /// restart. Choosing a root in Settings is the user's own explicit act, so
    /// it carries the same authority as the startup grant; a path that cannot be
    /// canonicalized is reported and skipped rather than failing the save, which
    /// has already been committed.
    fn authorize_configured_roots(&self, settings: &AppSettings) {
        for path in settings
            .scan
            .default_root
            .iter()
            .chain(settings.watch.roots.iter())
        {
            if let Err(error) = self.grant_authorized_root(path, true) {
                tracing::warn!(
                    path = %path.display(),
                    error = %error,
                    "configured root could not be authorized"
                );
            }
        }
    }

    /// Host-only capability grant intended for a native folder picker or
    /// trusted server bootstrap. Transports should not expose this directly.
    pub fn grant_authorized_root(
        &self,
        path: impl AsRef<Path>,
        writable: bool,
    ) -> RuntimeResult<AuthorizedRootGrant> {
        let roots = self
            .inner
            .dependencies
            .authorized_roots
            .as_ref()
            .ok_or_else(|| {
                RuntimeError::internal(
                    "dynamic path grants are unavailable for custom dependencies",
                )
            })?;
        let root = roots
            .grant(path, writable)
            .map_err(|error| RuntimeError::invalid(error.to_string()))?;
        Ok(AuthorizedRootGrant {
            path: display_path(root.path()),
            writable: root.writable(),
        })
    }

    #[must_use]
    pub fn list_authorized_roots(&self) -> Vec<AuthorizedRootGrant> {
        self.inner
            .dependencies
            .authorized_roots
            .as_ref()
            .map(|roots| {
                roots
                    .roots()
                    .into_iter()
                    .map(|root| AuthorizedRootGrant {
                        path: display_path(root.path()),
                        writable: root.writable(),
                    })
                    .collect()
            })
            .unwrap_or_else(|| {
                self.inner
                    .config
                    .authorized_roots
                    .iter()
                    .map(|(path, writable)| AuthorizedRootGrant {
                        path: display_path(path),
                        writable: *writable,
                    })
                    .collect()
            })
    }

    pub(crate) fn legacy_rename_history(&self) -> &[LegacyRenameBatchRecord] {
        &self.inner.legacy_rename_history
    }

    async fn secret_alias(&self, keys: &[&str]) -> RuntimeResult<Option<String>> {
        for key in keys {
            if let Some(value) = self.inner.dependencies.secrets.get(key).await?
                && !value.is_empty()
            {
                return Ok(Some(value));
            }
        }
        Ok(None)
    }
}

fn scan_result_state(outcome: &ScanOutcome) -> ScanResultState {
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

fn current_scan_response(state: &CurrentScanState) -> CurrentScanResponse {
    CurrentScanResponse {
        updated_utc: state.updated_utc,
        files: state.files.iter().map(MediaFileRow::from).collect(),
        summary: state.summary,
        selected_paths: state.selected_display_paths(),
    }
}

fn scan_state_from_snapshot(snapshot: &JobSnapshot) -> Option<ScanResultState> {
    serde_json::from_value(snapshot.result.clone()?).ok()
}

fn classify_recovery(
    snapshot: &JobSnapshot,
    journal: Option<&mkvo_application::JournalRecord>,
) -> (RecoveryDisposition, String) {
    if let Some(journal) = journal {
        return match journal.status {
            JournalStatus::Completed => (
                RecoveryDisposition::Completed,
                format!(
                    "the mutation journal completed at step {}; the stale job record was reconciled",
                    journal.step
                ),
            ),
            JournalStatus::RolledBack => (
                RecoveryDisposition::CleanRetry,
                format!(
                    "the interrupted operation was rolled back at step {}; it is safe to build a new plan and retry",
                    journal.step
                ),
            ),
            JournalStatus::Prepared if journal.step == 0 => (
                RecoveryDisposition::CleanRetry,
                "the journal was prepared but no mutation step completed; build a new plan and retry"
                    .to_owned(),
            ),
            JournalStatus::Prepared
            | JournalStatus::Running
            | JournalStatus::Failed => (
                RecoveryDisposition::ManualReview,
                format!(
                    "the mutation journal stopped in {:?} at step {}; inspect its resources before retrying",
                    journal.status, journal.step
                ),
            ),
        };
    }

    if matches!(
        snapshot.status,
        JobStatus::Queued | JobStatus::WaitingForResources
    ) || matches!(
        snapshot.kind,
        JobKind::Scan | JobKind::LibraryAudit | JobKind::CacheReconcile
    ) {
        (
            RecoveryDisposition::CleanRetry,
            "no mutation journal exists and the job had not acquired mutation resources; retry with a new idempotency key"
                .to_owned(),
        )
    } else {
        (
            RecoveryDisposition::ManualReview,
            "a running mutating job has no durable journal; inspect source, staged, backup, and target paths before retrying"
                .to_owned(),
        )
    }
}

/// Render a path for the UI.
///
/// Authorized roots are canonicalized, and on Windows `fs::canonicalize` returns
/// the extended-length `\\?\` form. That prefix is correct to keep for process
/// arguments, where it is what lifts the `MAX_PATH` limit, but showing it to the
/// user is wrong: it is not what they typed and not what they can paste back.
/// It is only removed when doing so still yields a valid path.
pub(crate) fn display_path(path: &Path) -> String {
    mkvo_domain::normalized_path_text(path)
}

fn parse_job_id(value: &str) -> RuntimeResult<JobId> {
    value
        .parse()
        .map_err(|_| RuntimeError::invalid(format!("invalid job id: {value}")))
}

pub(crate) fn parse_provider(value: &str) -> RuntimeResult<MetadataProvider> {
    match value.trim().to_ascii_lowercase().as_str() {
        "tvdb" => Ok(MetadataProvider::Tvdb),
        "tmdb" => Ok(MetadataProvider::Tmdb),
        "anidb" => Ok(MetadataProvider::AniDb),
        "anilist" => Ok(MetadataProvider::AniList),
        _ => Err(RuntimeError::invalid(format!(
            "unknown metadata provider: {value}"
        ))),
    }
}

pub(crate) fn provider_name(value: MetadataProvider) -> &'static str {
    match value {
        MetadataProvider::Tvdb => "tvdb",
        MetadataProvider::Tmdb => "tmdb",
        MetadataProvider::AniDb => "anidb",
        MetadataProvider::AniList => "anilist",
    }
}

fn parse_server_kind(value: &str) -> RuntimeResult<MediaServerKind> {
    match value.trim().to_ascii_lowercase().as_str() {
        "emby" => Ok(MediaServerKind::Emby),
        "jellyfin" => Ok(MediaServerKind::Jellyfin),
        "plex" => Ok(MediaServerKind::Plex),
        _ => Err(RuntimeError::invalid(format!(
            "unknown media server type: {value}"
        ))),
    }
}

fn server_kind_name(value: MediaServerKind) -> &'static str {
    match value {
        MediaServerKind::Emby => "emby",
        MediaServerKind::Jellyfin => "jellyfin",
        MediaServerKind::Plex => "plex",
    }
}

fn web_settings(
    settings: &AppSettings,
    secret_status: &[mkvo_contracts::SecretStatus],
    compatibility: &CompatibilitySettings,
) -> WebSettings {
    let configured = |key: &str| {
        secret_status
            .iter()
            .any(|status| status.key == key && status.configured)
    };
    WebSettings {
        has_tvdb_api_key: configured("tvdbApiKey"),
        has_tvdb_pin: configured("tvdbPin"),
        has_tmdb_api_key: configured("tmdbApiKey"),
        has_anidb_client: configured("anidbClient"),
        tvdb_language: settings.rename.language.clone(),
        rename_lookup_provider: provider_name(settings.rename.provider).to_owned(),
        rename_template: settings.rename.template.clone(),
        rename_templates: settings.rename.templates.clone(),
        audio_name_presets: compatibility.audio_name_presets.clone(),
        subtitle_name_presets: compatibility.subtitle_name_presets.clone(),
        language_presets: compatibility.language_presets.clone(),
        mkv_merge_default_audio_languages: compatibility.mkv_merge_default_audio_languages.clone(),
        mkv_merge_default_subtitle_languages: compatibility
            .mkv_merge_default_subtitle_languages
            .clone(),
        mkv_tool_nix_directory: settings
            .tools
            .mkvtoolnix_directory
            .as_deref()
            .map(display_path),
        ffmpeg_directory: settings.tools.ffmpeg_directory.as_deref().map(display_path),
        default_root: settings.scan.default_root.as_deref().map(display_path),
        ignored_scan_folder_names: settings.scan.ignored_folder_names.iter().cloned().collect(),
        use_quick_hash_on_unreliable_timestamps: settings
            .scan
            .use_quick_hash_on_unreliable_timestamps,
        rename_preview_compact_view: settings.rename.compact_preview,
        max_scan_workers: settings.workers.max_scan_workers,
        max_edit_workers: settings.workers.max_edit_workers,
        max_remux_workers: settings.workers.max_remux_workers,
        watch_debounce_millis: settings.watch.debounce_millis,
        watch_reconciliation_interval_minutes: settings.watch.reconciliation_interval_minutes,
        watch_force_polling: settings.watch.force_polling,
        selected_theme_name: settings.appearance.selected_theme.clone(),
        custom_themes: settings
            .appearance
            .custom_themes
            .iter()
            .map(web_theme)
            .collect(),
        watch_folders: settings
            .watch
            .roots
            .iter()
            .map(|path| display_path(path))
            .collect(),
        enable_live_watch_folder_monitoring: settings.watch.enabled,
        media_servers: settings
            .media_servers
            .iter()
            .map(web_media_server)
            .collect(),
        media_server_path_mappings: settings
            .media_server_path_mappings
            .iter()
            .map(|mapping| WebMediaServerPathMapping {
                server_path_prefix: display_path(&mapping.remote_prefix),
                container_path_prefix: display_path(&mapping.local_prefix),
            })
            .collect(),
    }
}

fn apply_web_settings_request(
    settings: &mut AppSettings,
    request: WebSettingsRequest,
    secrets: &mut Vec<SecretUpdate>,
) -> RuntimeResult<()> {
    for (primary, alias, value) in [
        ("provider.tvdb.api_key", "tvdbApiKey", request.tvdb_api_key),
        ("provider.tvdb.pin", "tvdbPin", request.tvdb_pin),
        ("provider.tmdb.api_key", "tmdbApiKey", request.tmdb_api_key),
        ("provider.anidb.client", "anidbClient", request.anidb_client),
    ] {
        if let Some(value) = value {
            for key in [primary, alias] {
                secrets.push(SecretUpdate {
                    key: key.to_owned(),
                    clear: value.is_empty(),
                    value: (!value.is_empty()).then_some(value.clone()),
                });
            }
        }
    }
    if let Some(value) = request
        .tvdb_language
        .filter(|value| !value.trim().is_empty())
    {
        settings.rename.language = value.trim().to_owned();
    }
    if let Some(value) = request.rename_lookup_provider {
        settings.rename.provider = parse_provider(&value)?;
    }
    if let Some(value) = request
        .rename_template
        .filter(|value| !value.trim().is_empty())
    {
        settings.rename.template = value.trim().to_owned();
    }
    if let Some(values) = request.rename_templates {
        settings.rename.templates = normalized_strings(values);
    }
    if let Some(value) = request.rename_preview_compact_view {
        settings.rename.compact_preview = value;
    }
    if let Some(value) = requested_path(request.mkv_tool_nix_directory) {
        settings.tools.mkvtoolnix_directory = value;
    }
    if let Some(value) = requested_path(request.ffmpeg_directory) {
        settings.tools.ffmpeg_directory = value;
    }
    if let Some(value) = requested_path(request.default_root) {
        settings.scan.default_root = value;
    }
    if let Some(values) = request.ignored_scan_folder_names {
        settings.scan.ignored_folder_names = normalized_strings(values).into_iter().collect();
    }
    if let Some(value) = request.use_quick_hash_on_unreliable_timestamps {
        settings.scan.use_quick_hash_on_unreliable_timestamps = value;
    }
    // Worker limits are clamped by `WorkerSettings::normalized` so an out-of-range
    // request cannot raise tool pressure beyond the documented safety defaults.
    if let Some(value) = request.max_scan_workers {
        settings.workers.max_scan_workers = value;
    }
    if let Some(value) = request.max_edit_workers {
        settings.workers.max_edit_workers = value;
    }
    if let Some(value) = request.max_remux_workers {
        settings.workers.max_remux_workers = value;
    }
    settings.workers = settings.workers.normalized();
    if let Some(value) = request.watch_debounce_millis {
        settings.watch.debounce_millis = value;
    }
    if let Some(value) = request.watch_reconciliation_interval_minutes {
        settings.watch.reconciliation_interval_minutes = value;
    }
    if let Some(value) = request.watch_force_polling {
        settings.watch.force_polling = value;
    }
    if let Some(value) = request
        .selected_theme_name
        .filter(|value| !value.trim().is_empty())
    {
        settings.appearance.selected_theme = value.trim().to_owned();
    }
    if let Some(themes) = request.custom_themes {
        settings.appearance.custom_themes = themes.into_iter().map(domain_theme).collect();
    }
    if let Some(values) = request.watch_folders {
        settings.watch.roots = normalized_strings(values)
            .into_iter()
            .map(PathBuf::from)
            .collect();
    }
    if let Some(value) = request.enable_live_watch_folder_monitoring {
        settings.watch.enabled = value;
    }
    if let Some(mappings) = request.media_server_path_mappings {
        settings.media_server_path_mappings = mappings
            .into_iter()
            .map(|mapping| PathMapping {
                remote_prefix: PathBuf::from(mapping.server_path_prefix),
                local_prefix: PathBuf::from(mapping.container_path_prefix),
            })
            .collect();
    }
    if let Some(servers) = request.media_servers {
        let existing: HashMap<_, _> = settings
            .media_servers
            .iter()
            .cloned()
            .map(|server| (server.id, server))
            .collect();
        let mut updated = Vec::with_capacity(servers.len());
        for server in servers {
            let id = server
                .id
                .as_deref()
                .and_then(|value| value.parse::<MediaServerId>().ok())
                .unwrap_or_default();
            let prior = existing.get(&id);
            let secret_reference = format!("media_server.{id}.api_key");
            let legacy_secret_reference = format!("mediaServer:{id}");
            let configured_update = server.api_key.as_ref().map(|value| !value.is_empty());
            if let Some(api_key) = server.api_key {
                for key in [&secret_reference, &legacy_secret_reference] {
                    secrets.push(SecretUpdate {
                        key: key.clone(),
                        clear: api_key.is_empty(),
                        value: (!api_key.is_empty()).then_some(api_key.clone()),
                    });
                }
            }
            updated.push(MediaServerSettings {
                id,
                name: server
                    .name
                    .or_else(|| prior.map(|value| value.name.clone()))
                    .unwrap_or_else(|| "Media server".to_owned()),
                kind: server
                    .server_type
                    .as_deref()
                    .map(parse_server_kind)
                    .transpose()?
                    .or_else(|| prior.map(|value| value.kind))
                    .unwrap_or(MediaServerKind::Jellyfin),
                server_url: server
                    .server_url
                    .or_else(|| prior.map(|value| value.server_url.clone()))
                    .unwrap_or_default(),
                credential: CredentialState {
                    configured: configured_update
                        .unwrap_or_else(|| prior.is_some_and(|value| value.credential.configured)),
                    masked_hint: prior.and_then(|value| value.credential.masked_hint.clone()),
                    secret_reference: Some(secret_reference),
                },
                is_default: server.is_default,
                libraries: server.libraries.map_or_else(
                    || prior.map_or_else(Vec::new, |value| value.libraries.clone()),
                    |libraries| {
                        libraries
                            .into_iter()
                            .map(|library| MediaServerLibrary {
                                id: library.id,
                                name: library.name,
                                media_type: Some(library.media_type),
                                server_path: PathBuf::from(library.server_path),
                                local_path: Some(PathBuf::from(library.container_path)),
                                enabled: library.is_enabled,
                            })
                            .collect()
                    },
                ),
                last_synced_at: prior.and_then(|value| value.last_synced_at),
            });
        }
        settings.media_servers = updated;
    }
    Ok(())
}

fn normalized_strings(values: Vec<String>) -> Vec<String> {
    let mut values: Vec<_> = values
        .into_iter()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .collect();
    values.sort_by_key(|value| value.to_lowercase());
    values.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
    values
}

fn web_theme(theme: &mkvo_domain::ThemeDefinition) -> mkvo_contracts::ThemeDefinition {
    mkvo_contracts::ThemeDefinition {
        name: theme.name.clone(),
        colors: theme.colors.clone(),
    }
}

fn domain_theme(theme: mkvo_contracts::ThemeDefinition) -> mkvo_domain::ThemeDefinition {
    mkvo_domain::ThemeDefinition {
        name: theme.name,
        colors: theme.colors,
    }
}

/// Optional path fields use `Option<Option<String>>`: the outer `None` means
/// "unchanged" and an inner empty/blank value means "clear this setting".
fn requested_path(value: Option<Option<String>>) -> Option<Option<PathBuf>> {
    value.map(|value| {
        value
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    })
}

fn web_media_server(server: &MediaServerSettings) -> WebMediaServer {
    WebMediaServer {
        id: server.id.to_string(),
        name: server.name.clone(),
        server_type: server_kind_name(server.kind).to_owned(),
        server_url: server.server_url.clone(),
        has_api_key: server.credential.configured,
        is_default: server.is_default,
        last_synced_utc: server.last_synced_at,
        libraries: server
            .libraries
            .iter()
            .map(|library| WebMediaServerLibraryPath {
                id: library.id.clone(),
                name: library.name.clone(),
                media_type: library.media_type.clone().unwrap_or_default(),
                server_path: display_path(&library.server_path),
                container_path: library
                    .local_path
                    .as_deref()
                    .map_or_else(String::new, display_path),
                is_enabled: library.enabled,
            })
            .collect(),
    }
}

fn media_server_client(
    kind: MediaServerKind,
    settings: &AppSettings,
) -> Arc<dyn MediaServerClient> {
    let mappings = settings
        .media_server_path_mappings
        .iter()
        .map(|mapping| MediaServerPathMapping {
            server_path_prefix: display_path(&mapping.remote_prefix),
            local_path_prefix: mapping.local_prefix.clone(),
        })
        .collect();
    Arc::new(ConfiguredMediaServerClient::new(
        kind,
        MediaServerDiscoveryClient::new(),
        mappings,
    ))
}

fn rename_search_result(value: mkvo_domain::ProviderSearchResult) -> RenameSearchResult {
    let id = value.id.parse::<u64>().map_or_else(
        |_| serde_json::Value::String(value.id),
        serde_json::Value::from,
    );
    let provider = provider_name(value.provider).to_owned();
    let year = value.year.map_or_else(String::new, |year| year.to_string());
    RenameSearchResult {
        id,
        display_name: if year.is_empty() {
            value.title.clone()
        } else {
            format!("{} ({year})", value.title)
        },
        name: value.title,
        year,
        overview: value.overview.unwrap_or_default(),
        provider: provider.clone(),
        format: "series".to_owned(),
        database_url: String::new(),
        provider_display: provider.to_ascii_uppercase(),
    }
}

fn compatibility_settings_path(config_root: &Path) -> PathBuf {
    config_root.join("web-settings-extra.json")
}

fn load_compatibility_settings(config_root: &Path) -> CompatibilitySettings {
    std::fs::read(compatibility_settings_path(config_root))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn updated_compatibility_settings(
    current: &CompatibilitySettings,
    request: &WebSettingsRequest,
) -> CompatibilitySettings {
    let mut updated = current.clone();
    if let Some(values) = request.audio_name_presets.clone() {
        updated.audio_name_presets = normalized_strings(values);
    }
    if let Some(values) = request.subtitle_name_presets.clone() {
        updated.subtitle_name_presets = normalized_strings(values);
    }
    if let Some(values) = request.language_presets.clone() {
        updated.language_presets = normalized_strings(values);
    }
    if let Some(value) = request
        .mkv_merge_default_audio_languages
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        updated.mkv_merge_default_audio_languages = value.trim().to_owned();
    }
    if let Some(value) = request
        .mkv_merge_default_subtitle_languages
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        updated.mkv_merge_default_subtitle_languages = value.trim().to_owned();
    }
    updated
}

async fn persist_compatibility_settings(
    config_root: &Path,
    settings: &CompatibilitySettings,
) -> RuntimeResult<()> {
    let path = compatibility_settings_path(config_root);
    let bytes = serde_json::to_vec_pretty(settings)?;
    tokio::fs::write(path, bytes).await?;
    Ok(())
}

/// The volume list shown above every drive root.
///
/// Windows has no single filesystem root, so "up" from `C:\` is this list
/// rather than a directory. Unix does have one, but presenting `/` the same way
/// keeps the browser's navigation model identical on both.
fn volume_listing() -> FileSystemResponse {
    let mut entries = Vec::new();

    #[cfg(windows)]
    {
        for letter in b'A'..=b'Z' {
            let root = format!("{}:\\", letter as char);
            let path = std::path::Path::new(&root);
            if !path.is_dir() {
                continue;
            }
            entries.push(FileSystemEntry {
                name: format!("{}:", letter as char),
                path: root.clone(),
                kind: FileSystemEntryKind::Folder,
                size_bytes: None,
                modified_utc: std::fs::metadata(path)
                    .and_then(|metadata| metadata.modified())
                    .unwrap_or(SystemTime::UNIX_EPOCH)
                    .into(),
            });
        }
    }

    #[cfg(not(windows))]
    {
        entries.push(FileSystemEntry {
            name: "/".to_owned(),
            path: "/".to_owned(),
            kind: FileSystemEntryKind::Folder,
            size_bytes: None,
            modified_utc: std::fs::metadata("/")
                .and_then(|metadata| metadata.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH)
                .into(),
        });
    }

    FileSystemResponse {
        path: String::new(),
        // Already at the top.
        parent_path: None,
        entries,
    }
}

/// The shares a server publishes, presented as an ordinary folder listing so
/// the browser can navigate into one without knowing it came from elsewhere.
fn server_listing(server: &str) -> RuntimeResult<FileSystemResponse> {
    let shares = list_server_shares(server).map_err(|error| {
        RuntimeError::invalid(format!(r"cannot list the shares of \\{server}: {error}"))
    })?;

    Ok(FileSystemResponse {
        path: format!(r"\\{server}"),
        // Above a server is the volume list, the same as above a drive.
        parent_path: Some(String::new()),
        entries: shares
            .into_iter()
            .map(|share| FileSystemEntry {
                name: share.name,
                path: share.path,
                kind: FileSystemEntryKind::Folder,
                // A share has no meaningful size or timestamp of its own;
                // asking the server for one would mean a round trip per share.
                size_bytes: None,
                modified_utc: SystemTime::UNIX_EPOCH.into(),
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The working set is authoritative in Rust, so a selection naming a file
    /// the backend does not have is refused rather than stored — otherwise a
    /// stale frontend could hand an operation a path that no longer exists.
    #[test]
    fn unknown_paths_are_rejected_and_known_ones_normalize() {
        let mut state = CurrentScanState {
            files: vec![
                media_at(r"\\?\C:\media\first.mkv"),
                media_at(r"\\?\C:\media\second.mkv"),
            ],
            ..CurrentScanState::default()
        };

        let available: BTreeSet<String> = state
            .files
            .iter()
            .map(|file| mkvo_application::paths::path_key(&file.path))
            .collect();
        // The working set holds canonicalized paths, but the UI is shown - and
        // sends back - the plain spelling. Both must name the same file.
        assert!(
            available.contains(&mkvo_application::paths::path_key(Path::new(
                r"C:\media\first.mkv"
            )))
        );
        assert!(
            !available.contains(&mkvo_application::paths::path_key(Path::new(
                r"C:\media\gone.mkv"
            )))
        );

        state
            .selected
            .insert(mkvo_application::paths::path_key(Path::new(
                r"C:\media\first.mkv",
            )));
        assert_eq!(
            state.selected_display_paths(),
            vec![r"C:\media\first.mkv".to_owned()]
        );
    }

    /// A job that consumes or renames a file changes which paths exist.
    #[test]
    fn reconciliation_drops_selections_whose_file_is_gone() {
        let mut state = CurrentScanState {
            files: vec![media_at("/media/a.mkv"), media_at("/media/b.mkv")],
            ..CurrentScanState::default()
        };
        for path in ["/media/a.mkv", "/media/b.mkv"] {
            state
                .selected
                .insert(mkvo_application::paths::path_key(std::path::Path::new(
                    path,
                )));
        }

        state.files = vec![media_at("/media/a.mkv"), media_at("/media/c.mkv")];
        state.reconcile_selection();

        assert_eq!(
            state.selected_display_paths(),
            vec!["/media/a.mkv".to_owned()]
        );
    }

    /// Clearing the working set is a transient step on the way to replacing it,
    /// so it must not be read as "the user deselected everything".
    #[test]
    fn an_empty_working_set_does_not_clear_the_selection() {
        let mut state = CurrentScanState {
            files: vec![media_at("/media/a.mkv")],
            ..CurrentScanState::default()
        };
        state
            .selected
            .insert(mkvo_application::paths::path_key(std::path::Path::new(
                "/media/a.mkv",
            )));

        state.files.clear();
        state.reconcile_selection();
        assert_eq!(state.selected.len(), 1);

        state.files = vec![media_at("/media/a.mkv")];
        state.reconcile_selection();
        assert_eq!(state.selected_display_paths().len(), 1);
    }

    fn media_at(path: &str) -> MediaFile {
        MediaFile {
            path: PathBuf::from(path),
            original_file_name: None,
            watch_root: None,
            relative_path: None,
            fingerprint: mkvo_domain::FileFingerprint {
                path: PathBuf::from(path),
                size_bytes: 1,
                modified_at: Utc::now(),
                quick_hash: None,
            },
            container: mkvo_domain::ContainerMetadata::default(),
            tracks: Vec::new(),
            attachments: Vec::new(),
            episode: None,
            provider_match: None,
            status: mkvo_domain::MediaStatus::Ready,
        }
    }

    /// The desktop must reach a library anywhere on the machine; a network
    /// service must not. The scope is the only thing that differs, and it
    /// governs listing alone.
    #[test]
    fn browse_scope_defaults_to_confined() {
        let config = RuntimeConfig::new("/media", "/config");
        assert_eq!(config.browse_scope, BrowseScope::AuthorizedRootsOnly);
    }

    /// Above a volume root is the volume list, addressed as the empty path, so
    /// an unrestricted browser can always navigate all the way out.
    #[test]
    fn the_volume_list_is_the_top_of_an_unrestricted_browser() {
        let listing = volume_listing();
        assert!(listing.path.is_empty());
        assert!(
            listing.parent_path.is_none(),
            "the volume list has no parent"
        );
        assert!(
            listing
                .entries
                .iter()
                .all(|entry| entry.kind == FileSystemEntryKind::Folder),
            "volumes are navigable"
        );
        // Every host this runs on has at least one readable volume.
        assert!(!listing.entries.is_empty());
    }

    /// Authorized roots are canonicalized, so on Windows every path the UI shows
    /// arrives in the extended-length form. `\\?\C:\Users\me\Videos` is not a
    /// path a user recognizes or can paste back into a folder field.
    #[test]
    fn windows_extended_length_prefixes_are_not_shown_to_the_user() {
        assert_eq!(
            display_path(Path::new(r"\\?\C:\Users\me\Videos")),
            r"C:\Users\me\Videos"
        );
        assert_eq!(
            display_path(Path::new(r"\\?\UNC\nas\media\Shows")),
            r"\\nas\media\Shows"
        );
        assert_eq!(
            display_path(Path::new(r"C:\Users\me\Videos")),
            r"C:\Users\me\Videos",
            "an ordinary path is unchanged"
        );
        assert_eq!(
            display_path(Path::new("/mnt/media/Shows")),
            "/mnt/media/Shows",
            "POSIX paths are unchanged"
        );
        assert_eq!(
            display_path(Path::new(r"\\?\Volume{9f3a}\media")),
            r"\\?\Volume{9f3a}\media",
            "a device path has no plain form, so it is left intact"
        );
    }

    /// Every field the settings page can edit must survive a request → domain →
    /// response round trip. A field that is mapped in only one direction reads
    /// back as its old value and silently discards the user's change.
    #[test]
    fn every_web_settings_field_survives_a_round_trip() {
        let mut settings = AppSettings::default();
        let request = WebSettingsRequest {
            tvdb_language: Some("deu".to_owned()),
            rename_lookup_provider: Some("tmdb".to_owned()),
            rename_template: Some("{series} - {episodeTitle}".to_owned()),
            rename_templates: Some(vec!["{title}".to_owned()]),
            rename_preview_compact_view: Some(true),
            mkv_tool_nix_directory: Some(Some("D:/tools/mkvtoolnix".to_owned())),
            ffmpeg_directory: Some(Some("D:/tools/ffmpeg".to_owned())),
            default_root: Some(Some("D:/media".to_owned())),
            ignored_scan_folder_names: Some(vec!["Trailers".to_owned()]),
            use_quick_hash_on_unreliable_timestamps: Some(true),
            max_scan_workers: Some(6),
            max_edit_workers: Some(3),
            max_remux_workers: Some(2),
            watch_folders: Some(vec!["D:/watch".to_owned()]),
            enable_live_watch_folder_monitoring: Some(true),
            watch_debounce_millis: Some(1500),
            watch_reconciliation_interval_minutes: Some(45),
            watch_force_polling: Some(true),
            selected_theme_name: Some("Midnight".to_owned()),
            custom_themes: Some(vec![mkvo_contracts::ThemeDefinition {
                name: "Midnight".to_owned(),
                colors: [("accent".to_owned(), "#5b21b6".to_owned())]
                    .into_iter()
                    .collect(),
            }]),
            ..WebSettingsRequest::default()
        };

        let mut secrets = Vec::new();
        apply_web_settings_request(&mut settings, request, &mut secrets).expect("apply");
        let view = web_settings(&settings, &[], &CompatibilitySettings::default());

        assert_eq!(view.tvdb_language, "deu");
        assert_eq!(view.rename_lookup_provider, "tmdb");
        assert_eq!(view.rename_template, "{series} - {episodeTitle}");
        assert_eq!(view.rename_templates, vec!["{title}".to_owned()]);
        assert!(view.rename_preview_compact_view);
        assert_eq!(
            view.mkv_tool_nix_directory.as_deref(),
            Some(display_path(Path::new("D:/tools/mkvtoolnix")).as_str())
        );
        assert_eq!(
            view.ffmpeg_directory.as_deref(),
            Some(display_path(Path::new("D:/tools/ffmpeg")).as_str())
        );
        assert_eq!(
            view.default_root.as_deref(),
            Some(display_path(Path::new("D:/media")).as_str())
        );
        assert_eq!(view.ignored_scan_folder_names, vec!["Trailers".to_owned()]);
        assert!(view.use_quick_hash_on_unreliable_timestamps);
        assert_eq!(view.max_scan_workers, 6);
        assert_eq!(view.max_edit_workers, 3);
        assert_eq!(view.max_remux_workers, 2);
        assert_eq!(
            view.watch_folders,
            vec![display_path(Path::new("D:/watch"))]
        );
        assert!(view.enable_live_watch_folder_monitoring);
        assert_eq!(view.watch_debounce_millis, 1500);
        assert_eq!(view.watch_reconciliation_interval_minutes, 45);
        assert!(view.watch_force_polling);
        assert_eq!(view.selected_theme_name, "Midnight");
        assert_eq!(view.custom_themes.len(), 1);
        assert_eq!(view.custom_themes[0].name, "Midnight");
        assert_eq!(
            view.custom_themes[0]
                .colors
                .get("accent")
                .map(String::as_str),
            Some("#5b21b6")
        );
    }

    /// An omitted field means "unchanged"; an explicit empty path means "clear".
    #[test]
    fn optional_paths_distinguish_unchanged_from_cleared() {
        let mut settings = AppSettings::default();
        settings.tools.ffmpeg_directory = Some(PathBuf::from("D:/tools/ffmpeg"));
        settings.scan.default_root = Some(PathBuf::from("D:/media"));
        let mut secrets = Vec::new();

        apply_web_settings_request(&mut settings, WebSettingsRequest::default(), &mut secrets)
            .expect("omitted fields are unchanged");
        assert_eq!(
            settings.tools.ffmpeg_directory,
            Some(PathBuf::from("D:/tools/ffmpeg"))
        );
        assert_eq!(settings.scan.default_root, Some(PathBuf::from("D:/media")));

        apply_web_settings_request(
            &mut settings,
            WebSettingsRequest {
                ffmpeg_directory: Some(None),
                default_root: Some(Some("   ".to_owned())),
                ..WebSettingsRequest::default()
            },
            &mut secrets,
        )
        .expect("explicit clears");
        assert_eq!(settings.tools.ffmpeg_directory, None);
        assert_eq!(settings.scan.default_root, None);
    }

    /// Worker limits come from the UI, so they must be clamped to the documented
    /// safety defaults rather than trusted.
    #[test]
    fn worker_limits_are_clamped_to_the_documented_ceilings() {
        let mut settings = AppSettings::default();
        let mut secrets = Vec::new();
        apply_web_settings_request(
            &mut settings,
            WebSettingsRequest {
                max_scan_workers: Some(0),
                max_edit_workers: Some(99),
                max_remux_workers: Some(64),
                ..WebSettingsRequest::default()
            },
            &mut secrets,
        )
        .expect("apply");

        assert_eq!(settings.workers.max_scan_workers, 1);
        assert_eq!(settings.workers.max_edit_workers, 6);
        assert_eq!(settings.workers.max_remux_workers, 2);
    }
}
