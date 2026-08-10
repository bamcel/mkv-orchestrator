//! Authorized-root validation and resilient filesystem watching.

mod adapter;
mod paths;
mod reconcile;
mod watcher;

pub use adapter::{LocalFileSystem, LocalMediaCatalog, WatchService};

pub use paths::{
    AccessMode, AuthorizedPath, AuthorizedRoot, AuthorizedRoots, PathAuthorizationError,
};
pub use reconcile::{
    FileIdentity, ReconcileChange, ReconcileChangeKind, Snapshot, SnapshotError, diff_snapshots,
    snapshot_roots,
};
pub use watcher::{
    WatchEvent, WatchEventKind, WatchHandle, WatchMode, WatchOptions, WatchStartError,
};

pub fn is_supported_media_path(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("mkv") || extension.eq_ignore_ascii_case("mp4")
        })
}
