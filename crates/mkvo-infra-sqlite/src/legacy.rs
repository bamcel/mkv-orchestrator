use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{self, BufReader},
    path::{Path, PathBuf},
    str::FromStr,
};

use mkvo_domain::{
    AppSettings, CredentialState, MediaServerId, MediaServerKind, MediaServerLibrary,
    MediaServerSettings, MetadataProvider, PathMapping, ThemeDefinition,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::{SqliteStore, StoreError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LegacyImportOutcome {
    SourceMissing,
    SkippedExisting,
    Imported {
        backup_path: PathBuf,
        revision: u64,
        secrets_imported: usize,
        /// The legacy metadata cache is intentionally not imported. It is a
        /// derived artifact and must be rebuilt with Rust tool fingerprints.
        cache_rebuild_required: bool,
    },
}

#[derive(Debug, Error)]
pub enum LegacyImportError {
    #[error("legacy settings I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("legacy settings JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("could not import legacy secret `{key}`: {message}")]
    SecretSink { key: String, message: String },
}

/// Imports the current .NET `settings.json` without overwriting Rust settings.
///
/// The source is copied once to a sibling `.mkvo-backup` file. Credentials are
/// handed to the supplied sink (normally the OS credential store) and are never
/// written to SQLite. The legacy SQLite metadata cache is deliberately ignored
/// because it is safe and more accurate to rebuild it with Rust probe versions.
pub fn import_legacy_settings<F, E>(
    store: &SqliteStore,
    source: &Path,
    mut secret_sink: F,
) -> Result<LegacyImportOutcome, LegacyImportError>
where
    F: FnMut(&str, &str) -> Result<(), E>,
    E: std::fmt::Display,
{
    if !source.is_file() {
        return Ok(LegacyImportOutcome::SourceMissing);
    }
    if store
        .get_setting_with_revision::<AppSettings>("app")?
        .is_some()
    {
        return Ok(LegacyImportOutcome::SkippedExisting);
    }

    let backup_path = legacy_backup_path(source);
    if !backup_path.exists() {
        let mut input = fs::File::open(source)?;
        let mut backup = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&backup_path)?;
        io::copy(&mut input, &mut backup)?;
        backup.sync_all()?;
    }

    let document: Value = serde_json::from_reader(BufReader::new(fs::File::open(source)?))?;
    let (mut settings, secrets) = convert_legacy_settings(&document);
    for (key, value) in &secrets {
        secret_sink(key, value).map_err(|error| LegacyImportError::SecretSink {
            key: key.clone(),
            message: error.to_string(),
        })?;
    }
    settings = settings.normalized();
    let revision =
        store.save_setting_optimistic("app", settings.schema_version, &settings, None)?;
    Ok(LegacyImportOutcome::Imported {
        backup_path,
        revision,
        secrets_imported: secrets.len(),
        cache_rebuild_required: true,
    })
}

fn legacy_backup_path(source: &Path) -> PathBuf {
    let file_name = source
        .file_name()
        .map_or_else(|| "settings.json".into(), |name| name.to_os_string());
    let mut backup_name = file_name;
    backup_name.push(".mkvo-backup");
    source.with_file_name(backup_name)
}

fn convert_legacy_settings(document: &Value) -> (AppSettings, BTreeMap<String, String>) {
    let mut settings = AppSettings::default();
    let mut secrets = BTreeMap::new();

    settings.tools.mkvtoolnix_directory = path(document, "MkvToolNixDirectory");
    settings.tools.ffmpeg_directory = path(document, "FfmpegDirectory");
    settings.tools.mkvmerge_path = path(document, "MkvMergePath");
    settings.tools.mkvpropedit_path = path(document, "MkvPropEditPath");
    settings.tools.ffprobe_path = path(document, "FfProbePath");
    settings.scan.default_root =
        path(document, "RootFolderPath").or_else(|| path(document, "LastFolderPath"));
    if let Some(values) = strings(document, "IgnoredScanFolderNames") {
        settings.scan.ignored_folder_names = values.into_iter().collect();
    }
    if let Some(provider) = string(document, "RenameLookupProvider") {
        settings.rename.provider = if provider.eq_ignore_ascii_case("TMDB") {
            MetadataProvider::Tmdb
        } else {
            MetadataProvider::Tvdb
        };
    }
    if let Some(language) = string(document, "TvdbLanguage") {
        settings.rename.language = language;
    }
    if let Some(template) = string(document, "RenameTemplate") {
        settings.rename.template = template;
    }
    if let Some(templates) = strings(document, "RenameTemplates") {
        settings.rename.templates = templates;
    }
    if let Some(compact) = boolean(document, "RenamePreviewCompactView") {
        settings.rename.compact_preview = compact;
    }
    if let Some(roots) = strings(document, "WatchFolders") {
        settings.watch.roots = roots.into_iter().map(PathBuf::from).collect();
    }
    settings.watch.enabled = boolean(document, "EnableLiveWatchFolderMonitoring").unwrap_or(false);
    if let Some(theme) = string(document, "SelectedThemeName") {
        settings.appearance.selected_theme = theme;
    }
    if let Some(themes) = property(document, "CustomThemes").and_then(Value::as_array) {
        settings.appearance.custom_themes = themes
            .iter()
            .filter_map(|theme| {
                let name = string(theme, "Name")?;
                let colors = property(theme, "Colors")
                    .and_then(Value::as_object)
                    .map(|colors| {
                        colors
                            .iter()
                            .filter_map(|(key, value)| {
                                value.as_str().map(|value| (key.clone(), value.to_owned()))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                Some(ThemeDefinition { name, colors })
            })
            .collect();
    }
    if let Some(workers) = property(document, "Workers") {
        settings.workers.max_scan_workers = unsigned(workers, "MaxScanWorkers")
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or(settings.workers.max_scan_workers);
        settings.workers.max_edit_workers = unsigned(workers, "MaxEditWorkers")
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or(settings.workers.max_edit_workers);
        settings.workers.max_remux_workers = unsigned(workers, "MaxRemuxWorkers")
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or(settings.workers.max_remux_workers);
    }

    import_provider_secret(
        document,
        "TvdbApiKey",
        "secret://providers/tvdb/api-key",
        "secret://providers/tvdb",
        MetadataProvider::Tvdb,
        &mut settings,
        &mut secrets,
    );
    if let Some(pin) = string(document, "TvdbPin").filter(|value| !value.is_empty()) {
        secrets.insert("secret://providers/tvdb/pin".to_owned(), pin);
    }
    import_provider_secret(
        document,
        "TmdbApiKey",
        "secret://providers/tmdb/api-key",
        "secret://providers/tmdb",
        MetadataProvider::Tmdb,
        &mut settings,
        &mut secrets,
    );

    if let Some(servers) = property(document, "MediaServers").and_then(Value::as_array) {
        settings.media_servers = servers
            .iter()
            .filter_map(|server| convert_media_server(server, &mut secrets))
            .collect();
    }
    if let Some(mappings) = property(document, "MediaServerPathMappings").and_then(Value::as_array)
    {
        settings.media_server_path_mappings = mappings
            .iter()
            .filter_map(|mapping| {
                Some(PathMapping {
                    remote_prefix: PathBuf::from(string(mapping, "ServerPathPrefix")?),
                    local_prefix: PathBuf::from(string(mapping, "ContainerPathPrefix")?),
                })
            })
            .collect();
    }
    (settings, secrets)
}

fn import_provider_secret(
    document: &Value,
    legacy_key: &str,
    secret_destination: &str,
    secret_reference: &str,
    provider: MetadataProvider,
    settings: &mut AppSettings,
    secrets: &mut BTreeMap<String, String>,
) {
    if let Some(secret) = string(document, legacy_key).filter(|value| !value.is_empty()) {
        secrets.insert(secret_destination.to_owned(), secret);
        settings.providers.configured.insert(
            provider,
            CredentialState {
                configured: true,
                masked_hint: Some("configured".to_owned()),
                secret_reference: Some(secret_reference.to_owned()),
            },
        );
    }
}

fn convert_media_server(
    value: &Value,
    secrets: &mut BTreeMap<String, String>,
) -> Option<MediaServerSettings> {
    let id_text = string(value, "Id")?;
    let id = MediaServerId::from_str(&id_text).unwrap_or_default();
    let secret_key = format!("secret://media-servers/{id}/api-key");
    let secret_reference = format!("secret://media-servers/{id}");
    let api_key = string(value, "ApiKey").unwrap_or_default();
    if !api_key.is_empty() {
        secrets.insert(secret_key.clone(), api_key);
    }
    let kind = match string(value, "Type")?.to_ascii_lowercase().as_str() {
        "plex" => MediaServerKind::Plex,
        "jellyfin" => MediaServerKind::Jellyfin,
        _ => MediaServerKind::Emby,
    };
    let libraries = property(value, "Libraries")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|library| {
            Some(MediaServerLibrary {
                id: string(library, "Id").unwrap_or_default(),
                name: string(library, "Name").unwrap_or_default(),
                media_type: string(library, "Type").filter(|value| !value.is_empty()),
                server_path: PathBuf::from(string(library, "ServerPath")?),
                local_path: string(library, "ContainerPath")
                    .filter(|value| !value.is_empty())
                    .map(PathBuf::from),
                enabled: boolean(library, "IsEnabled").unwrap_or(true),
            })
        })
        .collect();
    Some(MediaServerSettings {
        id,
        name: string(value, "Name").unwrap_or_default(),
        kind,
        server_url: string(value, "ServerUrl")?,
        credential: CredentialState {
            configured: secrets.contains_key(&secret_key),
            masked_hint: secrets
                .contains_key(&secret_key)
                .then(|| "configured".to_owned()),
            secret_reference: secrets
                .contains_key(&secret_key)
                .then_some(secret_reference),
        },
        is_default: boolean(value, "IsDefault").unwrap_or(false),
        libraries,
        last_synced_at: string(value, "LastSyncedUtc")
            .and_then(|value| chrono::DateTime::parse_from_rfc3339(&value).ok())
            .map(|value| value.with_timezone(&chrono::Utc)),
    })
}

fn string(value: &Value, key: &str) -> Option<String> {
    property(value, key)?
        .as_str()
        .map(|value| value.trim().to_owned())
}

/// The legacy settings file stores unset paths as `""`. Importing those as
/// configured paths would pin a tool to a path that cannot exist, and explicit
/// tool paths deliberately never fall back to directory or `PATH` search.
fn path(value: &Value, key: &str) -> Option<PathBuf> {
    string(value, key)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn strings(value: &Value, key: &str) -> Option<Vec<String>> {
    Some(
        property(value, key)?
            .as_array()?
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .collect(),
    )
}

fn boolean(value: &Value, key: &str) -> Option<bool> {
    property(value, key)?.as_bool()
}

fn unsigned(value: &Value, key: &str) -> Option<u64> {
    property(value, key)?.as_u64()
}

fn property<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    value
        .as_object()?
        .iter()
        .find_map(|(candidate, value)| candidate.eq_ignore_ascii_case(key).then_some(value))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct LegacyRenameBatchRecord {
    pub id: String,
    pub created_at: String,
    #[serde(default)]
    pub undone_at: Option<String>,
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub template: String,
    #[serde(default)]
    pub total_files: usize,
    #[serde(default)]
    pub entries: Vec<LegacyRenameBatchEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct LegacyRenameBatchEntry {
    pub original_path: PathBuf,
    pub renamed_path: PathBuf,
}

/// Read-only compatibility for the old JSON history. Rust-native history adds
/// fingerprints that the legacy format cannot faithfully reconstruct.
pub fn read_legacy_rename_history(
    source: &Path,
) -> Result<Vec<LegacyRenameBatchRecord>, LegacyImportError> {
    if !source.is_file() {
        return Ok(Vec::new());
    }
    Ok(serde_json::from_reader(BufReader::new(fs::File::open(
        source,
    )?))?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos();
            let path =
                std::env::temp_dir().join(format!("mkvo-legacy-{}-{stamp}", std::process::id()));
            fs::create_dir_all(&path).expect("test directory");
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn imports_once_backs_up_and_hands_off_secrets() {
        let directory = TestDirectory::new();
        let source = directory.0.join("settings.json");
        fs::write(
            &source,
            r#"{"RootFolderPath":"/library","TvdbApiKey":"secret","RenameLookupProvider":"TVDB","Workers":{"MaxScanWorkers":6}}"#,
        )
        .expect("settings");
        let store = SqliteStore::open(directory.0.join("mkvo.db")).expect("store");
        let mut imported = BTreeMap::new();
        let outcome = import_legacy_settings(&store, &source, |key, value| {
            imported.insert(key.to_owned(), value.to_owned());
            Ok::<_, io::Error>(())
        })
        .expect("import");
        let LegacyImportOutcome::Imported {
            backup_path,
            cache_rebuild_required,
            ..
        } = outcome
        else {
            panic!("expected import")
        };
        assert!(backup_path.is_file());
        assert!(cache_rebuild_required);
        assert_eq!(imported["secret://providers/tvdb/api-key"], "secret");
        let (_, _, settings) = store
            .get_setting_with_revision::<AppSettings>("app")
            .expect("read")
            .expect("settings");
        assert_eq!(settings.workers.max_scan_workers, 6);
        assert_eq!(
            import_legacy_settings(&store, &source, |_, _| Ok::<_, io::Error>(()))
                .expect("second import"),
            LegacyImportOutcome::SkippedExisting
        );
    }

    #[test]
    fn reads_legacy_history_without_inventing_fingerprints() {
        let directory = TestDirectory::new();
        let source = directory.0.join("rename_history.json");
        fs::write(
            &source,
            r#"[{"Id":"b1","CreatedAt":"2025-01-01T00:00:00","TotalFiles":1,"Entries":[{"OriginalPath":"old.mkv","RenamedPath":"new.mkv"}]}]"#,
        )
        .expect("history");
        let rows = read_legacy_rename_history(&source).expect("read history");
        assert_eq!(rows[0].entries[0].renamed_path, PathBuf::from("new.mkv"));
    }

    #[test]
    fn executes_full_legacy_settings_import_fixture() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../tests/parity-fixtures/settings-legacy-import.json"
        ))
        .expect("legacy settings fixture JSON");
        let cases = fixture["cases"].as_array().expect("fixture cases");
        assert_eq!(cases.len(), 1, "fixture case count changed");
        let case = &cases[0];
        let directory = TestDirectory::new();
        let source = directory.0.join("settings.json");
        fs::write(
            &source,
            serde_json::to_vec_pretty(&case["input"]).expect("serialize input"),
        )
        .expect("write settings fixture");
        let store = SqliteStore::open(directory.0.join("mkvo.db")).expect("store");
        let mut secrets = BTreeMap::new();
        let outcome = import_legacy_settings(&store, &source, |destination, source_token| {
            secrets.insert(destination.to_owned(), source_token.to_owned());
            Ok::<_, io::Error>(())
        })
        .expect("fixture import");
        let LegacyImportOutcome::Imported {
            backup_path,
            secrets_imported,
            cache_rebuild_required,
            ..
        } = outcome
        else {
            panic!("fixture should import")
        };
        assert!(backup_path.is_file(), "backup is required before commit");
        assert!(cache_rebuild_required);
        assert_eq!(secrets_imported, 4);

        let (_, _, settings) = store
            .get_setting_with_revision::<AppSettings>("app")
            .expect("read settings")
            .expect("stored settings");
        assert_eq!(
            serde_json::to_value(&settings).expect("serialize settings"),
            case["expected"]["publicSettings"]
        );
        assert!(
            !serde_json::to_string(&settings)
                .expect("serialize settings")
                .contains("${SECRET:"),
            "public settings must never expose legacy secret tokens"
        );
        let expected_secrets = case["expected"]["secretWrites"]
            .as_array()
            .expect("secret writes")
            .iter()
            .map(|write| {
                (
                    write["destination"].as_str().unwrap().to_owned(),
                    write["sourceToken"].as_str().unwrap().to_owned(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        assert_eq!(secrets, expected_secrets);

        let mut lowercase_input = case["input"].clone();
        let object = lowercase_input.as_object_mut().expect("settings object");
        let root = object.remove("RootFolderPath").expect("root path");
        object.insert("rootfolderpath".to_owned(), root);
        let (settings, _) = convert_legacy_settings(&lowercase_input);
        assert_eq!(
            settings.scan.default_root,
            Some(PathBuf::from("${ROOT}/Library")),
            "legacy property names remain case-insensitive"
        );
    }
}
