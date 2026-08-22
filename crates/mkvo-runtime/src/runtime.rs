use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use chrono::{DateTime, Utc};
use mkvo_application::{
    ApplicationError, JobSpec, JobSupervisor, JournalStatus, MediaServerClient,
    MediaServerConnection, MetadataProviderClient, ScanService, SettingsService,
};
use mkvo_contracts::{
    AppStatus, CurrentScanResponse, FileSystemEntry, FileSystemEntryKind, FileSystemResponse,
    JobCompletion, JobKind, JobSnapshot, JobStatus, LibraryArtworkRequest, LibraryArtworkResponse,
    LibraryCatalogItem, LibraryCatalogRequest, LibraryCatalogResponse, LogQuery, MediaFileRow,
    MediaServerSyncResponse, MediaServerTestResponse, ScanJobResponse, ScanRequest, SecretUpdate,
    SourceRoot, WebMediaServer, WebMediaServerLibraryPath, WebMediaServerPathMapping, WebSettings,
    WebSettingsRequest,
};
use mkvo_domain::{
    AppSettings, CredentialState, IdempotencyKey, JobId, LibraryRoot, MediaFile, MediaServerId,
    MediaServerKind, MediaServerLibrary, MediaServerSettings, MetadataProvider, PathMapping,
    PresetSettings, same_path, stable_fingerprint,
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
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

use crate::compat::{MediaServerConnectionRequest, RenameSearchResult};
use crate::{BrowseScope, RuntimeConfig, RuntimeDependencies, RuntimeError, RuntimeResult};

mod browsing_facade;
mod host_operations;
mod media_server_facade;
mod media_servers;
mod metadata_facade;
mod migration_facade;
mod recovery;
mod runtime_support;
mod scan_facade;
mod scan_state;
mod settings_facade;
use media_servers::{
    media_server_urls_equivalent, parse_server_kind, server_kind_name, validate_media_server_url,
};
use recovery::classify_recovery;
pub(crate) use runtime_support::display_path;
use scan_state::{
    CurrentScanState, current_scan_response, scan_result_state, scan_state_from_snapshot,
};

struct RuntimeInner {
    config: RuntimeConfig,
    dependencies: RuntimeDependencies,
    jobs: Arc<JobSupervisor>,
    scan: Arc<ScanService>,
    settings_service: Arc<SettingsService>,
    current_scan: Arc<RwLock<CurrentScanState>>,
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
pub struct OrphanJournalItem {
    pub idempotency_key: IdempotencyKey,
    pub plan_id: mkvo_domain::PlanId,
    pub status: JournalStatus,
    pub step: u64,
    pub disposition: RecoveryDisposition,
    pub reason: String,
    #[serde(default)]
    pub items: Vec<mkvo_application::JournalItemOutcome>,
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
    #[serde(default)]
    pub orphan_journals: Vec<OrphanJournalItem>,
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
            config.scan_worker_override.unwrap_or(4),
        ));
        let settings_service = Arc::new(SettingsService::new(
            Arc::clone(&dependencies.settings),
            Arc::clone(&dependencies.secrets),
            dependencies.watcher.clone(),
        ));
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
                settings_service,
                current_scan: Arc::new(RwLock::new(CurrentScanState::default())),
                legacy_rename_history,
            }),
        }
    }
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

