//! Discovery exercised over real HTTP against a stub media server.
//!
//! The unit tests parse fixture payloads directly, which leaves the parts that
//! only exist on the wire untested: URL construction from a user-entered base,
//! the per-vendor auth header, the JSON/XML content split, and status handling.
//! A stub server covers those without requiring a real Emby, Jellyfin, or Plex
//! instance.

use std::sync::{Arc, Mutex};

use axum::{
    Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::get,
};
use mkvo_infra_media_servers::{
    MediaServerClient, MediaServerConfig, MediaServerDiscoveryClient, MediaServerKind,
    MediaServerPathMapping, SecretString,
};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Default)]
struct Seen {
    headers: Arc<Mutex<Vec<(String, HeaderMap)>>>,
}

impl Seen {
    fn record(&self, path: &str, headers: &HeaderMap) {
        self.headers
            .lock()
            .expect("header log")
            .push((path.to_owned(), headers.clone()));
    }

    fn header_for(&self, path: &str, name: &str) -> Option<String> {
        self.headers
            .lock()
            .expect("header log")
            .iter()
            .find(|(seen, _)| seen == path)
            .and_then(|(_, headers)| headers.get(name).cloned())
            .and_then(|value| value.to_str().ok().map(str::to_owned))
    }
}

async fn start_stub(router: Router) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind stub server");
    let address = listener.local_addr().expect("stub address");
    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    (format!("http://{address}"), handle)
}

fn config(kind: MediaServerKind, url: &str) -> MediaServerConfig {
    MediaServerConfig {
        id: "server-1".to_owned(),
        name: "Stub".to_owned(),
        kind,
        server_url: url.to_owned(),
        api_key: SecretString::new("token-123"),
    }
}

#[tokio::test]
async fn jellyfin_connection_and_libraries_round_trip_over_http() {
    let seen = Seen::default();
    let router = Router::new()
        .route(
            "/System/Info/Public",
            get(|State(seen): State<Seen>, headers: HeaderMap| async move {
                seen.record("/System/Info/Public", &headers);
                axum::Json(serde_json::json!({
                    "ServerName": "Basement Jellyfin",
                    "Version": "10.9.11"
                }))
            }),
        )
        .route(
            "/Library/VirtualFolders",
            get(|State(seen): State<Seen>, headers: HeaderMap| async move {
                seen.record("/Library/VirtualFolders", &headers);
                axum::Json(serde_json::json!([
                    {
                        "Name": "Shows",
                        "ItemId": "lib-1",
                        "CollectionType": "tvshows",
                        "Locations": ["/data/tv"]
                    },
                    {
                        "Name": "Films",
                        "ItemId": "lib-2",
                        "CollectionType": "movies",
                        "Locations": ["/data/movies"]
                    }
                ]))
            }),
        )
        .with_state(seen.clone());

    // A trailing slash is what a user typically pastes; URL joining must not
    // produce a doubled separator or drop the path.
    let (base, server) = start_stub(router).await;
    let client = MediaServerDiscoveryClient::new();
    let config = config(MediaServerKind::Jellyfin, &format!("{base}/"));

    let info = client
        .test_connection(&config, CancellationToken::new())
        .await
        .expect("connection");
    assert_eq!(info.server_name.as_deref(), Some("Basement Jellyfin"));
    assert_eq!(info.version.as_deref(), Some("10.9.11"));

    let mappings = [MediaServerPathMapping {
        server_path_prefix: "/data".to_owned(),
        local_path_prefix: std::path::PathBuf::from("/mnt/media"),
    }];
    let libraries = client
        .discover_libraries(&config, &mappings, CancellationToken::new())
        .await
        .expect("libraries");

    assert_eq!(libraries.len(), 2);
    let shows = libraries
        .iter()
        .find(|library| library.name == "Shows")
        .expect("shows library");
    assert_eq!(shows.server_path, "/data/tv");
    // The server-to-local mapping is what makes a remote path usable locally.
    // The joined remainder uses the host separator on purpose: a Linux media
    // server reporting `/data/tv` must become `D:\Media\tv` for a Windows
    // client, so the expected value is built the same way rather than hardcoded.
    assert_eq!(
        shows.local_path.as_deref(),
        Some(std::path::PathBuf::from("/mnt/media").join("tv").as_path())
    );

    // Jellyfin and Emby authenticate by token header, not query string, so the
    // key must never appear in a URL that could be logged.
    assert_eq!(
        seen.header_for("/Library/VirtualFolders", "x-emby-token")
            .as_deref(),
        Some("token-123")
    );
    server.abort();
}

#[tokio::test]
async fn plex_reads_xml_rather_than_json() {
    let seen = Seen::default();
    let router = Router::new()
        .route(
            "/identity",
            get(|State(seen): State<Seen>, headers: HeaderMap| async move {
                seen.record("/identity", &headers);
                (
                    [("content-type", "application/xml")],
                    r#"<MediaContainer machineIdentifier="abc123" version="1.40.0"/>"#,
                )
            }),
        )
        .route(
            "/library/sections",
            get(|| async {
                (
                    [("content-type", "application/xml")],
                    r#"<MediaContainer>
                         <Directory key="1" type="show" title="Anime">
                           <Location id="1" path="/srv/anime"/>
                         </Directory>
                       </MediaContainer>"#,
                )
            }),
        )
        .with_state(seen.clone());

    let (base, server) = start_stub(router).await;
    let client = MediaServerDiscoveryClient::new();
    let config = config(MediaServerKind::Plex, &base);

    let info = client
        .test_connection(&config, CancellationToken::new())
        .await
        .expect("connection");
    assert_eq!(info.server_name.as_deref(), Some("abc123"));

    let libraries = client
        .discover_libraries(&config, &[], CancellationToken::new())
        .await
        .expect("libraries");
    assert_eq!(libraries.len(), 1);
    assert_eq!(libraries[0].name, "Anime");
    assert_eq!(libraries[0].server_path, "/srv/anime");

    // Plex uses its own token header rather than the Emby one.
    assert_eq!(
        seen.header_for("/identity", "x-plex-token").as_deref(),
        Some("token-123")
    );
    server.abort();
}

#[tokio::test]
async fn an_unauthorized_response_is_reported_rather_than_parsed() {
    let router = Router::new().route(
        "/System/Info/Public",
        get(|| async { (StatusCode::UNAUTHORIZED, "nope").into_response() }),
    );
    let (base, server) = start_stub(router).await;

    let error = MediaServerDiscoveryClient::new()
        .test_connection(
            &config(MediaServerKind::Emby, &base),
            CancellationToken::new(),
        )
        .await
        .expect_err("unauthorized");

    // A wrong API key must say so, not surface as a parse failure.
    assert!(error.to_string().contains("401"), "{error}");
    server.abort();
}

#[tokio::test]
async fn a_malformed_body_is_reported_as_an_invalid_response() {
    let router = Router::new().route(
        "/System/Info/Public",
        get(|| async { ([("content-type", "application/json")], "{ not json") }),
    );
    let (base, server) = start_stub(router).await;

    let error = MediaServerDiscoveryClient::new()
        .test_connection(
            &config(MediaServerKind::Jellyfin, &base),
            CancellationToken::new(),
        )
        .await
        .expect_err("invalid body");

    assert!(error.to_string().contains("invalid response"), "{error}");
    server.abort();
}
