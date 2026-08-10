use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use mkvo_application::{
    AuthorizedPathPolicy, FileSystem, JobRepository, MediaCatalog, MediaProbe, MetadataCache,
    OperationJournal, OperationLog, PlanRepository, RenameHistoryRepository, SecretStore,
    SettingsRepository, ToolExecutor, ToolRegistry, WatchBackend,
};
use mkvo_domain::AppSettings;
use mkvo_infra_process::{
    MediaScanAdapter, ProcessRunner, ProcessToolExecutor, ToolKind,
    ToolRegistry as ProcessToolRegistry,
};
use mkvo_infra_sqlite::{LegacyImportOutcome, SqliteRepositories, import_legacy_settings};
use mkvo_infra_watch::{
    AuthorizedRoot, AuthorizedRoots, LocalFileSystem, LocalMediaCatalog, WatchOptions, WatchService,
};
use tokio::sync::RwLock;

use crate::{MkvoRuntime, RuntimeError, RuntimeResult};

#[derive(Clone, Debug)]
pub struct RuntimeConfig {
    pub app_name: String,
    pub version: String,
    pub media_root: PathBuf,
    pub config_root: PathBuf,
    pub source_roots: Vec<(String, PathBuf)>,
    pub authorized_roots: Vec<(PathBuf, bool)>,
    pub tool_directories: Vec<PathBuf>,
    pub scan_worker_override: Option<usize>,
    pub edit_worker_override: Option<usize>,
    pub remux_worker_override: Option<usize>,
    /// Explicit .NET settings source. When absent, the runtime probes the
    /// configured directory and the legacy platform application-data folder.
    pub legacy_settings_path: Option<PathBuf>,
    /// Explicit .NET rename-history source. Legacy history is exposed read-only.
    pub legacy_rename_history_path: Option<PathBuf>,
    pub legacy_migration_enabled: bool,
    /// How far the file browser may look.
    ///
    /// The desktop user owns the machine, and confining the browser there buys
    /// nothing they could not do in Explorer while making it impossible to
    /// reach a library outside the configured roots. A network service is the
    /// opposite case, so the server keeps the browser inside its roots.
    /// Browsing is read-only either way: mutations always re-check
    /// authorization, whatever the browser was allowed to show.
    pub browse_scope: BrowseScope,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BrowseScope {
    /// List only inside authorized roots. The default, so a host that forgets
    /// to choose is confined rather than open.
    #[default]
    AuthorizedRootsOnly,
    /// List anywhere the process can read, including volume roots.
    Unrestricted,
}

impl RuntimeConfig {
    #[must_use]
    pub fn new(media_root: impl Into<PathBuf>, config_root: impl Into<PathBuf>) -> Self {
        let media_root = media_root.into();
        Self {
            app_name: "MKV Orchestrator".to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            source_roots: vec![("Media".to_owned(), media_root.clone())],
            authorized_roots: vec![(media_root.clone(), true)],
            tool_directories: Vec::new(),
            scan_worker_override: None,
            edit_worker_override: None,
            remux_worker_override: None,
            media_root,
            config_root: config_root.into(),
            legacy_settings_path: None,
            legacy_rename_history_path: None,
            legacy_migration_enabled: true,
            browse_scope: BrowseScope::AuthorizedRootsOnly,
        }
    }

    pub(crate) fn resolved_legacy_settings_path(&self) -> Option<PathBuf> {
        self.resolve_legacy_path(self.legacy_settings_path.as_ref(), "settings.json")
    }

    pub(crate) fn resolved_legacy_rename_history_path(&self) -> Option<PathBuf> {
        self.resolve_legacy_path(
            self.legacy_rename_history_path.as_ref(),
            "rename-batches.json",
        )
    }