fn web_settings(
    settings: &AppSettings,
    secret_status: &[mkvo_contracts::SecretStatus],
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
        audio_name_presets: settings.presets.audio_name_presets.clone(),
        subtitle_name_presets: settings.presets.subtitle_name_presets.clone(),
        language_presets: settings.presets.language_presets.clone(),
        mkv_merge_default_audio_languages: settings
            .presets
            .mkv_merge_default_audio_languages
            .clone(),
        mkv_merge_default_subtitle_languages: settings
            .presets
            .mkv_merge_default_subtitle_languages
            .clone(),
        mkv_tool_nix_directory: settings
            .tools
            .mkvtoolnix_directory
            .as_deref()
            .map(display_path),
        ffmpeg_directory: settings.tools.ffmpeg_directory.as_deref().map(display_path),
        default_root: settings.scan.default_root.as_deref().map(display_path),
        default_root_name: settings.scan.default_root_name.clone(),
        library_roots: settings
            .scan
            .library_roots
            .iter()
            .map(|root| SourceRoot {
                name: root.name.clone(),
                path: display_path(&root.path),
            })
            .collect(),
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
    if let Some(values) = request.audio_name_presets {
        settings.presets.audio_name_presets = normalized_strings(values);
    }
    if let Some(values) = request.subtitle_name_presets {
        settings.presets.subtitle_name_presets = normalized_strings(values);
    }
    if let Some(values) = request.language_presets {
        settings.presets.language_presets = normalized_strings(values);
    }
    if let Some(value) = request
        .mkv_merge_default_audio_languages
        .filter(|value| !value.trim().is_empty())
    {
        settings.presets.mkv_merge_default_audio_languages = value.trim().to_owned();
    }
    if let Some(value) = request
        .mkv_merge_default_subtitle_languages
        .filter(|value| !value.trim().is_empty())
    {
        settings.presets.mkv_merge_default_subtitle_languages = value.trim().to_owned();
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
    if let Some(value) = request.default_root_name {
        settings.scan.default_root_name = value.trim().to_owned();
        if settings.scan.default_root_name.is_empty() {
            settings.scan.default_root_name = "Home".to_owned();
        }
    }
    if let Some(values) = request.library_roots {
        // Blank rows are how a half-finished edit arrives from the form; they
        // are dropped rather than saved as unnamed roots.
        settings.scan.library_roots = values
            .into_iter()
            .filter_map(|root| {
                let name = root.name.trim().to_owned();
                let path = root.path.trim();
                (!name.is_empty() && !path.is_empty()).then(|| LibraryRoot {
                    name,
                    path: PathBuf::from(path),
                })
            })
            .collect();
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
        // Removing a server must remove both current and legacy credential
        // aliases; otherwise its API key remains recoverable indefinitely.
        for removed_id in existing
            .keys()
            .filter(|id| !updated.iter().any(|server| server.id == **id))
        {
            for key in [
                format!("media_server.{removed_id}.api_key"),
                format!("mediaServer:{removed_id}"),
            ] {
                secrets.push(SecretUpdate {
                    key,
                    clear: true,
                    value: None,
                });
            }
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
    let mut seen_libraries = BTreeSet::new();
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
            .filter(|library| {
                let path = library
                    .local_path
                    .as_deref()
                    .unwrap_or(&library.server_path)
                    .to_string_lossy()
                    .replace('\\', "/");
                seen_libraries.insert(path.trim_end_matches('/').to_ascii_lowercase())
            })
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

fn resolve_media_server_local_paths(libraries: &mut [MediaServerLibrary], media_root: &Path) {
    for library in libraries {
        if library.local_path.as_deref().is_some_and(Path::is_dir) {
            continue;
        }

        if library.server_path.is_dir() {
            library.local_path = Some(library.server_path.clone());
            continue;
        }

        let Some(folder_name) = library.server_path.file_name() else {
            continue;
        };
        let mounted_path = media_root.join(folder_name);
        if mounted_path.is_dir() {
            library.local_path = Some(mounted_path);
        }
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
    // The provider's media kind survives as a prefix on the id, which is how
    // the episode lookup knows to ask for a film. Reporting every result as a
    // series threw that away at the last step, so a film was renamed as though
    // it had a season and an episode.
    let format = if value.id.starts_with("movie:") {
        "movie"
    } else {
        "series"
    };
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
        format: format.to_owned(),
        database_url: String::new(),
        provider_display: provider.to_ascii_uppercase(),
    }
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
mod library_root_tests {
    use super::*;
    use crate::MkvoRuntimeBuilder;

    #[test]
    fn media_server_path_falls_back_to_matching_mounted_folder() {
        let directory = tempfile::tempdir().unwrap();
        let media_root = directory.path().join("media");
        let mounted_anime = media_root.join("anime");
        std::fs::create_dir_all(&mounted_anime).unwrap();
        let mut libraries = vec![MediaServerLibrary {
            id: "anime".to_owned(),
            name: "Anime".to_owned(),
            media_type: Some("tvshows".to_owned()),
            server_path: PathBuf::from("/mnt/user/anime"),
            local_path: None,
            enabled: true,
        }];

        resolve_media_server_local_paths(&mut libraries, &media_root);

        assert_eq!(
            libraries[0].local_path.as_deref(),
            Some(mounted_anime.as_path())
        );
    }

    struct Host {
        _directory: tempfile::TempDir,
        mount: PathBuf,
        outside: PathBuf,
        config: PathBuf,
        runtime: MkvoRuntime,
    }

    /// `confined` mirrors the container: one mount, browsing limited to it.
    /// Otherwise the desktop: no granted roots, browsing unrestricted.
    fn host(confined: bool) -> Host {
        let directory = tempfile::tempdir().unwrap();
        let mount = directory.path().join("mnt/user");
        let outside = directory.path().join("etc");
        let config = directory.path().join("config");
        for path in [&mount, &outside, &config] {
            std::fs::create_dir_all(path).unwrap();
        }
        std::fs::create_dir_all(mount.join("anime")).unwrap();
        std::fs::create_dir_all(mount.join("tv")).unwrap();

        // Unit tests must never probe the real user's platform application-data
        // directory. A developer's legacy settings can otherwise be imported
        // into this temporary database and leak UNC roots or presets into the
        // assertions below.
        let mut builder = MkvoRuntimeBuilder::new(&mount, &config)
            .app_name("test")
            .disable_legacy_migration();
        builder = if confined {
            builder.authorized_root(&mount, true)
        } else {
            builder.unrestricted_browsing()
        };

        Host {
            _directory: directory,
            mount,
            outside,
            config,
            runtime: builder.build().unwrap(),
        }
    }

    fn root(name: &str, path: &Path) -> SourceRoot {
        SourceRoot {
            name: name.to_owned(),
            path: display_path(path),
        }
    }

    #[tokio::test]
    async fn legacy_web_presets_migrate_once_into_app_settings() {
        let host = host(false);
        let legacy = PresetSettings {
            audio_name_presets: vec!["Legacy Audio".to_owned()],
            subtitle_name_presets: vec!["Legacy Subtitle".to_owned()],
            language_presets: vec!["ita".to_owned()],
            mkv_merge_default_audio_languages: "ita".to_owned(),
            mkv_merge_default_subtitle_languages: "ita,eng".to_owned(),
        };
        std::fs::write(
            host.config.join("web-settings-extra.json"),
            serde_json::to_vec_pretty(&legacy).unwrap(),
        )
        .unwrap();

        host.runtime.migrate_legacy_data().await.expect("migrate");
        let migrated = host.runtime.get_web_settings().await.expect("settings");
        assert_eq!(migrated.audio_name_presets, ["Legacy Audio"]);
        assert_eq!(migrated.subtitle_name_presets, ["Legacy Subtitle"]);
        assert_eq!(migrated.language_presets, ["ita"]);
        assert_eq!(migrated.mkv_merge_default_audio_languages, "ita");
        assert_eq!(migrated.mkv_merge_default_subtitle_languages, "ita,eng");
        assert!(!host.config.join("web-settings-extra.json").exists());
        assert!(
            host.config
                .join("web-settings-extra.migrated.json")
                .exists()
        );

        host.runtime
            .save_web_settings(WebSettingsRequest {
                audio_name_presets: Some(vec!["Current Audio".to_owned()]),
                ..WebSettingsRequest::default()
            })
            .await
            .expect("save current settings");
        std::fs::write(
            host.config.join("web-settings-extra.json"),
            serde_json::to_vec_pretty(&legacy).unwrap(),
        )
        .unwrap();
        host.runtime
            .migrate_legacy_data()
            .await
            .expect("migrate again");
        let current = host.runtime.get_web_settings().await.expect("settings");
        assert_eq!(current.audio_name_presets, ["Current Audio"]);
    }

    #[tokio::test]
    async fn startup_recovery_reports_orphan_journals_per_item() {
        let host = host(false);
        let record = mkvo_application::JournalRecord {
            idempotency_key: IdempotencyKey::parse("orphan-journal").unwrap(),
            plan_id: mkvo_domain::PlanId::new(),
            step: 1,
            status: JournalStatus::Running,
            resources: Vec::new(),
            items: vec![mkvo_application::JournalItemOutcome {
                key: "episode.mkv".to_owned(),
                status: mkvo_application::JournalItemStatus::Completed,
                detail: None,
            }],
            detail: Some("process stopped after mutation".to_owned()),
            updated_utc: Utc::now(),
        };
        host.runtime
            .inner
            .dependencies
            .journal
            .begin(&record)
            .await
            .unwrap();

        let report = host
            .runtime
            .classify_startup_recovery()
            .await
            .expect("recovery report");
        assert!(report.journal_enumeration_supported);
        assert_eq!(report.manual_review, 1);
        assert_eq!(report.orphan_journals.len(), 1);
        assert_eq!(report.orphan_journals[0].items, record.items);
    }

    /// The whole point of the setting for a container user: several shares
    /// inside the one mount they bound in.
    #[tokio::test]
    async fn a_confined_host_accepts_folders_inside_its_mount() {
        let host = host(true);
        let anime = host.mount.join("anime");
        let tv = host.mount.join("tv");

        let saved = host
            .runtime
            .save_web_settings(WebSettingsRequest {
                library_roots: Some(vec![root("Anime", &anime), root("TV", &tv)]),
                ..WebSettingsRequest::default()
            })
            .await
            .expect("save");

        let names: Vec<&str> = saved
            .library_roots
            .iter()
            .map(|entry| entry.name.as_str())
            .collect();
        assert_eq!(names, ["Anime", "TV"]);
    }

    /// Saving a library folder grants it, so without this check the setting
    /// would be an authorization bypass: name `/etc`, save, and the runtime
    /// authorizes it for you.
    #[tokio::test]
    async fn a_confined_host_refuses_a_folder_outside_its_roots() {
        let host = host(true);

        let error = host
            .runtime
            .save_web_settings(WebSettingsRequest {
                library_roots: Some(vec![root("Escape", &host.outside)]),
                ..WebSettingsRequest::default()
            })
            .await
            .expect_err("a path outside the mount must be refused");

        assert!(
            format!("{error}").contains("unauthorized"),
            "expected an authorization failure, got: {error}"
        );

        // The refusal has to be total: nothing may be persisted on the way out.
        let settings = host.runtime.get_web_settings().await.expect("settings");
        assert!(settings.library_roots.is_empty());
    }

    /// The desktop user already reaches every path through their own file
    /// manager, so there is no boundary for the setting to breach.
    #[tokio::test]
    async fn an_unrestricted_host_accepts_any_real_folder() {
        let host = host(false);

        host.runtime
            .save_web_settings(WebSettingsRequest {
                library_roots: Some(vec![root("Elsewhere", &host.outside)]),
                ..WebSettingsRequest::default()
            })
            .await
            .expect("an unrestricted host may name any folder");
    }

    /// The placeholder root is an implementation detail. Advertising it would
    /// reintroduce the hardcoded library under another name.
    #[tokio::test]
    async fn a_desktop_offers_no_shortcuts_before_a_library_is_named() {
        let host = host(false);

        let status = host.runtime.get_status().await.expect("status");
        assert!(status.media_root.is_empty(), "{}", status.media_root);
        assert!(status.source_roots.is_empty(), "{:?}", status.source_roots);
    }

    /// The container was launched pointing at a mount, so that root is real and
    /// stays on offer.
    #[tokio::test]
    async fn a_confined_host_keeps_its_mount_as_a_shortcut() {
        let host = host(true);

        let status = host.runtime.get_status().await.expect("status");
        assert!(same_path(Path::new(&status.media_root), &host.mount));
        assert!(
            status
                .source_roots
                .iter()
                .any(|entry| same_path(Path::new(&entry.path), &host.mount)),
            "{:?}",
            status.source_roots
        );
    }

    #[tokio::test]
    async fn a_folder_that_does_not_exist_is_refused() {
        let host = host(false);
        let missing = host.outside.join("nope");

        let error = host
            .runtime
            .save_web_settings(WebSettingsRequest {
                library_roots: Some(vec![root("Missing", &missing)]),
                ..WebSettingsRequest::default()
            })
            .await
            .expect_err("a missing folder must be refused");

        assert!(format!("{error}").contains("not a directory"), "{error}");
    }

    /// Replaces the hardcoded Videos folder: with nothing configured the
    /// desktop has no library, and naming one makes it the starting point.
    #[tokio::test]
    async fn the_desktop_media_root_follows_the_first_library_folder() {
        let host = host(false);
        let anime = host.mount.join("anime");

        host.runtime
            .save_web_settings(WebSettingsRequest {
                library_roots: Some(vec![root("Anime", &anime)]),
                ..WebSettingsRequest::default()
            })
            .await
            .expect("save");

        let status = host.runtime.get_status().await.expect("status");
        assert!(same_path(Path::new(&status.media_root), &anime));
        assert!(
            status
                .source_roots
                .iter()
                .any(|entry| entry.name == "Anime"),
            "the library folder should be offered as a shortcut"
        );
    }

    /// Home is independent from shortcuts: once configured it remains the
    /// browser and scan starting point even when Quick Access has other roots.
    #[tokio::test]
    async fn the_desktop_media_root_prefers_the_default_directory() {
        let host = host(false);
        let home = host.mount.join("home");
        let anime = host.mount.join("anime");
        std::fs::create_dir_all(&home).expect("home directory");

        host.runtime
            .save_web_settings(WebSettingsRequest {
                default_root: Some(Some(display_path(&home))),
                library_roots: Some(vec![root("Anime", &anime)]),
                ..WebSettingsRequest::default()
            })
            .await
            .expect("save");

        let status = host.runtime.get_status().await.expect("status");
        assert!(same_path(Path::new(&status.media_root), &home));
        assert!(
            status
                .source_roots
                .iter()
                .any(|entry| entry.name == "Anime")
        );
    }

    /// A saved folder has to be usable immediately; requiring a restart to
    /// authorize it is the bug this mirrors for watch folders.
    #[tokio::test]
    async fn a_saved_folder_is_browsable_without_a_restart() {
        let host = host(false);
        let anime = host.mount.join("anime");
        std::fs::write(anime.join("Ep01.mkv"), b"x").unwrap();

        host.runtime
            .save_web_settings(WebSettingsRequest {
                library_roots: Some(vec![root("Anime", &anime)]),
                ..WebSettingsRequest::default()
            })
            .await
            .expect("save");

        let listing = host
            .runtime
            .browse_file_system(None)
            .await
            .expect("browsing with no path should open the library folder");
        assert!(same_path(Path::new(&listing.path), &anime));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_server_urls_are_compared_without_cosmetic_slashes() {
        assert!(media_server_urls_equivalent(
            "https://Media.Example:443/jellyfin/",
            "https://media.example/jellyfin"
        ));
        assert!(!media_server_urls_equivalent(
            "https://media.example/jellyfin",
            "https://attacker.example/jellyfin"
        ));
    }

    #[test]
    fn media_server_urls_reject_embedded_credentials_and_queries() {
        assert!(validate_media_server_url("http://media.example").is_ok());
        assert!(validate_media_server_url("http://user:secret@media.example").is_err());
        assert!(validate_media_server_url("http://media.example?token=secret").is_err());
    }

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

    #[test]
    fn initial_scan_selects_every_file() {
        let mut state = CurrentScanState::default();

        state.apply_scan(
            vec![media_at("/media/a.mkv"), media_at("/media/b.mkv")],
            mkvo_contracts::ScanSummary::default(),
        );

        assert_eq!(
            state.selected_display_paths(),
            vec!["/media/a.mkv".to_owned(), "/media/b.mkv".to_owned()]
        );
    }

    #[test]
    fn rescan_preserves_a_deliberate_empty_selection() {
        let mut state = CurrentScanState {
            files: vec![media_at("/media/a.mkv")],
            ..CurrentScanState::default()
        };

        state.apply_scan(
            vec![media_at("/media/a.mkv"), media_at("/media/b.mkv")],
            mkvo_contracts::ScanSummary::default(),
        );

        assert!(state.selected_display_paths().is_empty());
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

    pub(super) fn media_at(path: &str) -> MediaFile {
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
            audio_name_presets: Some(vec!["Original Audio".to_owned()]),
            subtitle_name_presets: Some(vec!["Original Subtitle".to_owned()]),
            language_presets: Some(vec!["ita".to_owned()]),
            mkv_merge_default_audio_languages: Some("ita,jpn".to_owned()),
            mkv_merge_default_subtitle_languages: Some("ita".to_owned()),
            rename_preview_compact_view: Some(true),
            mkv_tool_nix_directory: Some(Some("D:/tools/mkvtoolnix".to_owned())),
            ffmpeg_directory: Some(Some("D:/tools/ffmpeg".to_owned())),
            default_root: Some(Some("D:/media".to_owned())),
            default_root_name: Some("Media".to_owned()),
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
        let view = web_settings(&settings, &[]);

        assert_eq!(view.tvdb_language, "deu");
        assert_eq!(view.rename_lookup_provider, "tmdb");
        assert_eq!(view.rename_template, "{series} - {episodeTitle}");
        assert_eq!(view.rename_templates, vec!["{title}".to_owned()]);
        assert_eq!(view.audio_name_presets, ["Original Audio"]);
        assert_eq!(view.subtitle_name_presets, ["Original Subtitle"]);
        assert_eq!(view.language_presets, ["ita"]);
        assert_eq!(view.mkv_merge_default_audio_languages, "ita,jpn");
        assert_eq!(view.mkv_merge_default_subtitle_languages, "ita");
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
        assert_eq!(view.default_root_name, "Media");
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

    #[test]
    fn removing_a_media_server_clears_all_credential_aliases() {
        let id = MediaServerId::default();
        let mut settings = AppSettings::default();
        settings.media_servers.push(MediaServerSettings {
            id,
            name: "Living room".to_owned(),
            kind: MediaServerKind::Jellyfin,
            server_url: "http://media.local".to_owned(),
            credential: CredentialState {
                configured: true,
                masked_hint: None,
                secret_reference: Some(format!("media_server.{id}.api_key")),
            },
            is_default: true,
            libraries: Vec::new(),
            last_synced_at: None,
        });
        let mut secrets = Vec::new();

        apply_web_settings_request(
            &mut settings,
            WebSettingsRequest {
                media_servers: Some(Vec::new()),
                ..WebSettingsRequest::default()
            },
            &mut secrets,
        )
        .expect("remove server");

        assert!(settings.media_servers.is_empty());
        assert_eq!(secrets.len(), 2);
        assert!(secrets.iter().all(|secret| secret.clear));
        assert!(
            secrets
                .iter()
                .any(|secret| secret.key == format!("media_server.{id}.api_key"))
        );
        assert!(
            secrets
                .iter()
                .any(|secret| secret.key == format!("mediaServer:{id}"))
        );
    }
}

#[cfg(test)]
mod working_set_rename_tests {
    use super::tests::media_at;
    use super::*;

    fn state_with(paths: &[&str], selected: &[&str]) -> CurrentScanState {
        CurrentScanState {
            files: paths.iter().map(|path| media_at(path)).collect(),
            selected: selected
                .iter()
                .map(|path| mkvo_application::paths::path_key(Path::new(path)))
                .collect(),
            ..CurrentScanState::default()
        }
    }

    /// The working set is what later operations run against. Leaving it on the
    /// old paths means the next operation resolves nothing, and the dashboard
    /// keeps showing names that are gone until a rescan.
    #[test]
    fn the_working_set_moves_with_a_renamed_file() {
        let mut state = state_with(&[r"C:\media\old.mkv", r"C:\media\other.mkv"], &[]);

        state.apply_renames(&[(
            PathBuf::from(r"C:\media\old.mkv"),
            PathBuf::from(r"C:\media\new.mkv"),
        )]);

        let paths: Vec<String> = state
            .files
            .iter()
            .map(|file| file.path.display().to_string())
            .collect();
        assert!(
            paths.iter().any(|path| path.ends_with("new.mkv")),
            "{paths:?}"
        );
        assert!(
            !paths.iter().any(|path| path.ends_with("old.mkv")),
            "{paths:?}"
        );
    }

    /// A renamed file is still the file the user picked, so the selection
    /// follows it rather than being silently dropped.
    #[test]
    fn a_selection_follows_the_rename() {
        let mut state = state_with(&[r"C:\media\old.mkv"], &[r"C:\media\old.mkv"]);

        state.apply_renames(&[(
            PathBuf::from(r"C:\media\old.mkv"),
            PathBuf::from(r"C:\media\new.mkv"),
        )]);

        assert_eq!(
            state.selected_display_paths(),
            vec![display_path(Path::new(r"C:\media\new.mkv"))]
        );
    }

    /// Renaming one file must not disturb the selection of another.
    #[test]
    fn an_unrelated_selection_is_untouched() {
        let mut state = state_with(
            &[r"C:\media\old.mkv", r"C:\media\keep.mkv"],
            &[r"C:\media\keep.mkv"],
        );

        state.apply_renames(&[(
            PathBuf::from(r"C:\media\old.mkv"),
            PathBuf::from(r"C:\media\new.mkv"),
        )]);

        assert_eq!(
            state.selected_display_paths(),
            vec![display_path(Path::new(r"C:\media\keep.mkv"))]
        );
    }
}

#[cfg(test)]
mod search_result_format_tests {
    use super::*;

    fn provider_result(id: &str) -> mkvo_domain::ProviderSearchResult {
        mkvo_domain::ProviderSearchResult {
            provider: mkvo_domain::MetadataProvider::Tmdb,
            id: id.to_owned(),
            title: "Obsession".to_owned(),
            year: Some(2026),
            overview: None,
        }
    }

    /// Every result was reported as a series, which is what made a film rename
    /// as though it had a season and an episode.
    #[test]
    fn a_film_is_reported_as_a_film() {
        let result = rename_search_result(provider_result("movie:12345"));

        assert_eq!(result.format, "movie");
        // The prefix has to survive: the episode lookup reads it back to know
        // which endpoint to ask.
        assert_eq!(result.media_id(), "movie:12345");
    }

    #[test]
    fn a_series_is_still_reported_as_a_series() {
        let result = rename_search_result(provider_result("12345"));

        assert_eq!(result.format, "series");
        assert_eq!(result.media_id(), "12345");
    }
}
