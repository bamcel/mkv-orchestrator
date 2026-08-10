//! Emby, Jellyfin and Plex connection/discovery adapters.

use std::{
    fmt,
    path::{Path, PathBuf},
    time::Duration,
};

mod adapter;

pub use adapter::ConfiguredMediaServerClient;

use async_trait::async_trait;
use reqwest::{Client, RequestBuilder, Response, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use tokio_util::sync::CancellationToken;
use url::Url;

#[derive(Clone, PartialEq, Eq)]
pub struct SecretString(String);

impl SecretString {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretString([REDACTED])")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaServerKind {
    Emby,
    Jellyfin,
    Plex,
}

impl fmt::Display for MediaServerKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Emby => "Emby",
            Self::Jellyfin => "Jellyfin",
            Self::Plex => "Plex",
        })
    }
}

#[derive(Debug, Clone)]
pub struct MediaServerConfig {
    pub id: String,
    pub name: String,
    pub kind: MediaServerKind,
    pub server_url: String,
    pub api_key: SecretString,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaServerPathMapping {
    pub server_path_prefix: String,
    pub local_path_prefix: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveredLibrary {
    pub id: String,
    pub name: String,
    pub collection_type: Option<String>,
    pub server_path: String,
    pub local_path: Option<PathBuf>,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionInfo {
    pub kind: MediaServerKind,
    pub server_name: Option<String>,
    pub version: Option<String>,
}

#[derive(Debug, Error)]
pub enum MediaServerError {
    #[error("server URL is invalid: {0}")]
    InvalidUrl(String),
    #[error("only HTTP and HTTPS media-server URLs are supported")]
    UnsupportedScheme,
    #[error("{kind} request could not be sent: {message}")]
    Network {
        kind: MediaServerKind,
        message: String,
    },
    #[error("{kind} returned HTTP {status}")]
    Status {
        kind: MediaServerKind,
        status: StatusCode,
    },
    #[error("{kind} returned an invalid response: {message}")]
    InvalidResponse {
        kind: MediaServerKind,
        message: String,
    },
    #[error("{0} request was cancelled")]
    Cancelled(MediaServerKind),
}

#[async_trait]
pub trait MediaServerClient: Send + Sync {
    async fn test_connection(
        &self,
        server: &MediaServerConfig,
        cancellation: CancellationToken,
    ) -> Result<ConnectionInfo, MediaServerError>;

    async fn discover_libraries(
        &self,
        server: &MediaServerConfig,
        mappings: &[MediaServerPathMapping],
        cancellation: CancellationToken,
    ) -> Result<Vec<DiscoveredLibrary>, MediaServerError>;
}

#[derive(Debug, Clone)]
pub struct MediaServerDiscoveryClient {
    client: Client,
}

impl MediaServerDiscoveryClient {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(20))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("static media-server HTTP client configuration must build"),
        }
    }

    pub const fn with_client(client: Client) -> Self {
        Self { client }
    }

    async fn get(
        &self,
        server: &MediaServerConfig,
        relative_path: &str,
        cancellation: &CancellationToken,
    ) -> Result<Response, MediaServerError> {
        let url = build_url(&server.server_url, relative_path)?;
        let request = apply_auth(self.client.get(url), server);
        tokio::select! {
            () = cancellation.cancelled() => Err(MediaServerError::Cancelled(server.kind)),
            result = request.send() => {
                let response = result.map_err(|error| MediaServerError::Network {
                    kind: server.kind,
                    message: error.without_url().to_string(),
                })?;
                if response.status().is_success() {
                    Ok(response)
                } else {
                    Err(MediaServerError::Status { kind: server.kind, status: response.status() })
                }
            }
        }
    }
}

