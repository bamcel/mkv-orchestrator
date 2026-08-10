use std::{
    collections::HashMap,
    env,
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::{ProcessRunner, ProcessSpec};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    MkvMerge,
    MkvPropEdit,
    MkvExtract,
    MkvInfo,
    Ffmpeg,
    Ffprobe,
}

impl ToolKind {
    pub const ALL: [Self; 6] = [
        Self::MkvMerge,
        Self::MkvPropEdit,
        Self::MkvExtract,
        Self::MkvInfo,
        Self::Ffmpeg,
        Self::Ffprobe,
    ];

    pub const fn command_name(self) -> &'static str {
        match self {
            Self::MkvMerge => "mkvmerge",
            Self::MkvPropEdit => "mkvpropedit",
            Self::MkvExtract => "mkvextract",
            Self::MkvInfo => "mkvinfo",
            Self::Ffmpeg => "ffmpeg",
            Self::Ffprobe => "ffprobe",
        }
    }

    /// MKVToolNix accepts `--version`; the FFmpeg tools only accept `-version`
    /// and exit non-zero on the GNU-style spelling, which reads as "installed
    /// but unusable" rather than as a wrong argument.
    pub const fn version_argument(self) -> &'static str {
        match self {
            Self::MkvMerge | Self::MkvPropEdit | Self::MkvExtract | Self::MkvInfo => "--version",
            Self::Ffmpeg | Self::Ffprobe => "-version",
        }
    }

    fn executable_name(self) -> String {
        if cfg!(windows) {
            format!("{}.exe", self.command_name())
        } else {
            self.command_name().to_owned()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedTool {
    pub kind: ToolKind,
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolStatus {
    pub kind: ToolKind,
    pub path: Option<PathBuf>,
    pub available: bool,
    pub version: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ToolRegistryBuilder {
    explicit: HashMap<ToolKind, PathBuf>,
    search_directories: Vec<PathBuf>,
}

impl ToolRegistryBuilder {
    pub fn explicit(mut self, kind: ToolKind, path: impl Into<PathBuf>) -> Self {
        self.explicit.insert(kind, path.into());
        self
    }

    pub fn search_directory(mut self, path: impl Into<PathBuf>) -> Self {
        self.search_directories.push(path.into());
        self
    }

    pub fn build(self) -> ToolRegistry {
        ToolRegistry {
            configuration: Arc::new(RwLock::new(ToolRegistryConfiguration {
                explicit: self.explicit,
                search_directories: self.search_directories,
            })),
        }
    }
}

#[derive(Debug, Clone, Default)]
struct ToolRegistryConfiguration {
    explicit: HashMap<ToolKind, PathBuf>,
    search_directories: Vec<PathBuf>,
}

#[derive(Debug, Clone, Default)]
pub struct ToolRegistry {
    configuration: Arc<RwLock<ToolRegistryConfiguration>>,
}

impl ToolRegistry {
    pub fn builder() -> ToolRegistryBuilder {
        ToolRegistryBuilder::default()
    }

    /// Atomically replace explicit tool paths and additional search directories.
    /// Every clone observes the new snapshot, including clones already owned by
    /// media probes and tool executors.
    pub fn reconfigure<I, D>(&self, explicit: I, search_directories: D)
    where
        I: IntoIterator<Item = (ToolKind, PathBuf)>,
        D: IntoIterator<Item = PathBuf>,
    {
        let next = ToolRegistryConfiguration {
            explicit: explicit.into_iter().collect(),
            search_directories: search_directories.into_iter().collect(),
        };
        let mut configuration = self
            .configuration
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *configuration = next;
    }

    pub fn resolve(&self, kind: ToolKind) -> Option<ResolvedTool> {
        let configuration = self
            .configuration
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if let Some(path) = configuration.explicit.get(&kind) {
            return existing_file(path).map(|path| ResolvedTool { kind, path });
        }

        let executable = kind.executable_name();
        for directory in configuration
            .search_directories
            .iter()
            .chain(common_tool_directories().iter())
        {
            if let Some(path) = existing_file(&directory.join(&executable)) {
                return Some(ResolvedTool { kind, path });
            }
        }

        find_on_path(&executable).map(|path| ResolvedTool { kind, path })
    }

    pub async fn status(&self, kind: ToolKind, runner: &ProcessRunner) -> ToolStatus {
        let Some(resolved) = self.resolve(kind) else {
            return ToolStatus {
                kind,
                path: None,
                available: false,
                version: None,
                error: Some(format!("{} was not found", kind.command_name())),
            };
        };

        let spec = ProcessSpec::new(&resolved.path)
            .arg(kind.version_argument())
            .timeout(Duration::from_secs(10))
            .output_limit(64 * 1024);
        match runner.run(spec, CancellationToken::new()).await {
            Ok(output) if output.success() => {
                let version = output
                    .stdout
                    .lines()
                    .chain(output.stderr.lines())
                    .find(|line| !line.trim().is_empty())
                    .map(|line| line.trim().to_owned());
                ToolStatus {
                    kind,
                    path: Some(resolved.path),
                    available: true,
                    version,
                    error: None,
                }
            }
            Ok(output) => ToolStatus {
                kind,
                path: Some(resolved.path),
                available: false,
                version: None,
                error: Some(format!(
                    "version check exited with code {:?}",
                    output.exit_code
                )),
            },
            Err(error) => ToolStatus {
                kind,
                path: Some(resolved.path),
                available: false,
                version: None,
                error: Some(error.to_string()),
            },
        }
    }
}

fn existing_file(path: &Path) -> Option<PathBuf> {
    if !path.is_file() {
        return None;
    }
    std::fs::canonicalize(path)
        .ok()
        .or_else(|| Some(path.to_path_buf()))
}

fn find_on_path(executable: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .map(|directory| directory.join(executable))
        .find_map(|candidate| existing_file(&candidate))
}

fn common_tool_directories() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if cfg!(windows) {
        for variable in ["ProgramFiles", "ProgramFiles(x86)"] {
            if let Some(base) = env::var_os(variable) {
                paths.push(PathBuf::from(base).join("MKVToolNix"));
            }
        }
    } else {
        paths.extend([PathBuf::from("/usr/bin"), PathBuf::from("/usr/local/bin")]);
        if cfg!(target_os = "macos") {
            paths.extend([
                PathBuf::from("/opt/homebrew/bin"),
                PathBuf::from("/Applications/MKVToolNix-79.0.0.app/Contents/MacOS"),
            ]);
        }
    }
    paths
}

#[allow(dead_code)]
fn has_path_separator(value: &OsStr) -> bool {
    Path::new(value).components().count() > 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, time::SystemTime};

    fn temp_file(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let directory = env::temp_dir().join(format!("mkvo-tools-{}-{stamp}", std::process::id()));
        fs::create_dir_all(&directory).expect("create test directory");
        let path = directory.join(name);
        fs::write(&path, b"test executable placeholder").expect("create test file");
        path
    }

    /// `ffmpeg --version` exits 8 and `ffprobe --version` exits 1, so probing
    /// the FFmpeg tools with the GNU-style flag reports them as present but
    /// unusable and silently disables MP4 inspection and conversion.
    #[test]
    fn ffmpeg_tools_use_the_single_dash_version_flag() {
        for kind in ToolKind::ALL {
            let expected = match kind {
                ToolKind::Ffmpeg | ToolKind::Ffprobe => "-version",
                _ => "--version",
            };
            assert_eq!(
                kind.version_argument(),
                expected,
                "{} version flag",
                kind.command_name()
            );
        }
    }

    #[test]
    fn explicit_configuration_wins() {
        let path = temp_file("custom-mkvmerge");
        let registry = ToolRegistry::builder()
            .explicit(ToolKind::MkvMerge, &path)
            .build();

        let resolved = registry.resolve(ToolKind::MkvMerge).expect("resolved tool");
        assert_eq!(resolved.kind, ToolKind::MkvMerge);
        assert_eq!(
            resolved.path,
            fs::canonicalize(path).expect("canonical path")
        );
    }

    #[test]
    fn missing_explicit_tool_does_not_fall_back_silently() {
        let registry = ToolRegistry::builder()
            .explicit(ToolKind::MkvMerge, "definitely-not-an-mkvo-tool")
            .build();
        assert!(registry.resolve(ToolKind::MkvMerge).is_none());
    }

    #[test]
    fn live_reconfiguration_is_atomic_and_visible_to_existing_clones() {
        let registry = ToolRegistry::builder()
            .explicit(ToolKind::MkvMerge, "definitely-not-an-mkvo-tool")
            .build();
        let probe_clone = registry.clone();
        let executor_clone = registry.clone();
        assert!(probe_clone.resolve(ToolKind::MkvMerge).is_none());

        let path = temp_file("live-mkvmerge");
        registry.reconfigure([(ToolKind::MkvMerge, path.clone())], std::iter::empty());
        let expected = fs::canonicalize(path).expect("canonical path");
        assert_eq!(
            probe_clone.resolve(ToolKind::MkvMerge).unwrap().path,
            expected
        );
        assert_eq!(
            executor_clone.resolve(ToolKind::MkvMerge).unwrap().path,
            expected
        );

        let readers = (0..8)
            .map(|_| {
                let clone = registry.clone();
                std::thread::spawn(move || clone.resolve(ToolKind::MkvMerge).unwrap().path)
            })
            .collect::<Vec<_>>();
        for reader in readers {
            assert_eq!(reader.join().expect("reader thread"), expected);
        }
    }
}
