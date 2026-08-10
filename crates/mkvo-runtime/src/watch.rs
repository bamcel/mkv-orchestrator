//! Watch-folder supervision.
//!
//! The watch backend only emits changes; it does not update anything. This
//! module is the consumer that turns those changes into cache state, and the
//! only place that starts watchers from persisted settings at boot.
//!
//! Per the architecture plan, filesystem events are hints and reconciliation is
//! authoritative: a lagged subscriber or an explicit rescan request falls back
//! to comparing the filesystem against the cache rather than trusting that
//! every individual event arrived.

use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use mkvo_application::{MediaEnumerationRequest, WatchChange, WatchChangeKind, WatchHealth};
use mkvo_domain::AppSettings;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast::error::RecvError;
use tokio_util::sync::CancellationToken;

use crate::{RuntimeDependencies, RuntimeError, RuntimeResult, runtime::MkvoRuntime};

/// Extensions the cache tracks. Kept in one place so watcher refreshes and
/// folder scans agree on what counts as media.
fn supported_extensions() -> BTreeSet<String> {
    ["mkv", "mka", "webm", "mp4", "m4v"]
        .into_iter()
        .map(str::to_owned)
        .collect()
}

fn is_supported_media(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| {
            supported_extensions()
                .iter()
                .any(|supported| supported.eq_ignore_ascii_case(extension))
        })
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchStartReport {
    pub enabled: bool,
    pub roots: Vec<String>,
    pub started: bool,
    /// Set when watching was requested but could not begin. Startup continues:
    /// a dead watcher must not stop the app from serving requests.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl MkvoRuntime {
    /// Start watchers from persisted settings and begin consuming their events.
    ///
    /// Hosts call this once after composing the runtime. Saving settings later
    /// restarts the backend through `SettingsService`, which this supervisor's
    /// subscription survives because it holds a receiver rather than a handle.
    pub async fn start_watchers(&self) -> RuntimeResult<WatchStartReport> {
        let settings = self.settings_service().load().await?.settings;
        let Some(watcher) = self.dependencies().watcher.clone() else {
            return Ok(WatchStartReport {
                enabled: settings.watch.enabled,
                roots: Vec::new(),
                started: false,
                error: Some("this host composed no watch backend".to_owned()),
            });
        };

        // Subscribe before starting so a change emitted during startup enumeration
        // is queued rather than dropped.
        let changes = watcher.subscribe();
        self.spawn_watch_consumer(changes, settings.clone());

        let roots = settings
            .watch
            .roots
            .iter()
            .map(|root| root.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        if !settings.watch.enabled || settings.watch.roots.is_empty() {
            return Ok(WatchStartReport {
                enabled: settings.watch.enabled,
                roots,
                started: false,
                error: None,
            });
        }

        for root in &settings.watch.roots {
            if let Err(error) = self.grant_authorized_root(root, true) {
                tracing::warn!(
                    path = %root.display(),
                    error = %error,
                    "watch root could not be authorized"
                );
            }
        }

        match watcher
            .start(&settings.watch.roots, settings.watch.force_polling)
            .await
        {
            Ok(()) => {
                tracing::info!(
                    roots = settings.watch.roots.len(),
                    force_polling = settings.watch.force_polling,
                    "watch folders started"
                );
                Ok(WatchStartReport {
                    enabled: true,
                    roots,
                    started: true,
                    error: None,
                })
            }
            Err(error) => {
                tracing::warn!(%error, "watch folders could not be started");
                Ok(WatchStartReport {
                    enabled: true,
                    roots,
                    started: false,
                    error: Some(error.to_string()),
                })
            }
        }
    }

    /// Current watcher health, including which backend the crate selected.
    pub async fn watch_health(&self) -> RuntimeResult<WatchHealth> {
        let watcher = self
            .dependencies()
            .watcher
            .clone()
            .ok_or_else(|| RuntimeError::internal("this host composed no watch backend"))?;
        Ok(watcher.health().await?)
    }

    fn spawn_watch_consumer(
        &self,
        mut changes: tokio::sync::broadcast::Receiver<WatchChange>,
        settings: AppSettings,
    ) {
        let dependencies = self.dependencies().clone();
        let mut roots = settings.watch.roots.clone();
        let ignored = settings.scan.ignored_folder_names.clone();

        tokio::spawn(async move {
            loop {
                match changes.recv().await {
                    Ok(change) => {
                        apply_change(&dependencies, &change, &roots, &ignored).await;
                    }
                    // A lagged receiver means changes were lost, so the cache can
                    // no longer be trusted for these roots and must be rebuilt
                    // from the filesystem.
                    Err(RecvError::Lagged(skipped)) => {
                        tracing::warn!(skipped, "watch subscriber lagged; reconciling watch roots");
                        reconcile_roots(&dependencies, &roots, &ignored).await;
                    }
                    Err(RecvError::Closed) => {
                        tracing::debug!("watch backend closed; stopping consumer");
                        return;
                    }
                }
                // Settings saves restart the backend with new roots; pick those
                // up without needing to respawn this task.
                if let Ok((loaded, _revision)) = dependencies.settings.load().await {
                    roots = loaded.watch.roots.clone();
                }
            }
        });
    }
}

async fn apply_change(
    dependencies: &RuntimeDependencies,
    change: &WatchChange,
    roots: &[PathBuf],
    ignored: &BTreeSet<String>,
) {
    tracing::debug!(
        kind = ?change.kind,
        paths = ?change.paths,
        "watch change received"
    );
    match change.kind {
        WatchChangeKind::RescanRequired => reconcile_roots(dependencies, roots, ignored).await,
        WatchChangeKind::Removed => {
            for path in &change.paths {
                prune(dependencies, path).await;
            }
        }
        // A rename is the old path disappearing and a new path appearing. The
        // backend reports both paths, and a path that still exists refreshes
        // while one that does not is pruned, so either ordering is handled.
        WatchChangeKind::Created | WatchChangeKind::Modified | WatchChangeKind::Renamed => {
            for path in &change.paths {
                if path.exists() {
                    refresh(dependencies, path, ignored).await;
                } else {
                    prune(dependencies, path).await;
                }
            }
        }
    }
}

/// Remove a deleted file, or an entire deleted subtree, from the cache.
async fn prune(dependencies: &RuntimeDependencies, path: &Path) {
    match dependencies.cache.remove(path).await {
        Ok(true) => tracing::debug!(path = %path.display(), "pruned cache entry"),
        Ok(false) => {
            // Not a cached file: it may have been a directory, so drop anything
            // cached beneath it.
            match dependencies.cache.remove_under(path).await {
                Ok(0) => {}
                Ok(removed) => {
                    tracing::debug!(path = %path.display(), removed, "pruned cache subtree");
                }
                Err(error) => {
                    tracing::warn!(path = %path.display(), %error, "cache subtree prune failed");
                }
            }
        }
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "cache prune failed");
        }
    }
}