impl Default for MediaServerDiscoveryClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl MediaServerClient for MediaServerDiscoveryClient {
    async fn test_connection(
        &self,
        server: &MediaServerConfig,
        cancellation: CancellationToken,
    ) -> Result<ConnectionInfo, MediaServerError> {
        match server.kind {
            MediaServerKind::Emby | MediaServerKind::Jellyfin => {
                let response = self
                    .get(server, "System/Info/Public", &cancellation)
                    .await?;
                let document: Value = response
                    .json()
                    .await
                    .map_err(|error| invalid(server.kind, error))?;
                Ok(ConnectionInfo {
                    kind: server.kind,
                    server_name: json_string(&document, &["ServerName", "serverName", "Name"]),
                    version: json_string(&document, &["Version", "version"]),
                })
            }
            MediaServerKind::Plex => {
                let response = self.get(server, "identity", &cancellation).await?;
                let xml = response
                    .text()
                    .await
                    .map_err(|error| invalid(server.kind, error))?;
                let identity: PlexIdentity =
                    quick_xml::de::from_str(&xml).map_err(|error| invalid(server.kind, error))?;
                Ok(ConnectionInfo {
                    kind: server.kind,
                    server_name: identity.machine_identifier,
                    version: identity.version,
                })
            }
        }
    }

    async fn discover_libraries(
        &self,
        server: &MediaServerConfig,
        mappings: &[MediaServerPathMapping],
        cancellation: CancellationToken,
    ) -> Result<Vec<DiscoveredLibrary>, MediaServerError> {
        let rows = match server.kind {
            MediaServerKind::Emby | MediaServerKind::Jellyfin => {
                let response = self
                    .get(server, "Library/VirtualFolders", &cancellation)
                    .await?;
                let document: Value = response
                    .json()
                    .await
                    .map_err(|error| invalid(server.kind, error))?;
                parse_emby_libraries(&document, server, mappings)
            }
            MediaServerKind::Plex => {
                let response = self.get(server, "library/sections", &cancellation).await?;
                let xml = response
                    .text()
                    .await
                    .map_err(|error| invalid(server.kind, error))?;
                parse_plex_libraries(&xml, server, mappings)?
            }
        };
        Ok(deduplicate_libraries(rows))
    }
}

fn build_url(base: &str, relative_path: &str) -> Result<Url, MediaServerError> {
    let mut normalized = base.trim().trim_end_matches('/').to_owned();
    normalized.push('/');
    let base =
        Url::parse(&normalized).map_err(|error| MediaServerError::InvalidUrl(error.to_string()))?;
    if !matches!(base.scheme(), "http" | "https") {
        return Err(MediaServerError::UnsupportedScheme);
    }
    base.join(relative_path.trim_start_matches('/'))
        .map_err(|error| MediaServerError::InvalidUrl(error.to_string()))
}

fn apply_auth(request: RequestBuilder, server: &MediaServerConfig) -> RequestBuilder {
    if server.api_key.expose().trim().is_empty() {
        return request;
    }
    match server.kind {
        MediaServerKind::Emby | MediaServerKind::Jellyfin => request
            .header("X-Emby-Token", server.api_key.expose().trim())
            .header("X-MediaBrowser-Token", server.api_key.expose().trim()),
        MediaServerKind::Plex => request.header("X-Plex-Token", server.api_key.expose().trim()),
    }
}

fn invalid(kind: MediaServerKind, error: impl fmt::Display) -> MediaServerError {
    MediaServerError::InvalidResponse {
        kind,
        message: error.to_string(),
    }
}

fn json_string(document: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| document.get(*key).and_then(Value::as_str))
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn parse_emby_libraries(
    document: &Value,
    server: &MediaServerConfig,
    mappings: &[MediaServerPathMapping],
) -> Vec<DiscoveredLibrary> {
    document
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|item| {
            let name = json_string(item, &["Name", "name"]).unwrap_or_else(|| "Library".to_owned());
            let collection_type = json_string(item, &["CollectionType", "collectionType"]);
            let mut locations: Vec<String> = ["Locations", "locations"]
                .into_iter()
                .filter_map(|key| item.get(key).and_then(Value::as_array))
                .flatten()
                .filter_map(Value::as_str)
                .filter(|path| !path.trim().is_empty())
                .map(str::to_owned)
                .collect();
            if locations.is_empty()
                && let Some(path) = json_string(item, &["Path", "path"])
            {
                locations.push(path);
            }
            locations.into_iter().map(move |server_path| {
                make_library(
                    server,
                    mappings,
                    name.clone(),
                    collection_type.clone(),
                    server_path,
                )
            })
        })
        .collect()
}