    fn resolve_legacy_path(&self, explicit: Option<&PathBuf>, file_name: &str) -> Option<PathBuf> {
        if !self.legacy_migration_enabled {
            return None;
        }
        if let Some(path) = explicit {
            return Some(path.clone());
        }

        let colocated = self.config_root.join(file_name);
        if colocated.is_file() {
            return Some(colocated);
        }
        legacy_app_data_root()
            .map(|root| root.join(file_name))
            .filter(|path| path.is_file())
            .or(Some(colocated))
    }
}

/// Port bundle used by tests and by hosts that need custom persistence.
#[derive(Clone)]
pub struct RuntimeDependencies {
    pub catalog: Arc<dyn MediaCatalog>,
    pub probe: Arc<dyn MediaProbe>,
    pub cache: Arc<dyn MetadataCache>,
    pub paths: Arc<dyn AuthorizedPathPolicy>,
    pub file_system: Arc<dyn FileSystem>,
    pub tools: Arc<dyn ToolRegistry>,
    pub tool_executor: Arc<dyn ToolExecutor>,
    pub settings: Arc<dyn SettingsRepository>,
    pub secrets: Arc<dyn SecretStore>,
    pub plans: Arc<dyn PlanRepository>,
    pub jobs: Arc<dyn JobRepository>,
    pub rename_history: Arc<dyn RenameHistoryRepository>,
    pub journal: Arc<dyn OperationJournal>,
    pub logs: Arc<dyn OperationLog>,
    pub watcher: Option<Arc<dyn WatchBackend>>,
    pub process_tools: Option<ProcessToolRegistry>,
    pub authorized_roots: Option<AuthorizedRoots>,
}

/// Secret storage backed by the operating system credential facility.
///
/// This is the desktop default: Windows Credential Manager, macOS Keychain, or
/// the Linux Secret Service. Container hosts keep [`FileSecretStore`] because
/// no such facility exists there.
///
/// Keyring calls block, so each one runs on a blocking thread rather than
/// stalling the async runtime.
pub struct KeyringSecretStore {
    service: String,
}

impl KeyringSecretStore {
    #[must_use]
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }

    /// Verify the platform credential store actually works before committing to
    /// it. A Linux desktop without a running Secret Service, or a locked
    /// keychain, fails here — and silently losing the user's API keys at save
    /// time would be worse than using the file store from the start.
    pub fn is_usable(&self) -> bool {
        let probe = "mkvo.keyring.probe";
        let Ok(entry) = keyring::Entry::new(&self.service, probe) else {
            return false;
        };
        if entry.set_password("probe").is_err() {
            return false;
        }
        let readable = entry.get_password().is_ok_and(|value| value == "probe");
        let _ = entry.delete_credential();
        readable
    }

    fn entry(&self, key: &str) -> Result<keyring::Entry, mkvo_application::PortError> {
        keyring::Entry::new(&self.service, key).map_err(|error| {
            mkvo_application::PortError::Other(format!("credential store unavailable: {error}"))
        })
    }
}

#[async_trait]
impl SecretStore for KeyringSecretStore {
    async fn get(&self, key: &str) -> Result<Option<String>, mkvo_application::PortError> {
        let entry = self.entry(key)?;
        tokio::task::spawn_blocking(move || match entry.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(mkvo_application::PortError::Other(format!(
                "credential could not be read: {error}"
            ))),
        })
        .await
        .map_err(|error| {
            mkvo_application::PortError::Other(format!("credential read task failed: {error}"))
        })?
    }

    async fn set(&self, key: &str, value: &str) -> Result<(), mkvo_application::PortError> {
        let entry = self.entry(key)?;
        let value = value.to_owned();
        tokio::task::spawn_blocking(move || {
            entry.set_password(&value).map_err(|error| {
                mkvo_application::PortError::Other(format!(
                    "credential could not be stored: {error}"
                ))
            })
        })
        .await
        .map_err(|error| {
            mkvo_application::PortError::Other(format!("credential write task failed: {error}"))
        })?
    }

    async fn remove(&self, key: &str) -> Result<(), mkvo_application::PortError> {
        let entry = self.entry(key)?;
        tokio::task::spawn_blocking(move || match entry.delete_credential() {
            // Removing an absent secret is the desired end state, not a failure.
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(mkvo_application::PortError::Other(format!(
                "credential could not be removed: {error}"
            ))),
        })
        .await
        .map_err(|error| {
            mkvo_application::PortError::Other(format!("credential delete task failed: {error}"))
        })?
    }
}

/// Process-local secret storage intended for tests and development hosts.
/// Production hosts should inject an OS-keychain or protected-file adapter.
#[derive(Default)]
pub struct MemorySecretStore {
    values: RwLock<BTreeMap<String, String>>,
}

/// Durable secret storage for headless/container hosts.
///
/// Values are never returned by a runtime response. The host must place the
/// config directory on a protected volume; Unix files are restricted to mode
/// 0600. Desktop hosts may inject an OS-keychain implementation instead.
pub struct FileSecretStore {
    path: PathBuf,
    values: RwLock<BTreeMap<String, String>>,
    persist_lock: tokio::sync::Mutex<()>,
}

