//! Path comparison shared by the planners.
//!
//! Planners compare paths that reach them from different sources: a scanned
//! `MediaFile` path has been canonicalized, while a configured root may still be
//! the raw string a user typed. On Windows canonicalization adds the
//! extended-length `\\?\` prefix, so the two forms of the same directory do not
//! share a textual prefix. Comparing them without normalizing made every
//! mutating plan report its inputs as unauthorized.

use std::path::Path;

/// Comparison key for a path: separators unified, case folded, and the Windows
/// extended-length prefix removed.
///
/// This is a *textual* key for comparing paths that are already trusted or
/// already resolved. It is not an authorization decision — that stays with the
/// `AuthorizedRoots` service, which resolves symlinks.
#[must_use]
pub fn path_key(path: &Path) -> String {
    let value = path.to_string_lossy().replace('\\', "/");
    let value = value
        .strip_prefix("//?/UNC/")
        .map_or_else(
            || value.strip_prefix("//?/").map(ToOwned::to_owned),
            |rest| Some(format!("//{rest}")),
        )
        .unwrap_or(value);
    value.to_lowercase()
}

/// True when `child` is `root` itself or sits underneath it.
///
/// Matching is segment-aware, so `/media/show` does not contain
/// `/media/show-extras`.
#[must_use]
pub fn path_contains(root: &Path, child: &Path) -> bool {
    let root = path_key(root).trim_end_matches('/').to_owned();
    if root.is_empty() {
        return false;
    }
    let child = path_key(child);
    child == root
        || child
            .strip_prefix(&root)
            .is_some_and(|tail| tail.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A scanned file path is canonical (`\\?\C:\...`) while a configured root
    /// is usually not. Treating those as different directories blocked every
    /// rename, remux, and property edit on Windows.
    #[test]
    fn extended_length_prefix_does_not_change_containment() {
        let root = PathBuf::from(r"C:\Users\me\media");
        let canonical_child = PathBuf::from(r"\\?\C:\Users\me\media\Show\ep1.mkv");
        assert!(path_contains(&root, &canonical_child));

        let canonical_root = PathBuf::from(r"\\?\C:\Users\me\media");
        let plain_child = PathBuf::from(r"C:\Users\me\media\Show\ep1.mkv");
        assert!(path_contains(&canonical_root, &plain_child));
    }

    #[test]
    fn unc_paths_normalize_to_a_single_form() {
        assert_eq!(
            path_key(Path::new(r"\\?\UNC\server\share\file.mkv")),
            path_key(Path::new(r"\\server\share\file.mkv"))
        );
    }

    #[test]
    fn containment_is_segment_aware() {
        let root = PathBuf::from("/media/show");
        assert!(path_contains(&root, Path::new("/media/show")));
        assert!(path_contains(&root, Path::new("/media/show/ep1.mkv")));
        assert!(!path_contains(
            &root,
            Path::new("/media/show-extras/ep1.mkv")
        ));
        assert!(!path_contains(&root, Path::new("/media")));
    }

    #[test]
    fn separator_and_case_differences_are_ignored() {
        assert!(path_contains(
            Path::new(r"C:\Media"),
            Path::new("c:/media/Show/ep1.mkv")
        ));
    }

    #[test]
    fn an_empty_root_never_contains_anything() {
        assert!(!path_contains(Path::new(""), Path::new("/media/ep1.mkv")));
    }
}
