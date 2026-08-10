//! One canonical spelling for a path used as an identity.
//!
//! On Windows `fs::canonicalize` returns the extended-length `\\?\` form, but
//! filesystem-watch events, user input, and configured settings arrive in the
//! plain form. Two spellings of the same file must not become two cache rows or
//! a lookup that silently misses, so everything that keys on a path — cache
//! rows, plan resource claims, UI text — routes through here first.
//!
//! The prefix is kept where removing it would change meaning: a device path
//! such as `\\?\Volume{...}` has no plain equivalent, and long paths keep their
//! prefix when handed to an external tool, which is a separate concern from
//! identity.

use std::path::{Path, PathBuf};

/// Strip the Windows extended-length prefix when the result is still a valid
/// path. Non-Windows paths and device paths are returned unchanged.
#[must_use]
pub fn normalized_path_text(path: &Path) -> String {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{rest}");
    }
    if let Some(rest) = text.strip_prefix(r"\\?\")
        && rest.as_bytes().get(1) == Some(&b':')
    {
        return rest.to_owned();
    }
    text.into_owned()
}

/// [`normalized_path_text`] as a `PathBuf`.
#[must_use]
pub fn normalized_path(path: &Path) -> PathBuf {
    PathBuf::from(normalized_path_text(path))
}

/// Compare two paths by identity rather than by spelling.
#[must_use]
pub fn same_path(left: &Path, right: &Path) -> bool {
    let left = normalized_path_text(left);
    let right = normalized_path_text(right);
    if cfg!(windows) {
        left.eq_ignore_ascii_case(&right)
    } else {
        left == right
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The cache is written from canonicalized scan paths but queried with raw
    /// watcher paths. Without one spelling, deletions never match a cache row
    /// and prune silently does nothing.
    #[test]
    fn canonical_and_raw_windows_paths_share_one_key() {
        assert_eq!(
            normalized_path_text(Path::new(r"\\?\C:\media\a.mkv")),
            normalized_path_text(Path::new(r"C:\media\a.mkv"))
        );
        assert!(same_path(
            Path::new(r"\\?\C:\media\a.mkv"),
            Path::new(r"C:\media\a.mkv")
        ));
    }

    #[test]
    fn unc_paths_normalize_to_their_plain_form() {
        assert_eq!(
            normalized_path_text(Path::new(r"\\?\UNC\nas\media\a.mkv")),
            r"\\nas\media\a.mkv"
        );
    }

    #[test]
    fn paths_without_a_plain_form_are_left_intact() {
        assert_eq!(
            normalized_path_text(Path::new(r"\\?\Volume{9f3a}\media")),
            r"\\?\Volume{9f3a}\media"
        );
        assert_eq!(
            normalized_path_text(Path::new("/mnt/media/a.mkv")),
            "/mnt/media/a.mkv"
        );
    }
}
