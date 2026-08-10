use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{MediaServerId, MetadataProvider};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub schema_version: u32,
    pub tools: ToolSettings,
    pub scan: ScanSettings,
    pub rename: RenameSettings,
    pub watch: WatchSettings,
    pub providers: ProviderSettings,
    #[serde(default)]
    pub media_servers: Vec<MediaServerSettings>,
    #[serde(default)]
    pub media_server_path_mappings: Vec<PathMapping>,
    pub workers: WorkerSettings,
    pub appearance: AppearanceSettings,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            schema_version: 1,
            tools: ToolSettings::default(),
            scan: ScanSettings::default(),
            rename: RenameSettings::default(),
            watch: WatchSettings::default(),
            providers: ProviderSettings::default(),
            media_servers: Vec::new(),
            media_server_path_mappings: Vec::new(),
            workers: WorkerSettings::default(),
            appearance: AppearanceSettings::default(),
        }
    }
}

impl AppSettings {
    #[must_use]
    pub fn normalized(mut self) -> Self {
        self.workers = self.workers.normalized();
        self.tools = self.tools.normalized();
        self.scan.default_root = configured_path(self.scan.default_root);
        self.watch.roots = self
            .watch
            .roots
            .into_iter()
            .filter_map(|root| configured_path(Some(root)))
            .collect();
        self.scan.ignored_folder_names = self
            .scan
            .ignored_folder_names
            .into_iter()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .collect();
        let mut seen_templates = BTreeSet::new();
        self.rename.templates = self
            .rename
            .templates
            .into_iter()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .filter(|value| seen_templates.insert(value.clone()))
            .collect();
        self.watch.roots.sort();
        self.watch.roots.dedup();
        self
    }
}

/// A blank path is "not configured", never "configured to nothing". An explicit
/// tool path deliberately never falls back to directory or `PATH` search, so a
/// blank value that survives here silently makes that tool unresolvable.
fn configured_path(path: Option<PathBuf>) -> Option<PathBuf> {
    path.filter(|path| !path.as_os_str().is_empty())
        .map(|path| PathBuf::from(path.to_string_lossy().trim()))
        .filter(|path| !path.as_os_str().is_empty())
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mkvtoolnix_directory: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ffmpeg_directory: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mkvmerge_path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mkvpropedit_path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mkvextract_path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ffprobe_path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ffmpeg_path: Option<PathBuf>,
}

