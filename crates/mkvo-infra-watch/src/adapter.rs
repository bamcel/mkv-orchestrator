use std::{
    collections::BTreeSet,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use mkvo_application::{
    AuthorizedPathPolicy, FileAccessState, FileSystem, MediaCatalog, MediaEnumerationRequest,
    PortError, RequiredAccess, WatchBackend, WatchBackendKind, WatchChange, WatchChangeKind,
    WatchHealth,
};
use mkvo_domain::{FileFingerprint, WatchSettings};
use tokio::{
    sync::{Mutex, broadcast},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;
use walkdir::{DirEntry, WalkDir};

use crate::{AccessMode, AuthorizedRoots, WatchHandle, WatchMode, WatchOptions};

#[derive(Debug, Clone)]
pub struct LocalMediaCatalog {
    authorized_roots: Arc<AuthorizedRoots>,
    quick_hash: bool,
}

impl LocalMediaCatalog {
    pub fn new(authorized_roots: AuthorizedRoots, quick_hash: bool) -> Self {
        Self {
            authorized_roots: Arc::new(authorized_roots),
            quick_hash,
        }
    }
}

#[async_trait]
impl MediaCatalog for LocalMediaCatalog {
    async fn enumerate(
        &self,
        request: &MediaEnumerationRequest,
        cancel: CancellationToken,
    ) -> Result<Vec<FileFingerprint>, PortError> {
        let roots = request
            .roots
            .iter()
            .map(|root| {
                self.authorized_roots
                    .authorize_existing(root, AccessMode::Read)
                    .map(|path| path.into_path_buf())
                    .map_err(path_port_error)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let ignored = request.ignored_folder_names.clone();
        let extensions = request.supported_extensions.clone();
        let quick_hash = self.quick_hash;
        tokio::task::spawn_blocking(move || {
            enumerate_files(&roots, &ignored, &extensions, quick_hash, &cancel)
        })
        .await
        .map_err(|error| PortError::Other(format!("media enumeration task failed: {error}")))?
    }
}

fn enumerate_files(
    roots: &[PathBuf],
    ignored: &BTreeSet<String>,
    extensions: &BTreeSet<String>,
    quick_hash: bool,
    cancel: &CancellationToken,
) -> Result<Vec<FileFingerprint>, PortError> {
    let ignored: BTreeSet<_> = ignored
        .iter()
        .map(|value| value.to_ascii_lowercase())
        .collect();
    let extensions: BTreeSet<_> = extensions
        .iter()
        .map(|value| value.trim_start_matches('.').to_ascii_lowercase())
        .collect();
    let mut files = Vec::new();
    for root in roots {
        let walker = WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|entry| !is_ignored_directory(entry, &ignored));
        for entry in walker {
            if cancel.is_cancelled() {
                return Err(PortError::Canceled);
            }
            let entry = entry.map_err(|error| PortError::Other(error.to_string()))?;
            if !entry.file_type().is_file() {
                continue;
            }
            let extension = entry
                .path()
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            if !extensions.contains(&extension) {
                continue;
            }
            files.push(fingerprint_file(entry.path(), quick_hash).map_err(|error| {
                PortError::Other(format!(
                    "could not fingerprint `{}`: {error}",
                    entry.path().display()
                ))
            })?);
        }
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    files.dedup_by(|left, right| left.path == right.path);
    Ok(files)
}

fn is_ignored_directory(entry: &DirEntry, ignored: &BTreeSet<String>) -> bool {
    entry.depth() > 0
        && entry.file_type().is_dir()
        && ignored.contains(&entry.file_name().to_string_lossy().to_ascii_lowercase())
}

#[derive(Debug, Clone)]
pub struct LocalFileSystem {
    authorized_roots: Arc<AuthorizedRoots>,
    quick_hash: bool,
}

impl LocalFileSystem {
    pub fn new(authorized_roots: AuthorizedRoots, quick_hash: bool) -> Self {
        Self {
            authorized_roots: Arc::new(authorized_roots),
            quick_hash,
        }
    }
}

#[async_trait]
impl AuthorizedPathPolicy for AuthorizedRoots {
    async fn authorize_read(&self, path: &Path) -> Result<PathBuf, PortError> {
        self.authorize_existing(path, AccessMode::Read)
            .map(|path| path.into_path_buf())
            .map_err(path_port_error)
    }

    async fn authorize_write(&self, path: &Path) -> Result<PathBuf, PortError> {
        self.authorize_candidate(path, AccessMode::Write)
            .map(|path| path.into_path_buf())
            .map_err(path_port_error)
    }
}

#[async_trait]
impl FileSystem for LocalFileSystem {
    async fn exists(&self, path: &Path) -> Result<bool, PortError> {
        match self
            .authorized_roots
            .authorize_candidate(path, AccessMode::Read)
        {
            Ok(_) => Ok(path.exists()),
            Err(error) => Err(path_port_error(error)),
        }
    }

    async fn is_directory(&self, path: &Path) -> Result<bool, PortError> {
        let path = self
            .authorized_roots
            .authorize_existing(path, AccessMode::Read)
            .map_err(path_port_error)?;
        Ok(path.path().is_dir())
    }

    async fn fingerprint(&self, path: &Path) -> Result<FileFingerprint, PortError> {
        let path = self
            .authorized_roots
            .authorize_existing(path, AccessMode::Read)
            .map_err(path_port_error)?
            .into_path_buf();
        let quick_hash = self.quick_hash;
        tokio::task::spawn_blocking(move || fingerprint_file(&path, quick_hash))
            .await
            .map_err(|error| PortError::Other(format!("fingerprint task failed: {error}")))?
            .map_err(|error| PortError::Other(error.to_string()))
    }

    async fn probe_access(
        &self,
        path: &Path,
        access: RequiredAccess,
    ) -> Result<FileAccessState, PortError> {
        let mode = match access {
            RequiredAccess::Read => AccessMode::Read,
            RequiredAccess::ReadWrite => AccessMode::Write,
        };
        let authorized = match self.authorized_roots.authorize_existing(path, mode) {
            Ok(path) => path.into_path_buf(),
            // A path that cannot be authorized is reported by the caller's own
            // authorization check; here it simply is not reachable.
            Err(crate::PathAuthorizationError::Canonicalize { .. }) => {
                return Ok(FileAccessState::Missing);
            }
            Err(error) => return Err(path_port_error(error)),
        };

        tokio::task::spawn_blocking(move || probe_file_access(&authorized, access))
            .await
            .map_err(|error| PortError::Other(format!("access probe task failed: {error}")))
    }

    async fn move_file(&self, source: &Path, target: &Path) -> Result<(), PortError> {
        let source = self
            .authorized_roots
            .authorize_existing(source, AccessMode::Write)
            .map_err(path_port_error)?
            .into_path_buf();
        let target = self
            .authorized_roots
            .authorize_candidate(target, AccessMode::Write)
            .map_err(path_port_error)?
            .into_path_buf();
        if target.exists() {
            return Err(PortError::Conflict(format!(
                "target already exists: `{}`",
                target.display()
            )));
        }
        tokio::fs::rename(&source, &target).await.map_err(|error| {
            PortError::Other(format!(
                "could not move `{}` to `{}`: {error}",
                source.display(),
                target.display()
            ))
        })
    }

    async fn remove_file(&self, path: &Path) -> Result<(), PortError> {
        let path = self
            .authorized_roots
            .authorize_existing(path, AccessMode::Write)
            .map_err(path_port_error)?
            .into_path_buf();
        tokio::fs::remove_file(&path).await.map_err(|error| {
            PortError::Other(format!("could not remove `{}`: {error}", path.display()))
        })
    }
}

fn fingerprint_file(
    path: &Path,
    include_quick_hash: bool,
) -> Result<FileFingerprint, std::io::Error> {
    let canonical = std::fs::canonicalize(path)?;
    let metadata = std::fs::metadata(&canonical)?;
    let modified_at: DateTime<Utc> = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH).into();
    let quick_hash = include_quick_hash
        .then(|| quick_hash_file(&canonical, metadata.len()))
        .transpose()?;
    Ok(FileFingerprint {
        path: canonical,
        size_bytes: metadata.len(),
        modified_at,
        quick_hash,
    })
}

fn quick_hash_file(path: &Path, length: u64) -> Result<String, std::io::Error> {
    const CHUNK: usize = 64 * 1024;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    hasher.update(&length.to_le_bytes());
    let mut buffer = vec![0; CHUNK];
    let first = file.read(&mut buffer)?;
    hasher.update(&buffer[..first]);
    if length > CHUNK as u64 {
        file.seek(SeekFrom::End(-(CHUNK as i64)))?;
        let last = file.read(&mut buffer)?;
        hasher.update(&buffer[..last]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

fn path_port_error(error: crate::PathAuthorizationError) -> PortError {
    match error {
        crate::PathAuthorizationError::OutsideRoots(path)
        | crate::PathAuthorizationError::ReadOnly(path)
        | crate::PathAuthorizationError::Relative(path)
        | crate::PathAuthorizationError::Traversal(path) => {
            PortError::Conflict(format!("unauthorized path `{}`", path.display()))
        }
        error => PortError::Other(error.to_string()),
    }
}

#[derive(Debug)]
struct WatchState {
    task: Option<JoinHandle<()>>,
    cancellation: Option<CancellationToken>,
    running: bool,
    backend: WatchBackendKind,
    watched_roots: usize,
    last_event_utc: Option<DateTime<Utc>>,
    error: Option<String>,
}

impl Default for WatchState {
    fn default() -> Self {
        Self {
            task: None,
            cancellation: None,
            running: false,
            backend: WatchBackendKind::Native,
            watched_roots: 0,
            last_event_utc: None,
            error: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct WatchService {
    state: Arc<Mutex<WatchState>>,
    events: broadcast::Sender<WatchChange>,
    options: WatchOptions,
}

impl WatchService {
    pub fn new(options: WatchOptions) -> Self {
        let (events, _) = broadcast::channel(options.channel_capacity.max(16));
        Self {
            state: Arc::new(Mutex::new(WatchState::default())),
            events,
            options,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<WatchChange> {
        self.events.subscribe()
    }

    async fn stop_inner(&self) {
        let mut state = self.state.lock().await;
        if let Some(cancellation) = state.cancellation.take() {
            cancellation.cancel();
        }
        if let Some(task) = state.task.take() {
            task.abort();
        }
        state.running = false;
        state.watched_roots = 0;
    }
}

#[async_trait]
impl WatchBackend for WatchService {
    fn subscribe(&self) -> broadcast::Receiver<WatchChange> {
        WatchService::subscribe(self)
    }

    async fn start(&self, settings: &WatchSettings) -> Result<(), PortError> {
        self.stop_inner().await;
        let mut options = self.options.clone();
        options.roots = settings.roots.clone();
        options.debounce = std::time::Duration::from_millis(settings.debounce_millis.max(50));
        if settings.reconciliation_interval_minutes > 0 {
            options.poll_interval = std::time::Duration::from_secs(
                settings.reconciliation_interval_minutes.saturating_mul(60),
            );
        }
        options.mode = if settings.force_polling {
            WatchMode::Polling
        } else {
            WatchMode::Auto
        };
        let mut handle = WatchHandle::start(options).map_err(|error| {
            PortError::unavailable(format!("watcher could not start: {error}"), true)
        })?;
        let backend = if handle.active_mode() == WatchMode::Polling {
            WatchBackendKind::Polling
        } else {
            WatchBackendKind::Native
        };
        let cancellation = CancellationToken::new();
        let task_cancel = cancellation.clone();
        let state = Arc::clone(&self.state);
        let events = self.events.clone();
        let task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    () = task_cancel.cancelled() => {
                        handle.stop();
                        break;
                    }
                    event = handle.recv() => {
                        let Some(event) = event else { break; };
                        let _ = events.send(WatchChange {
                            kind: match event.kind {
                                crate::WatchEventKind::Created => WatchChangeKind::Created,
                                crate::WatchEventKind::Modified => WatchChangeKind::Modified,
                                crate::WatchEventKind::Removed => WatchChangeKind::Removed,
                                crate::WatchEventKind::Renamed => WatchChangeKind::Renamed,
                                crate::WatchEventKind::RescanRequired => WatchChangeKind::RescanRequired,
                            },
                            paths: event.paths,
                        });
                        state.lock().await.last_event_utc = Some(Utc::now());
                    }
                }
            }
            state.lock().await.running = false;
        });
        let mut state = self.state.lock().await;
        state.task = Some(task);
        state.cancellation = Some(cancellation);
        state.running = true;
        state.backend = backend;
        state.watched_roots = settings.roots.len();
        state.error = None;
        Ok(())
    }

    async fn stop(&self) -> Result<(), PortError> {
        self.stop_inner().await;
        Ok(())
    }

    async fn health(&self) -> Result<WatchHealth, PortError> {
        let state = self.state.lock().await;
        Ok(WatchHealth {
            running: state.running,
            backend: state.backend,
            watched_roots: state.watched_roots,
            last_event_utc: state.last_event_utc,
            error: state.error.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn quick_hash_changes_when_edges_change() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("mkvo-hash-{}-{stamp}", std::process::id()));
        std::fs::write(&path, vec![1_u8; 128 * 1024]).expect("file");
        let first = quick_hash_file(&path, 128 * 1024).expect("hash");
        let mut bytes = vec![1_u8; 128 * 1024];
        bytes[0] = 2;
        std::fs::write(&path, bytes).expect("rewrite");
        let second = quick_hash_file(&path, 128 * 1024).expect("hash");
        let _ = std::fs::remove_file(path);
        assert_ne!(first, second);
    }

    #[tokio::test]
    async fn polling_reconciliation_is_delivered_through_watch_backend_subscription() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("mkvo-watch-events-{}-{stamp}", std::process::id()));
        std::fs::create_dir_all(&root).expect("watch root");
        let service = WatchService::new(WatchOptions {
            mode: WatchMode::Polling,
            poll_interval: std::time::Duration::from_millis(10),
            ..WatchOptions::default()
        });
        let mut changes = WatchBackend::subscribe(&service);
        service
            .start(&WatchSettings {
                enabled: true,
                roots: vec![root.clone()],
                reconciliation_interval_minutes: 0,
                force_polling: true,
                ..WatchSettings::default()
            })
            .await
            .expect("start polling watcher");
        let created = root.join("episode.mkv");
        std::fs::write(&created, b"episode").expect("create media file");

        let change = tokio::time::timeout(std::time::Duration::from_secs(3), changes.recv())
            .await
            .expect("polling reconciliation timed out")
            .expect("watch change channel closed");
        assert_eq!(change.kind, WatchChangeKind::Created);
        assert_eq!(
            change.paths,
            [std::fs::canonicalize(created).expect("canonical created path")]
        );

        service.stop().await.expect("stop watcher");
        let _ = std::fs::remove_dir_all(root);
    }
}

/// Open the file the way a mutation would, and report why that fails.
///
/// The open is exclusive on Windows (`share_mode(0)`), matching what the .NET
/// implementation did: a media file that a media server or player currently has
/// open is exactly the case worth catching, and a shared open would not detect
/// it. Unix has no mandatory locking, so there the probe reports permissions
/// only. The handle is closed immediately.
fn probe_file_access(path: &Path, access: RequiredAccess) -> FileAccessState {
    if !path.exists() {
        return FileAccessState::Missing;
    }

    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    if access == RequiredAccess::ReadWrite {
        options.write(true);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0);
    }

    match options.open(path) {
        Ok(_) => FileAccessState::Available,
        Err(error) => match error.kind() {
            std::io::ErrorKind::PermissionDenied => FileAccessState::ReadOnly,
            std::io::ErrorKind::NotFound => FileAccessState::Missing,
            // Windows reports a sharing violation as os error 32; other kinds
            // that reach here mean the file cannot be opened right now, which
            // is the same practical outcome as being locked.
            _ => FileAccessState::Busy,
        },
    }
}

#[cfg(test)]
mod access_probe_tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "mkvo-access-{}-{name}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).expect("temp dir");
        path
    }

    #[test]
    fn an_ordinary_file_is_available_for_writing() {
        let directory = temp_dir("plain");
        let file = directory.join("a.mkv");
        std::fs::write(&file, b"data").expect("write");

        assert_eq!(
            probe_file_access(&file, RequiredAccess::ReadWrite),
            FileAccessState::Available
        );
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_missing_file_is_reported_as_missing_not_busy() {
        let directory = temp_dir("missing");
        assert_eq!(
            probe_file_access(&directory.join("nope.mkv"), RequiredAccess::ReadWrite),
            FileAccessState::Missing
        );
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// Root bypasses Unix permission checks, so a read-only file really is
    /// writable for it and reporting anything else would be wrong. The MKVO
    /// container runs as root unless PUID is set, and contributors often run
    /// tests in a root container, so this is a normal environment rather than
    /// an exotic one.
    #[cfg(unix)]
    fn permissions_are_enforced_for_this_user() -> bool {
        // `id -u` rather than /proc, which macOS does not have. If the uid
        // cannot be determined, assume permissions are enforced: that is the
        // stricter assertion, so an unknown environment fails loudly instead of
        // quietly skipping the check this test exists for.
        std::process::Command::new("id")
            .arg("-u")
            .output()
            .ok()
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .is_none_or(|uid| uid.trim() != "0")
    }

    #[cfg(not(unix))]
    fn permissions_are_enforced_for_this_user() -> bool {
        true
    }

    /// A read-only file passes every other precondition, so without this the
    /// failure only appears once an external tool is already running.
    #[test]
    fn a_read_only_file_blocks_write_access_but_allows_read() {
        let directory = temp_dir("readonly");
        let file = directory.join("a.mkv");
        std::fs::write(&file, b"data").expect("write");
        let mut permissions = std::fs::metadata(&file).expect("metadata").permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&file, permissions).expect("set readonly");

        if permissions_are_enforced_for_this_user() {
            assert_eq!(
                probe_file_access(&file, RequiredAccess::ReadWrite),
                FileAccessState::ReadOnly
            );
        } else {
            // Reporting the file as writable is the correct answer here: this
            // user can in fact rewrite it.
            assert_eq!(
                probe_file_access(&file, RequiredAccess::ReadWrite),
                FileAccessState::Available,
                "root can write a read-only file, so the probe must say so"
            );
        }
        assert_eq!(
            probe_file_access(&file, RequiredAccess::Read),
            FileAccessState::Available,
            "a read-only file is still readable"
        );

        let mut permissions = std::fs::metadata(&file).expect("metadata").permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(false);
        let _ = std::fs::set_permissions(&file, permissions);
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// The case this exists for: a media server or player holding the file open
    /// while MKVO plans a mutation against it.
    #[cfg(windows)]
    #[test]
    fn a_file_held_open_by_another_handle_is_busy() {
        let directory = temp_dir("busy");
        let file = directory.join("a.mkv");
        std::fs::write(&file, b"data").expect("write");

        let holder = std::fs::File::open(&file).expect("hold open");
        assert_eq!(
            probe_file_access(&file, RequiredAccess::ReadWrite),
            FileAccessState::Busy
        );

        drop(holder);
        assert_eq!(
            probe_file_access(&file, RequiredAccess::ReadWrite),
            FileAccessState::Available,
            "the file is usable again once the other handle closes"
        );
        let _ = std::fs::remove_dir_all(&directory);
    }
}
