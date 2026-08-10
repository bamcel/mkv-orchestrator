use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::{Component, Path, PathBuf},
    sync::{Arc, RwLock},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessMode {
    Read,
    Write,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizedRoot {
    path: PathBuf,
    writable: bool,
}

impl AuthorizedRoot {
    pub fn new(path: impl AsRef<Path>, writable: bool) -> Result<Self, PathAuthorizationError> {
        let input = path.as_ref();
        if !input.is_absolute() {
            return Err(PathAuthorizationError::Relative(input.to_path_buf()));
        }
        let path = std::fs::canonicalize(input).map_err(|source| {
            PathAuthorizationError::Canonicalize {
                path: input.to_path_buf(),
                source,
            }
        })?;
        if !path.is_dir() {
            return Err(PathAuthorizationError::RootNotDirectory(path));
        }
        Ok(Self { path, writable })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub const fn writable(&self) -> bool {
        self.writable
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizedPath {
    path: PathBuf,
    root: PathBuf,
    access: AccessMode,
}

impl AuthorizedPath {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub const fn access(&self) -> AccessMode {
        self.access
    }

    pub fn into_path_buf(self) -> PathBuf {
        self.path
    }
}

#[derive(Debug, Clone, Default)]
pub struct AuthorizedRoots {
    roots: Arc<RwLock<Vec<AuthorizedRoot>>>,
}

impl AuthorizedRoots {
    pub fn new(
        roots: impl IntoIterator<Item = AuthorizedRoot>,
    ) -> Result<Self, PathAuthorizationError> {
        let mut merged = BTreeMap::<PathBuf, bool>::new();
        for root in roots {
            merged
                .entry(root.path)
                .and_modify(|writable| *writable |= root.writable)
                .or_insert(root.writable);
        }
        let mut roots = merged
            .into_iter()
            .map(|(path, writable)| AuthorizedRoot { path, writable })
            .collect::<Vec<_>>();
        sort_roots(&mut roots);
        if roots.is_empty() {
            return Err(PathAuthorizationError::NoRoots);
        }
        Ok(Self {
            roots: Arc::new(RwLock::new(roots)),
        })
    }

    /// Return an immutable snapshot of the current grants.
    pub fn roots(&self) -> Vec<AuthorizedRoot> {
        self.roots
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Canonicalize and atomically add a root grant. Existing clones observe
    /// the grant immediately. Re-granting an exact root upgrades read-only to
    /// writable but never silently downgrades a writable grant.
    pub fn grant(
        &self,
        path: impl AsRef<Path>,
        writable: bool,
    ) -> Result<AuthorizedRoot, PathAuthorizationError> {
        let root = AuthorizedRoot::new(path, writable)?;
        let mut roots = self
            .roots
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(existing) = roots.iter_mut().find(|existing| existing.path == root.path) {
            existing.writable |= root.writable;
            return Ok(existing.clone());
        }
        roots.push(root.clone());
        sort_roots(&mut roots);
        Ok(root)
    }

    /// Revoke an exact canonical root. Descendant or parent grants are not
    /// implicitly changed.
    pub fn revoke(&self, path: impl AsRef<Path>) -> Result<bool, PathAuthorizationError> {
        let input = path.as_ref();
        require_absolute(input)?;
        let canonical = std::fs::canonicalize(input).map_err(|source| {
            PathAuthorizationError::Canonicalize {
                path: input.to_path_buf(),
                source,
            }
        })?;
        let mut roots = self
            .roots
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let before = roots.len();
        roots.retain(|root| root.path != canonical);
        Ok(roots.len() != before)
    }

    pub fn authorize_existing(
        &self,
        path: impl AsRef<Path>,
        access: AccessMode,
    ) -> Result<AuthorizedPath, PathAuthorizationError> {
        let input = path.as_ref();
        require_absolute(input)?;
        let canonical = std::fs::canonicalize(input).map_err(|source| {
            PathAuthorizationError::Canonicalize {
                path: input.to_path_buf(),
                source,
            }
        })?;
        self.authorize_canonical(canonical, access)
    }

    /// Authorizes an output path that may not exist yet.
    ///
    /// The nearest existing ancestor is canonicalized, so a symlinked parent
    /// cannot escape an authorized root. Call this again immediately before a
    /// mutation to protect against a parent being replaced after preview.
    pub fn authorize_candidate(
        &self,
        path: impl AsRef<Path>,
        access: AccessMode,
    ) -> Result<AuthorizedPath, PathAuthorizationError> {
        let input = path.as_ref();
        require_absolute(input)?;
        let lexical = lexical_normalize(input)?;
        if lexical.exists() {
            return self.authorize_existing(lexical, access);
        }

        let mut ancestor = lexical.as_path();
        let mut suffix = Vec::<OsString>::new();
        while !ancestor.exists() {
            let name = ancestor
                .file_name()
                .ok_or_else(|| PathAuthorizationError::NoExistingAncestor(lexical.clone()))?;
            suffix.push(name.to_os_string());
            ancestor = ancestor
                .parent()
                .ok_or_else(|| PathAuthorizationError::NoExistingAncestor(lexical.clone()))?;
        }
        if !ancestor.is_dir() {
            return Err(PathAuthorizationError::AncestorNotDirectory(
                ancestor.to_path_buf(),
            ));
        }
        let mut canonical = std::fs::canonicalize(ancestor).map_err(|source| {
            PathAuthorizationError::Canonicalize {
                path: ancestor.to_path_buf(),
                source,
            }
        })?;
        for component in suffix.into_iter().rev() {
            canonical.push(component);
        }
        self.authorize_canonical(canonical, access)
    }

    fn authorize_canonical(
        &self,
        canonical: PathBuf,
        access: AccessMode,
    ) -> Result<AuthorizedPath, PathAuthorizationError> {
        let roots = self
            .roots
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let root = roots.iter().find(|root| canonical.starts_with(&root.path));
        match root {
            Some(root) if access == AccessMode::Write && !root.writable => {
                Err(PathAuthorizationError::ReadOnly(canonical))
            }
            Some(root) => Ok(AuthorizedPath {
                path: canonical,
                root: root.path.clone(),
                access,
            }),
            None => Err(PathAuthorizationError::OutsideRoots(canonical)),
        }
    }
}

fn sort_roots(roots: &mut [AuthorizedRoot]) {
    roots.sort_by(|left, right| {
        right
            .path
            .components()
            .count()
            .cmp(&left.path.components().count())
    });
}

#[derive(Debug, Error)]
pub enum PathAuthorizationError {
    #[error("at least one authorized filesystem root is required")]
    NoRoots,
    #[error("filesystem path must be absolute: `{0}`")]
    Relative(PathBuf),
    #[error("configured root is not a directory: `{0}`")]
    RootNotDirectory(PathBuf),
    #[error("could not resolve `{path}`: {source}")]
    Canonicalize {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("path has no existing ancestor: `{0}`")]
    NoExistingAncestor(PathBuf),
    #[error("nearest existing ancestor is not a directory: `{0}`")]
    AncestorNotDirectory(PathBuf),
    #[error("path traversal escapes its absolute root: `{0}`")]
    Traversal(PathBuf),
    #[error("path is outside configured roots: `{0}`")]
    OutsideRoots(PathBuf),
    #[error("path is inside a read-only root: `{0}`")]
    ReadOnly(PathBuf),
}

fn require_absolute(path: &Path) -> Result<(), PathAuthorizationError> {
    if path.is_absolute() {
        Ok(())
    } else {
        Err(PathAuthorizationError::Relative(path.to_path_buf()))
    }
}

fn lexical_normalize(path: &Path) -> Result<PathBuf, PathAuthorizationError> {
    let mut output = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => output.push(prefix.as_os_str()),
            Component::RootDir => output.push(component.as_os_str()),
            Component::CurDir => {}
            Component::Normal(value) => output.push(value),
            Component::ParentDir => {
                if !output.pop() || output.as_os_str().is_empty() {
                    return Err(PathAuthorizationError::Traversal(path.to_path_buf()));
                }
            }
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestTree(PathBuf);

    impl TestTree {
        fn new() -> Self {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos();
            let path =
                std::env::temp_dir().join(format!("mkvo-roots-{}-{stamp}", std::process::id()));
            std::fs::create_dir_all(path.join("library/season")).expect("create tree");
            std::fs::write(path.join("library/season/a.mkv"), b"mkv").expect("write file");
            Self(path)
        }
    }

    impl Drop for TestTree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn existing_and_future_paths_are_confined() {
        let tree = TestTree::new();
        let roots =
            AuthorizedRoots::new(
                [AuthorizedRoot::new(tree.0.join("library"), true).expect("root")],
            )
            .expect("roots");
        let existing = roots
            .authorize_existing(tree.0.join("library/season/a.mkv"), AccessMode::Read)
            .expect("existing authorized");
        assert!(existing.path().ends_with("a.mkv"));

        let future = roots
            .authorize_candidate(tree.0.join("library/new/output.mkv"), AccessMode::Write)
            .expect("future authorized");
        assert!(future.path().ends_with(Path::new("new/output.mkv")));
    }

    #[test]
    fn blocks_outside_and_read_only_writes() {
        let tree = TestTree::new();
        let roots = AuthorizedRoots::new([
            AuthorizedRoot::new(tree.0.join("library"), false).expect("root")
        ])
        .expect("roots");
        assert!(matches!(
            roots.authorize_candidate(tree.0.join("library/new.mkv"), AccessMode::Write),
            Err(PathAuthorizationError::ReadOnly(_))
        ));
        assert!(matches!(
            roots.authorize_existing(&tree.0, AccessMode::Read),
            Err(PathAuthorizationError::OutsideRoots(_))
        ));
    }

    #[test]
    fn dynamic_grants_are_shared_canonical_and_overlap_safe() {
        let tree = TestTree::new();
        std::fs::create_dir_all(tree.0.join("outside")).expect("outside directory");
        std::fs::write(tree.0.join("outside/new.mkv"), b"outside").expect("outside file");
        let roots = AuthorizedRoots::new([
            AuthorizedRoot::new(tree.0.join("library"), true).expect("library root")
        ])
        .expect("roots");
        let existing_clone = roots.clone();

        assert!(matches!(
            existing_clone.authorize_existing(tree.0.join("outside/new.mkv"), AccessMode::Read),
            Err(PathAuthorizationError::OutsideRoots(_))
        ));
        roots
            .grant(tree.0.join("outside/../outside"), false)
            .expect("canonical grant");
        assert!(
            existing_clone
                .authorize_existing(tree.0.join("outside/new.mkv"), AccessMode::Read)
                .is_ok()
        );
        assert!(matches!(
            existing_clone.authorize_existing(tree.0.join("outside/new.mkv"), AccessMode::Write),
            Err(PathAuthorizationError::ReadOnly(_))
        ));

        roots
            .grant(tree.0.join("library/season"), false)
            .expect("nested read-only root");
        assert!(matches!(
            existing_clone
                .authorize_existing(tree.0.join("library/season/a.mkv"), AccessMode::Write),
            Err(PathAuthorizationError::ReadOnly(_))
        ));
        roots
            .grant(tree.0.join("library/season/."), true)
            .expect("upgrade exact canonical root");
        assert!(
            existing_clone
                .authorize_existing(tree.0.join("library/season/a.mkv"), AccessMode::Write)
                .is_ok()
        );
        assert_eq!(
            roots
                .roots()
                .iter()
                .filter(|root| root.path().ends_with("season"))
                .count(),
            1,
            "canonical duplicate grants are merged"
        );

        assert!(roots.revoke(tree.0.join("outside")).expect("revoke"));
        assert!(matches!(
            existing_clone.authorize_existing(tree.0.join("outside/new.mkv"), AccessMode::Read),
            Err(PathAuthorizationError::OutsideRoots(_))
        ));
    }

    fn fixture_path(root: &Path, path: &str) -> PathBuf {
        if path == "/fixture" {
            return root.to_owned();
        }
        if let Some(relative) = path.strip_prefix("/fixture/") {
            return relative
                .split('/')
                .fold(root.to_owned(), |path, segment| path.join(segment));
        }
        PathBuf::from(path)
    }

    fn error_code(error: &PathAuthorizationError) -> &'static str {
        match error {
            PathAuthorizationError::NoRoots => "no_roots",
            PathAuthorizationError::Relative(_) => "relative",
            PathAuthorizationError::RootNotDirectory(_) => "root_not_directory",
            PathAuthorizationError::Canonicalize { .. } => "canonicalize_failed",
            PathAuthorizationError::NoExistingAncestor(_) => "no_existing_ancestor",
            PathAuthorizationError::AncestorNotDirectory(_) => "ancestor_not_directory",
            PathAuthorizationError::Traversal(_) => "traversal",
            PathAuthorizationError::OutsideRoots(_) => "outside_roots",
            PathAuthorizationError::ReadOnly(_) => "read_only",
        }
    }

    #[cfg(unix)]
    fn create_directory_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
        std::os::unix::fs::symlink(target, link)
    }

    /// `cmd` treats a forward slash as the start of a switch, so a path built
    /// with `/` separators makes `mklink` fail with "Invalid switch".
    #[cfg(windows)]
    fn cmd_argument(path: &Path) -> std::ffi::OsString {
        std::ffi::OsString::from(path.to_string_lossy().replace('/', "\\"))
    }

    /// Creating a real symlink needs a privilege ordinary Windows accounts do
    /// not hold, so fall back to a directory junction, which does not.
    #[cfg(windows)]
    fn create_directory_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
        match std::os::windows::fs::symlink_dir(target, link) {
            Ok(()) => Ok(()),
            Err(symlink_error) => {
                let status = std::process::Command::new("cmd")
                    .args(["/c", "mklink", "/J"])
                    .arg(cmd_argument(link))
                    .arg(cmd_argument(target))
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()?;
                if status.success() {
                    Ok(())
                } else {
                    Err(symlink_error)
                }
            }
        }
    }

    #[test]
    fn executes_every_path_authorization_fixture_case() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../tests/parity-fixtures/path-authorization.json"
        ))
        .expect("path authorization fixture JSON");
        let tree = TestTree::new();
        std::fs::create_dir_all(tree.0.join("archive")).expect("archive directory");
        std::fs::create_dir_all(tree.0.join("outside")).expect("outside directory");
        std::fs::write(tree.0.join("archive/old.mkv"), b"old").expect("archive file");
        std::fs::write(tree.0.join("outside/outside.mkv"), b"outside").expect("outside file");
        std::fs::write(tree.0.join("root-file"), b"root file").expect("root file");
        create_directory_symlink(&tree.0.join("outside"), &tree.0.join("library/escape"))
            .expect("fixture requires directory symlink support");

        let roots = AuthorizedRoots::new([
            AuthorizedRoot::new(tree.0.join("library"), true).expect("writable root"),
            AuthorizedRoot::new(tree.0.join("archive"), false).expect("read-only root"),
        ])
        .expect("authorized roots");
        // The fixture declares `pathCaseSensitive: true`. Windows and default
        // macOS volumes are case-insensitive, so `posix-` cases describe
        // behavior that cannot hold there and are asserted on POSIX hosts only.
        let case_sensitive_filesystem = !tree.0.join("LIBRARY").is_dir();

        let cases = fixture["cases"].as_array().expect("fixture cases");
        assert_eq!(cases.len(), 17, "fixture case count changed");
        for case in cases {
            let id = case["id"].as_str().expect("case id");
            if id.starts_with("posix-") && !case_sensitive_filesystem {
                continue;
            }
            let operation = case["operation"].as_str().expect("operation");
            let input = &case["input"];
            let expected = &case["expected"];
            if operation == "AuthorizedRoot.new" {
                let raw_path = input["path"].as_str().expect("root path");
                let path = if raw_path.starts_with("/fixture") {
                    fixture_path(&tree.0, raw_path)
                } else {
                    PathBuf::from(raw_path)
                };
                let result = AuthorizedRoot::new(path, input["writable"].as_bool().unwrap());
                assert_eq!(
                    result.is_ok(),
                    expected["created"].as_bool().unwrap(),
                    "{id}"
                );
                if let Err(error) = result {
                    assert_eq!(
                        error_code(&error),
                        expected["error"]["code"].as_str().unwrap()
                    );
                }
                continue;
            }
            if operation == "AuthorizedRoots.new" {
                let result = AuthorizedRoots::new(Vec::<AuthorizedRoot>::new());
                assert_eq!(
                    result.is_ok(),
                    expected["created"].as_bool().unwrap(),
                    "{id}"
                );
                assert_eq!(
                    error_code(&result.unwrap_err()),
                    expected["error"]["code"].as_str().unwrap()
                );
                continue;
            }

            let raw_path = input["path"].as_str().expect("path");
            let path = if id == "parent-traversal-above-filesystem-root" {
                // The fixture asserts traversal *above* the filesystem root, so
                // the `..` count has to exceed the depth of the temporary tree.
                // A fixed count silently degrades to `outside_roots` on hosts
                // whose temp directory is deeper than the count.
                let depth = tree
                    .0
                    .components()
                    .filter(|component| matches!(component, Component::Normal(_)))
                    .count();
                let mut escape = tree.0.clone();
                for _ in 0..=depth {
                    escape.push("..");
                }
                escape.join("escape.mkv")
            } else if raw_path.starts_with("/fixture") {
                fixture_path(&tree.0, raw_path)
            } else {
                PathBuf::from(raw_path)
            };
            let access = match input["access"].as_str().expect("access") {
                "read" => AccessMode::Read,
                "write" => AccessMode::Write,
                _ => unreachable!("known access mode"),
            };
            let result = if operation == "authorize_existing" {
                roots.authorize_existing(path, access)
            } else {
                roots.authorize_candidate(path, access)
            };
            assert_eq!(
                result.is_ok(),
                expected["authorized"].as_bool().unwrap(),
                "fixture case `{id}`"
            );
            match result {
                Ok(authorized) => {
                    assert_eq!(authorized.access(), access);
                    assert!(authorized.path().starts_with(authorized.root()));
                }
                Err(error) => assert_eq!(
                    error_code(&error),
                    expected["error"]["code"].as_str().unwrap(),
                    "fixture case `{id}`: {error}"
                ),
            }
        }
    }
}