/// Read-through non-persisted secret overrides layered over a durable store.
/// Writes only reach the fallback and cannot alter the override map.
pub struct LayeredSecretStore {
    overrides: BTreeMap<String, String>,
    fallback: Arc<dyn SecretStore>,
}

impl LayeredSecretStore {
    #[must_use]
    pub fn new(overrides: BTreeMap<String, String>, fallback: Arc<dyn SecretStore>) -> Self {
        Self {
            overrides,
            fallback,
        }
    }
}

impl FileSecretStore {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, std::io::Error> {
        let path = path.into();
        let values = read_secret_values(&path)?;
        Ok(Self::from_values(path, values))
    }

    fn from_values(path: PathBuf, values: BTreeMap<String, String>) -> Self {
        Self {
            path,
            values: RwLock::new(values),
            persist_lock: tokio::sync::Mutex::new(()),
        }
    }

    async fn persist(
        &self,
        values: &BTreeMap<String, String>,
    ) -> Result<(), mkvo_application::PortError> {
        let path = self.path.clone();
        let values = values.clone();
        tokio::task::spawn_blocking(move || persist_secret_values(&path, &values))
            .await
            .map_err(|error| {
                mkvo_application::PortError::Other(format!(
                    "secret persistence task failed: {error}"
                ))
            })?
            .map_err(|error| mkvo_application::PortError::Other(error.to_string()))
    }
}

impl MemorySecretStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl SecretStore for MemorySecretStore {
    async fn get(&self, key: &str) -> Result<Option<String>, mkvo_application::PortError> {
        Ok(self.values.read().await.get(key).cloned())
    }

    async fn set(&self, key: &str, value: &str) -> Result<(), mkvo_application::PortError> {
        self.values
            .write()
            .await
            .insert(key.to_owned(), value.to_owned());
        Ok(())
    }

    async fn remove(&self, key: &str) -> Result<(), mkvo_application::PortError> {
        self.values.write().await.remove(key);
        Ok(())
    }
}

#[async_trait]
impl SecretStore for FileSecretStore {
    async fn get(&self, key: &str) -> Result<Option<String>, mkvo_application::PortError> {
        Ok(self.values.read().await.get(key).cloned())
    }

    async fn set(&self, key: &str, value: &str) -> Result<(), mkvo_application::PortError> {
        let _persist = self.persist_lock.lock().await;
        let mut values = self.values.read().await.clone();
        values.insert(key.to_owned(), value.to_owned());
        self.persist(&values).await?;
        *self.values.write().await = values;
        Ok(())
    }

    async fn remove(&self, key: &str) -> Result<(), mkvo_application::PortError> {
        let _persist = self.persist_lock.lock().await;
        let mut values = self.values.read().await.clone();
        values.remove(key);
        self.persist(&values).await?;
        *self.values.write().await = values;
        Ok(())
    }
}

#[async_trait]
impl SecretStore for LayeredSecretStore {
    async fn get(&self, key: &str) -> Result<Option<String>, mkvo_application::PortError> {
        Ok(self
            .overrides
            .get(key)
            .cloned()
            .or(self.fallback.get(key).await?))
    }

    async fn set(&self, key: &str, value: &str) -> Result<(), mkvo_application::PortError> {
        self.fallback.set(key, value).await
    }

    async fn remove(&self, key: &str) -> Result<(), mkvo_application::PortError> {
        self.fallback.remove(key).await
    }
}

pub struct MkvoRuntimeBuilder {
    config: RuntimeConfig,
    dependencies: Option<RuntimeDependencies>,
    secret_store: Option<Arc<dyn SecretStore>>,
    secret_overrides: BTreeMap<String, String>,
}

