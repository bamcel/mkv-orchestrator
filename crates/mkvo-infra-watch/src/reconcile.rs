use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    time::UNIX_EPOCH,
};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use walkdir::WalkDir;

use crate::is_supported_media_path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileIdentity {
    pub size: u64,
    pub modified_at_ns: i128,
}

pub type Snapshot = BTreeMap<PathBuf, FileIdentity>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReconcileChangeKind {
    Created,
    Modified,
    Removed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReconcileChange {
    pub path: PathBuf,
    pub kind: ReconcileChangeKind,
}

#[derive(Debug, Error)]
pub enum SnapshotError {
    #[error("could not inspect `{path}`: {source}")]
    Metadata {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

pub fn snapshot_roots(roots: &[PathBuf], media_only: bool) -> Result<Snapshot, SnapshotError> {
    let mut snapshot = Snapshot::new();
    for root in roots {
        for entry in WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_map(Result::ok)
        {
            if !entry.file_type().is_file()
                || (media_only && !is_supported_media_path(entry.path()))
            {
                continue;
            }
            let metadata = entry.metadata().map_err(|source| SnapshotError::Metadata {
                path: entry.path().to_path_buf(),
                source: source.into(),
            })?;
            let modified_at_ns = metadata
                .modified()
                .ok()
                .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |duration| {
                    i128::try_from(duration.as_nanos()).unwrap_or(i128::MAX)
                });
            let path =
                std::fs::canonicalize(entry.path()).unwrap_or_else(|_| entry.path().to_path_buf());
            snapshot.insert(
                path,
                FileIdentity {
                    size: metadata.len(),
                    modified_at_ns,
                },
            );
        }
    }
    Ok(snapshot)
}

pub fn diff_snapshots(before: &Snapshot, after: &Snapshot) -> Vec<ReconcileChange> {
    let all_paths: BTreeSet<_> = before.keys().chain(after.keys()).collect();
    all_paths
        .into_iter()
        .filter_map(|path| {
            let kind = match (before.get(path), after.get(path)) {
                (None, Some(_)) => ReconcileChangeKind::Created,
                (Some(_), None) => ReconcileChangeKind::Removed,
                (Some(before), Some(after)) if before != after => ReconcileChangeKind::Modified,
                _ => return None,
            };
            Some(ReconcileChange {
                path: path.clone(),
                kind,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_is_deterministic() {
        let mut before = Snapshot::new();
        before.insert(
            PathBuf::from("a.mkv"),
            FileIdentity {
                size: 1,
                modified_at_ns: 1,
            },
        );
        before.insert(
            PathBuf::from("b.mkv"),
            FileIdentity {
                size: 1,
                modified_at_ns: 1,
            },
        );
        let mut after = Snapshot::new();
        after.insert(
            PathBuf::from("b.mkv"),
            FileIdentity {
                size: 2,
                modified_at_ns: 2,
            },
        );
        after.insert(
            PathBuf::from("c.mkv"),
            FileIdentity {
                size: 1,
                modified_at_ns: 1,
            },
        );

        let changes = diff_snapshots(&before, &after);
        assert_eq!(
            changes.iter().map(|change| change.kind).collect::<Vec<_>>(),
            vec![
                ReconcileChangeKind::Removed,
                ReconcileChangeKind::Modified,
                ReconcileChangeKind::Created,
            ]
        );
    }
}