/// Re-probe a single changed file and update its cache entry.
async fn refresh(dependencies: &RuntimeDependencies, path: &Path, ignored: &BTreeSet<String>) {
    if path.is_file() && !is_supported_media(path) {
        return;
    }
    let cancel = CancellationToken::new();
    let request = MediaEnumerationRequest {
        roots: vec![path.to_path_buf()],
        ignored_folder_names: ignored.iter().map(|value| value.to_lowercase()).collect(),
        supported_extensions: supported_extensions(),
    };
    let fingerprints = match dependencies
        .catalog
        .enumerate(&request, cancel.clone())
        .await
    {
        Ok(fingerprints) => fingerprints,
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "watch refresh could not enumerate");
            return;
        }
    };

    for fingerprint in fingerprints {
        match dependencies.cache.get_valid(&fingerprint).await {
            Ok(Some(_)) => continue,
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(path = %fingerprint.path.display(), %error, "cache lookup failed");
                continue;
            }
        }
        match dependencies
            .probe
            .inspect(&fingerprint.path, cancel.clone())
            .await
        {
            Ok(mut file) => {
                file.fingerprint = fingerprint;
                if let Err(error) = dependencies.cache.upsert(&file).await {
                    tracing::warn!(path = %file.path.display(), %error, "cache upsert failed");
                } else {
                    tracing::debug!(path = %file.path.display(), "watch refreshed cache entry");
                }
            }
            Err(error) => {
                tracing::warn!(
                    path = %fingerprint.path.display(),
                    %error,
                    "watch refresh could not probe"
                );
            }
        }
    }
}

/// Rebuild cache state for the watch roots from the filesystem.
///
/// Used when individual events cannot be trusted. Entries that no longer exist
/// on disk are dropped, and entries that are missing or stale are re-probed.
async fn reconcile_roots(
    dependencies: &RuntimeDependencies,
    roots: &[PathBuf],
    ignored: &BTreeSet<String>,
) {
    if roots.is_empty() {
        return;
    }
    let cancel = CancellationToken::new();
    let request = MediaEnumerationRequest {
        roots: roots.to_vec(),
        ignored_folder_names: ignored.iter().map(|value| value.to_lowercase()).collect(),
        supported_extensions: supported_extensions(),
    };
    let fingerprints = match dependencies
        .catalog
        .enumerate(&request, cancel.clone())
        .await
    {
        Ok(fingerprints) => fingerprints,
        Err(error) => {
            tracing::warn!(%error, "watch reconciliation could not enumerate");
            return;
        }
    };

    let present: BTreeSet<PathBuf> = fingerprints
        .iter()
        .map(|fingerprint| fingerprint.path.clone())
        .collect();
    for root in roots {
        match dependencies.cache.list_under(root).await {
            Ok(cached) => {
                for file in cached {
                    if !present.contains(&file.path) {
                        prune(dependencies, &file.path).await;
                    }
                }
            }
            Err(error) => {
                tracing::warn!(root = %root.display(), %error, "cache listing failed");
            }
        }
    }

    let mut refreshed = 0_usize;
    for fingerprint in fingerprints {
        match dependencies.cache.get_valid(&fingerprint).await {
            Ok(Some(_)) => continue,
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(path = %fingerprint.path.display(), %error, "cache lookup failed");
                continue;
            }
        }
        match dependencies
            .probe
            .inspect(&fingerprint.path, cancel.clone())
            .await
        {
            Ok(mut file) => {
                file.fingerprint = fingerprint;
                if let Err(error) = dependencies.cache.upsert(&file).await {
                    tracing::warn!(path = %file.path.display(), %error, "cache upsert failed");
                } else {
                    refreshed += 1;
                }
            }
            Err(error) => {
                tracing::warn!(
                    path = %fingerprint.path.display(),
                    %error,
                    "reconciliation could not probe"
                );
            }
        }
    }
    tracing::info!(refreshed, "watch reconciliation complete");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_supported_media_extensions_refresh() {
        assert!(is_supported_media(Path::new("a.mkv")));
        assert!(is_supported_media(Path::new("a.MP4")));
        assert!(is_supported_media(Path::new("a.m4v")));
        assert!(!is_supported_media(Path::new("a.srt")));
        assert!(!is_supported_media(Path::new("a.nfo")));
        assert!(!is_supported_media(Path::new("a")));
    }
}