impl MkvoRuntimeBuilder {
    #[must_use]
    pub fn new(media_root: impl Into<PathBuf>, config_root: impl Into<PathBuf>) -> Self {
        Self {
            config: RuntimeConfig::new(media_root, config_root),
            dependencies: None,
            secret_store: None,
            secret_overrides: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn app_name(mut self, value: impl Into<String>) -> Self {
        self.config.app_name = value.into();
        self
    }

    #[must_use]
    pub fn version(mut self, value: impl Into<String>) -> Self {
        self.config.version = value.into();
        self
    }

    #[must_use]
    pub fn media_root(mut self, path: impl Into<PathBuf>) -> Self {
        self.config.media_root = path.into();
        self
    }

    #[must_use]
    pub fn config_root(mut self, path: impl Into<PathBuf>) -> Self {
        self.config.config_root = path.into();
        self
    }

    #[must_use]
    pub fn source_root(mut self, name: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        self.config.source_roots.push((name.into(), path.into()));
        self
    }

    #[must_use]
    pub fn authorized_root(mut self, path: impl Into<PathBuf>, writable: bool) -> Self {
        self.config.authorized_roots.push((path.into(), writable));
        self
    }

    /// Let the file browser list outside the authorized roots.
    ///
    /// For hosts where the user already has full filesystem access, such as the
    /// desktop app. Mutations are unaffected and still require authorization.
    #[must_use]
    pub fn unrestricted_browsing(mut self) -> Self {
        self.config.browse_scope = BrowseScope::Unrestricted;
        self
    }

    #[must_use]
    pub fn tool_directory(mut self, path: impl Into<PathBuf>) -> Self {
        self.config.tool_directories.push(path.into());
        self
    }

    #[must_use]
    pub fn secret_override(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        let value = value.into();
        if !value.is_empty() {
            self.secret_overrides.insert(key.into(), value);
        }
        self
    }

    #[must_use]
    pub fn environment_secret_overrides(mut self) -> Self {
        for (variable, key) in [
            ("MKVO_TVDB_API_KEY", "provider.tvdb.api_key"),
            ("MKVO_TVDB_PIN", "provider.tvdb.pin"),
            ("MKVO_TMDB_API_KEY", "provider.tmdb.api_key"),
        ] {
            if let Ok(value) = std::env::var(variable)
                && !value.is_empty()
            {
                self.secret_overrides.insert(key.to_owned(), value);
            }
        }
        self
    }

    #[must_use]
    pub fn worker_limits(mut self, scan: usize, edit: usize, remux: usize) -> Self {
        self.config.scan_worker_override = Some(scan.clamp(1, 8));
        self.config.edit_worker_override = Some(edit.clamp(1, 6));
        self.config.remux_worker_override = Some(remux.clamp(1, 2));
        self
    }

    #[must_use]
    pub fn scan_worker_limit(mut self, workers: usize) -> Self {
        self.config.scan_worker_override = Some(workers.clamp(1, 8));
        self
    }

    #[must_use]
    pub fn edit_worker_limit(mut self, workers: usize) -> Self {
        self.config.edit_worker_override = Some(workers.clamp(1, 6));
        self
    }

    #[must_use]
    pub fn remux_worker_limit(mut self, workers: usize) -> Self {
        self.config.remux_worker_override = Some(workers.clamp(1, 2));
        self
    }

    #[must_use]
    pub fn secret_store(mut self, store: Arc<dyn SecretStore>) -> Self {
        self.secret_store = Some(store);
        self
    }

    #[must_use]
    pub fn legacy_data_root(mut self, path: impl Into<PathBuf>) -> Self {
        let root = path.into();
        self.config.legacy_settings_path = Some(root.join("settings.json"));
        self.config.legacy_rename_history_path = Some(root.join("rename-batches.json"));
        self.config.legacy_migration_enabled = true;
        self
    }

    #[must_use]
    pub fn legacy_settings_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.config.legacy_settings_path = Some(path.into());
        self.config.legacy_migration_enabled = true;
        self
    }

    #[must_use]
    pub fn legacy_rename_history_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.config.legacy_rename_history_path = Some(path.into());
        self.config.legacy_migration_enabled = true;
        self
    }

    #[must_use]
    pub fn disable_legacy_migration(mut self) -> Self {
        self.config.legacy_migration_enabled = false;
        self
    }

    #[must_use]
    pub fn dependencies(mut self, dependencies: RuntimeDependencies) -> Self {
        self.dependencies = Some(dependencies);
        self
    }

    /// Synchronously composes lazy/async services for straightforward Tauri setup.
    pub fn build(mut self) -> RuntimeResult<MkvoRuntime> {
        std::fs::create_dir_all(&self.config.config_root)?;
        let dependencies = match self.dependencies.take() {
            Some(dependencies) => dependencies,
            None => self.local_dependencies()?,
        };
        Ok(MkvoRuntime::from_parts(self.config, dependencies))
    }