impl ToolSettings {
    #[must_use]
    pub fn normalized(self) -> Self {
        Self {
            mkvtoolnix_directory: configured_path(self.mkvtoolnix_directory),
            ffmpeg_directory: configured_path(self.ffmpeg_directory),
            mkvmerge_path: configured_path(self.mkvmerge_path),
            mkvpropedit_path: configured_path(self.mkvpropedit_path),
            mkvextract_path: configured_path(self.mkvextract_path),
            ffprobe_path: configured_path(self.ffprobe_path),
            ffmpeg_path: configured_path(self.ffmpeg_path),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_root: Option<PathBuf>,
    #[serde(default)]
    pub ignored_folder_names: BTreeSet<String>,
    #[serde(default = "default_supported_extensions")]
    pub supported_extensions: BTreeSet<String>,
    #[serde(default)]
    pub use_quick_hash_on_unreliable_timestamps: bool,
}

fn default_supported_extensions() -> BTreeSet<String> {
    ["mkv", "mka", "webm", "mp4", "m4v"]
        .into_iter()
        .map(str::to_owned)
        .collect()
}

impl Default for ScanSettings {
    fn default() -> Self {
        Self {
            default_root: None,
            ignored_folder_names: [
                "Extras",
                "OVAs",
                "Backdrops",
                "Specials",
                "Trailers",
                "Trailer",
                "Featurettes",
                "Samples",
                "Sample",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
            supported_extensions: default_supported_extensions(),
            use_quick_hash_on_unreliable_timestamps: false,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameSettings {
    pub provider: MetadataProvider,
    pub language: String,
    pub template: String,
    #[serde(default)]
    pub templates: Vec<String>,
    #[serde(default)]
    pub compact_preview: bool,
    #[serde(default = "default_history_days")]
    pub history_retention_days: u32,
}

const fn default_history_days() -> u32 {
    90
}

impl Default for RenameSettings {
    fn default() -> Self {
        Self {
            provider: MetadataProvider::Tvdb,
            language: "eng".to_owned(),
            template: "{series} - S{season:00}E{episode:00} - {episodeTitle}".to_owned(),
            templates: vec![
                "{title}".to_owned(),
                "{title} ({year})".to_owned(),
                "{series} - S{season:00}E{episode:00} - {episodeTitle}".to_owned(),
                "S{season:00}E{episode:00} - {episodeTitle}".to_owned(),
                "{series} - {absolute:000} - {episodeTitle}".to_owned(),
            ],
            compact_preview: false,
            history_retention_days: default_history_days(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub roots: Vec<PathBuf>,
    #[serde(default = "default_debounce_millis")]
    pub debounce_millis: u64,
    #[serde(default = "default_reconcile_minutes")]
    pub reconciliation_interval_minutes: u64,
    #[serde(default)]
    pub force_polling: bool,
}

const fn default_debounce_millis() -> u64 {
    750
}

const fn default_reconcile_minutes() -> u64 {
    30
}

impl Default for WatchSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            roots: Vec::new(),
            debounce_millis: default_debounce_millis(),
            reconciliation_interval_minutes: default_reconcile_minutes(),
            force_polling: false,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSettings {
    #[serde(default)]
    pub configured: BTreeMap<MetadataProvider, CredentialState>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialState {
    pub configured: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub masked_hint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret_reference: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaServerSettings {
    pub id: MediaServerId,
    pub name: String,
    pub kind: MediaServerKind,
    pub server_url: String,
    pub credential: CredentialState,
    #[serde(default)]
    pub is_default: bool,
    #[serde(default)]
    pub libraries: Vec<MediaServerLibrary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_synced_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaServerKind {
    Emby,
    Jellyfin,
    Plex,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaServerLibrary {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    pub server_path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_path: Option<PathBuf>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

const fn default_true() -> bool {
    true
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PathMapping {
    pub remote_prefix: PathBuf,
    pub local_prefix: PathBuf,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerSettings {
    pub max_scan_workers: usize,
    pub max_edit_workers: usize,
    pub max_remux_workers: usize,
}

impl Default for WorkerSettings {
    fn default() -> Self {
        Self {
            max_scan_workers: 4,
            max_edit_workers: 2,
            max_remux_workers: 1,
        }
    }
}

impl WorkerSettings {
    #[must_use]
    pub fn normalized(self) -> Self {
        Self {
            max_scan_workers: self.max_scan_workers.clamp(1, 8),
            max_edit_workers: self.max_edit_workers.clamp(1, 6),
            max_remux_workers: self.max_remux_workers.clamp(1, 2),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppearanceSettings {
    pub selected_theme: String,
    #[serde(default)]
    pub custom_themes: Vec<ThemeDefinition>,
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            selected_theme: "Dark".to_owned(),
            custom_themes: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeDefinition {
    pub name: String,
    pub colors: BTreeMap<String, String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A blank tool path pins the tool to a path that cannot exist, and explicit
    /// paths never fall back to directory or `PATH` search — so a blank value
    /// silently makes the tool permanently unavailable.
    #[test]
    fn blank_tool_paths_normalize_to_unconfigured() {
        let settings = AppSettings {
            tools: ToolSettings {
                mkvmerge_path: Some(PathBuf::from("")),
                mkvpropedit_path: Some(PathBuf::from("   ")),
                ffprobe_path: Some(PathBuf::from("D:/tools/ffprobe.exe")),
                ..ToolSettings::default()
            },
            scan: ScanSettings {
                default_root: Some(PathBuf::from("  ")),
                ..ScanSettings::default()
            },
            watch: WatchSettings {
                roots: vec![PathBuf::from(""), PathBuf::from("D:/watch")],
                ..WatchSettings::default()
            },
            ..AppSettings::default()
        }
        .normalized();

        assert_eq!(settings.tools.mkvmerge_path, None);
        assert_eq!(settings.tools.mkvpropedit_path, None);
        assert_eq!(
            settings.tools.ffprobe_path,
            Some(PathBuf::from("D:/tools/ffprobe.exe")),
            "a real configured path is preserved"
        );
        assert_eq!(settings.scan.default_root, None);
        assert_eq!(settings.watch.roots, vec![PathBuf::from("D:/watch")]);
    }

    #[test]
    fn worker_limits_match_existing_product_policy() {
        let normalized = WorkerSettings {
            max_scan_workers: 0,
            max_edit_workers: 99,
            max_remux_workers: 5,
        }
        .normalized();
        assert_eq!(normalized.max_scan_workers, 1);
        assert_eq!(normalized.max_edit_workers, 6);
        assert_eq!(normalized.max_remux_workers, 2);
    }
}