#[derive(Debug, Deserialize)]
#[serde(rename = "MediaContainer")]
struct PlexIdentity {
    #[serde(rename = "@machineIdentifier")]
    machine_identifier: Option<String>,
    #[serde(rename = "@version")]
    version: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename = "MediaContainer")]
struct PlexSections {
    #[serde(rename = "Directory", default)]
    directories: Vec<PlexDirectory>,
}

#[derive(Debug, Deserialize)]
struct PlexDirectory {
    #[serde(rename = "@key")]
    key: Option<String>,
    #[serde(rename = "@title")]
    title: Option<String>,
    #[serde(rename = "@type")]
    kind: Option<String>,
    #[serde(rename = "Location", default)]
    locations: Vec<PlexLocation>,
}

#[derive(Debug, Deserialize)]
struct PlexLocation {
    #[serde(rename = "@path")]
    path: String,
}

fn parse_plex_libraries(
    xml: &str,
    server: &MediaServerConfig,
    mappings: &[MediaServerPathMapping],
) -> Result<Vec<DiscoveredLibrary>, MediaServerError> {
    let sections: PlexSections =
        quick_xml::de::from_str(xml).map_err(|error| invalid(server.kind, error))?;
    Ok(sections
        .directories
        .into_iter()
        .flat_map(|directory| {
            let name = directory
                .title
                .or(directory.key)
                .unwrap_or_else(|| "Library".to_owned());
            let collection_type = directory.kind;
            directory.locations.into_iter().map(move |location| {
                make_library(
                    server,
                    mappings,
                    name.clone(),
                    collection_type.clone(),
                    location.path,
                )
            })
        })
        .collect())
}

fn make_library(
    server: &MediaServerConfig,
    mappings: &[MediaServerPathMapping],
    name: String,
    collection_type: Option<String>,
    server_path: String,
) -> DiscoveredLibrary {
    let hash = blake3::hash(format!("{}\0{}", server.id, server_path).as_bytes());
    DiscoveredLibrary {
        id: hash.to_hex()[..24].to_owned(),
        name,
        collection_type,
        local_path: map_server_path(&server_path, mappings),
        server_path,
        enabled: true,
    }
}

fn deduplicate_libraries(rows: Vec<DiscoveredLibrary>) -> Vec<DiscoveredLibrary> {
    let mut seen = std::collections::HashSet::new();
    let mut rows: Vec<_> = rows
        .into_iter()
        .filter(|row| {
            let key = row
                .local_path
                .as_ref()
                .map_or_else(
                    || row.server_path.clone(),
                    |path| path.to_string_lossy().into_owned(),
                )
                .to_ascii_lowercase();
            seen.insert(key)
        })
        .collect();
    rows.sort_by(|left, right| {
        left.name
            .to_ascii_lowercase()
            .cmp(&right.name.to_ascii_lowercase())
            .then_with(|| left.server_path.cmp(&right.server_path))
    });
    rows
}

pub fn map_server_path(server_path: &str, mappings: &[MediaServerPathMapping]) -> Option<PathBuf> {
    let normalized_path = normalize_server_path(server_path);
    let mapping = mappings
        .iter()
        .filter_map(|mapping| {
            let prefix = normalize_server_path(&mapping.server_path_prefix);
            path_prefix_matches(&normalized_path, &prefix).then_some((mapping, prefix))
        })
        .max_by_key(|(_, prefix)| prefix.len())?;
    let suffix = normalized_path[mapping.1.len()..].trim_start_matches('/');
    let mut local = mapping.0.local_path_prefix.clone();
    for component in suffix.split('/').filter(|component| !component.is_empty()) {
        local.push(component);
    }
    Some(local)
}