    fn local_dependencies(&mut self) -> RuntimeResult<RuntimeDependencies> {
        let roots = self
            .config
            .authorized_roots
            .iter()
            .map(|(path, writable)| AuthorizedRoot::new(path, *writable))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| RuntimeError::invalid(error.to_string()))?;
        let authorized = AuthorizedRoots::new(roots)
            .map_err(|error| RuntimeError::invalid(error.to_string()))?;

        let repositories = SqliteRepositories::open(self.config.config_root.join("mkvo.db"))
            .map_err(|error| RuntimeError::internal(error.to_string()))?;
        let watcher = Arc::new(WatchService::new(WatchOptions::default()));
        let base_secrets = match self.secret_store.clone() {
            Some(store) => store,
            None => {
                let path = self.config.config_root.join("secrets.json");
                let mut values = read_secret_values(&path)?;
                if let Some(source) = self.config.resolved_legacy_settings_path() {
                    let outcome =
                        import_legacy_settings(repositories.store(), &source, |key, value| {
                            values
                                .entry(key.to_owned())
                                .or_insert_with(|| value.to_owned());
                            persist_secret_values(&path, &values)
                        })
                        .map_err(|error| RuntimeError::internal(error.to_string()))?;
                    if let LegacyImportOutcome::Imported {
                        revision,
                        secrets_imported,
                        backup_path,
                        ..
                    } = outcome
                    {
                        tracing::info!(
                            revision,
                            secrets_imported,
                            backup_path = %backup_path.display(),
                            "imported legacy MKVO settings"
                        );
                    }
                }
                Arc::new(FileSecretStore::from_values(path, values)) as Arc<dyn SecretStore>
            }
        };
        let secrets = if self.secret_overrides.is_empty() {
            base_secrets
        } else {
            Arc::new(LayeredSecretStore::new(
                self.secret_overrides.clone(),
                base_secrets,
            )) as Arc<dyn SecretStore>
        };
        let persisted_settings = repositories
            .store()
            .get_setting_with_revision::<AppSettings>("app")
            .map_err(|error| RuntimeError::internal(error.to_string()))?
            .map_or_else(AppSettings::default, |(_, _, settings)| settings)
            .normalized();
        self.config
            .scan_worker_override
            .get_or_insert(persisted_settings.workers.max_scan_workers);
        self.config
            .edit_worker_override
            .get_or_insert(persisted_settings.workers.max_edit_workers);
        self.config
            .remux_worker_override
            .get_or_insert(persisted_settings.workers.max_remux_workers);
        let quick_hash = persisted_settings
            .scan
            .use_quick_hash_on_unreliable_timestamps;
        let file_system = Arc::new(LocalFileSystem::new(authorized.clone(), quick_hash));
        let catalog = Arc::new(LocalMediaCatalog::new(authorized.clone(), quick_hash));
        if self.config.browse_scope == BrowseScope::Unrestricted {
            for path in persisted_settings
                .scan
                .default_root
                .iter()
                .chain(
                    persisted_settings
                        .scan
                        .library_roots
                        .iter()
                        .map(|root| &root.path),
                )
                .chain(persisted_settings.watch.roots.iter())
            {
                if let Err(error) = authorized.grant(path, true) {
                    tracing::warn!(
                        path = %path.display(),
                        error = %error,
                        "persisted root could not be authorized"
                    );
                }
            }
        }
        let (explicit_tools, search_directories) = tool_configuration(
            &persisted_settings,
            self.config.tool_directories.iter().cloned(),
        );
        let mut tool_builder = ProcessToolRegistry::builder();
        for (kind, path) in explicit_tools {
            tool_builder = tool_builder.explicit(kind, path);
        }
        for directory in search_directories {
            tool_builder = tool_builder.search_directory(directory);
        }
        let tools = tool_builder.build();
        let runner = ProcessRunner;
        let probe = MediaScanAdapter::new(tools.clone(), runner.clone());
        let repositories = Arc::new(repositories);