fn normalize_server_path(path: &str) -> String {
    let value = path.trim().replace('\\', "/");
    value.trim_end_matches('/').to_owned()
}

fn path_prefix_matches(path: &str, prefix: &str) -> bool {
    let case_insensitive = path.as_bytes().get(1) == Some(&b':')
        || prefix.as_bytes().get(1) == Some(&b':')
        || path.starts_with("//")
        || prefix.starts_with("//");
    if case_insensitive {
        let path = path.to_ascii_lowercase();
        let prefix = prefix.to_ascii_lowercase();
        path == prefix
            || path
                .strip_prefix(&prefix)
                .is_some_and(|suffix| suffix.starts_with('/'))
    } else {
        path == prefix
            || path
                .strip_prefix(prefix)
                .is_some_and(|suffix| suffix.starts_with('/'))
    }
}

pub fn local_path_is_available(path: Option<&Path>) -> bool {
    path.is_some_and(Path::exists)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn server(kind: MediaServerKind) -> MediaServerConfig {
        MediaServerConfig {
            id: "server-1".to_owned(),
            name: "Home".to_owned(),
            kind,
            server_url: "http://localhost:8096".to_owned(),
            api_key: SecretString::new("secret"),
        }
    }

    #[test]
    fn maps_longest_prefix_with_path_boundary() {
        let mappings = vec![
            MediaServerPathMapping {
                server_path_prefix: "/media".to_owned(),
                local_path_prefix: PathBuf::from("/mnt/media"),
            },
            MediaServerPathMapping {
                server_path_prefix: "/media/tv".to_owned(),
                local_path_prefix: PathBuf::from("/mnt/shows"),
            },
        ];
        assert_eq!(
            map_server_path("/media/tv/Show/Episode.mkv", &mappings),
            Some(PathBuf::from("/mnt/shows/Show/Episode.mkv"))
        );
        assert_eq!(map_server_path("/media2/movie.mkv", &mappings), None);
    }

    #[test]
    fn maps_windows_server_paths_case_insensitively() {
        let mappings = vec![MediaServerPathMapping {
            server_path_prefix: r"D:\Media".to_owned(),
            local_path_prefix: PathBuf::from("/data"),
        }];
        assert_eq!(
            map_server_path(r"d:\MEDIA\TV\Show", &mappings),
            Some(PathBuf::from("/data/TV/Show"))
        );
    }

    #[test]
    fn parses_emby_virtual_folders() {
        let rows = parse_emby_libraries(
            &json!([{"Name":"TV","CollectionType":"tvshows","Locations":["/media/tv"]}]),
            &server(MediaServerKind::Jellyfin),
            &[MediaServerPathMapping {
                server_path_prefix: "/media".to_owned(),
                local_path_prefix: PathBuf::from("/mnt"),
            }],
        );
        assert_eq!(rows[0].name, "TV");
        assert_eq!(rows[0].local_path, Some(PathBuf::from("/mnt/tv")));
    }

    #[test]
    fn parses_plex_section_locations() {
        let xml = r#"<MediaContainer size="1"><Directory key="1" type="show" title="TV"><Location id="1" path="/srv/tv" /></Directory></MediaContainer>"#;
        let rows = parse_plex_libraries(xml, &server(MediaServerKind::Plex), &[]).expect("parse");
        assert_eq!(rows[0].server_path, "/srv/tv");
        assert_eq!(rows[0].collection_type.as_deref(), Some("show"));
    }

    #[test]
    fn debug_never_exposes_api_key() {
        let debug = format!("{:?}", server(MediaServerKind::Emby));
        assert!(!debug.contains("secret"));
        assert!(debug.contains("REDACTED"));
    }
}