        Ok(RuntimeDependencies {
            catalog,
            probe: Arc::new(probe),
            cache: repositories.clone(),
            paths: Arc::new(authorized.clone()),
            file_system,
            tools: Arc::new(tools.clone()),
            tool_executor: Arc::new(ProcessToolExecutor::new(runner)),
            settings: repositories.clone(),
            secrets,
            plans: repositories.clone(),
            jobs: repositories.clone(),
            rename_history: repositories.clone(),
            journal: repositories.clone(),
            logs: repositories,
            watcher: Some(watcher),
            process_tools: Some(tools),
            authorized_roots: Some(authorized),
        })
    }
}

fn read_secret_values(path: &std::path::Path) -> Result<BTreeMap<String, String>, std::io::Error> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
        Err(error) => Err(error),
    }
}

fn persist_secret_values(
    path: &std::path::Path,
    values: &BTreeMap<String, String>,
) -> Result<(), std::io::Error> {
    let bytes = serde_json::to_vec(values)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    persist_bytes_atomic(path, &bytes, true)
}

pub(crate) fn persist_bytes_atomic(
    path: &std::path::Path,
    bytes: &[u8],
    private: bool,
) -> Result<(), std::io::Error> {
    #[cfg(not(unix))]
    let _ = private;
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "path has no parent")
    })?;
    std::fs::create_dir_all(parent)?;
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("mkvo-data");
    let temporary = parent.join(format!(".{name}.{}.tmp", uuid::Uuid::new_v4()));
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)?;
    #[cfg(unix)]
    if private {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    if let Err(error) = std::fs::rename(&temporary, path) {
        let _ = std::fs::remove_file(&temporary);
        return Err(error);
    }
    #[cfg(unix)]
    std::fs::File::open(parent)?.sync_all()?;
    Ok(())
}

fn legacy_app_data_root() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        return std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .map(|root| root.join("MKVOrchestrator"));
    }
    #[cfg(target_os = "macos")]
    {
        return std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|root| root.join("Library/Application Support/MKVOrchestrator"));
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(root) = std::env::var_os("XDG_CONFIG_HOME") {
            return Some(PathBuf::from(root).join("MKVOrchestrator"));
        }
        return std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|root| root.join(".config/MKVOrchestrator"));
    }
    #[allow(unreachable_code)]
    None
}

pub(crate) fn tool_configuration(
    settings: &AppSettings,
    host_directories: impl IntoIterator<Item = PathBuf>,
) -> (Vec<(ToolKind, PathBuf)>, Vec<PathBuf>) {
    let tools = &settings.tools;
    let explicit = [
        (ToolKind::MkvMerge, tools.mkvmerge_path.clone()),
        (ToolKind::MkvPropEdit, tools.mkvpropedit_path.clone()),
        (ToolKind::MkvExtract, tools.mkvextract_path.clone()),
        (ToolKind::Ffprobe, tools.ffprobe_path.clone()),
        (ToolKind::Ffmpeg, tools.ffmpeg_path.clone()),
    ]
    .into_iter()
    .filter_map(|(kind, path)| path.map(|path| (kind, path)))
    .collect();
    let mut directories: Vec<_> = host_directories.into_iter().collect();
    directories.extend(
        [
            tools.mkvtoolnix_directory.clone(),
            tools.ffmpeg_directory.clone(),
        ]
        .into_iter()
        .flatten(),
    );
    directories.sort();
    directories.dedup();
    (explicit, directories)
}

#[cfg(test)]
mod secret_store_tests {
    use super::*;

    /// Exercises the real platform credential facility. Skips rather than fails
    /// where none is available (headless CI, no Secret Service), because the
    /// production code takes the same fallback in that situation.
    #[tokio::test]
    async fn os_credential_store_round_trips_a_secret() {
        let service = format!("MKVO Test {}", std::process::id());
        let store = KeyringSecretStore::new(&service);
        if !store.is_usable() {
            eprintln!("skipping: no usable OS credential store on this host");
            return;
        }

        let key = "provider.test.api_key";
        assert_eq!(store.get(key).await.expect("absent read"), None);

        store.set(key, "s3cret").await.expect("write");
        assert_eq!(
            store.get(key).await.expect("read"),
            Some("s3cret".to_owned())
        );

        store.set(key, "rotated").await.expect("rotate");
        assert_eq!(
            store.get(key).await.expect("read"),
            Some("rotated".to_owned())
        );

        store.remove(key).await.expect("remove");
        assert_eq!(store.get(key).await.expect("read after remove"), None);
        // Removing an absent secret is the desired end state, not an error.
        store.remove(key).await.expect("idempotent remove");
    }
}
