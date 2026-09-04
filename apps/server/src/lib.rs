mod auth;
mod config;
mod error;
mod request_id;
mod routes;

use std::sync::Arc;

use axum::{
    Router,
    extract::{DefaultBodyLimit, Request},
    middleware,
};
use mkvo_runtime::{MkvoRuntime, MkvoRuntimeBuilder, RuntimeResult};
use tower_http::{
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};
use tracing::info_span;

pub use config::ServerConfig;
use request_id::CORRELATION_ID_HEADER;

#[derive(Clone)]
pub(crate) struct AppState {
    pub runtime: Arc<MkvoRuntime>,
}

pub fn build_runtime(config: &ServerConfig) -> RuntimeResult<MkvoRuntime> {
    let mut builder = MkvoRuntimeBuilder::new(&config.media_root, &config.config_dir)
        .app_name("MKV Orchestrator Web")
        .version(env!("CARGO_PKG_VERSION"));
    for (key, value) in &config.provider_secret_overrides {
        builder = builder.secret_override(key.clone(), value.clone());
    }
    for source in &config.source_roots {
        if source.path != config.media_root {
            builder = builder.source_root(source.label.clone(), &source.path);
        }
        builder = builder.authorized_root(&source.path, true);
    }
    builder.build()
}

pub fn build_router(
    config: Arc<ServerConfig>,
    runtime: Arc<MkvoRuntime>,
) -> anyhow::Result<Router> {
    let auth = auth::AuthState::load(
        &config.config_dir,
        config.auth.as_ref().map(|auth| auth.password.as_str()),
        &config.auth_username,
        config.secure_cookies,
    )?
    .with_authentication(config.auth_enabled);
    let state = AppState { runtime };
    let protected = routes::api_router(state)
        .merge(auth::protected_router(auth.clone()))
        .layer(middleware::from_fn_with_state(
            auth.clone(),
            auth::require_auth,
        ));

    Ok(Router::new()
        .route("/api/health", axum::routing::get(routes::health))
        .merge(auth::public_router(auth))
        .merge(protected)
        .fallback_service(
            ServeDir::new(&config.ui_dir)
                .fallback(ServeFile::new(config.ui_dir.join("index.html"))),
        )
        .layer(DefaultBodyLimit::max(config.request_body_limit_bytes))
        .layer(
            TraceLayer::new_for_http().make_span_with(|request: &Request| {
                let correlation_id = request
                    .headers()
                    .get(&CORRELATION_ID_HEADER)
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or("unknown");
                info_span!(
                    "http.request",
                    method = %request.method(),
                    uri = %request.uri(),
                    correlation_id
                )
            }),
        )
        .layer(middleware::from_fn(request_id::attach_correlation_id)))
}

pub async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, fs, net::SocketAddr, sync::Arc};

    use axum::{
        body::Body,
        http::{Request, StatusCode, header},
    };
    use http_body_util::BodyExt;
    use tempfile::TempDir;
    use tower::ServiceExt;

    use super::*;
    use crate::config::{BasicAuth, SourceRoot};

    struct TestHost {
        _directory: TempDir,
        config: Arc<ServerConfig>,
        runtime: Arc<MkvoRuntime>,
    }

    impl TestHost {
        fn new(auth: Option<BasicAuth>) -> Self {
            Self::new_with_provider_secrets(auth, BTreeMap::new())
        }

        fn new_with_provider_secrets(
            auth: Option<BasicAuth>,
            provider_secret_overrides: BTreeMap<String, String>,
        ) -> Self {
            let directory = tempfile::tempdir().unwrap();
            let media_root = directory.path().join("media");
            let config_dir = directory.path().join("config");
            let ui_dir = directory.path().join("ui");
            fs::create_dir_all(&media_root).unwrap();
            fs::create_dir_all(&config_dir).unwrap();
            fs::create_dir_all(&ui_dir).unwrap();
            fs::write(ui_dir.join("index.html"), "<main>MKVO test UI</main>").unwrap();
            let config = Arc::new(ServerConfig {
                bind: "127.0.0.1:0".parse::<SocketAddr>().unwrap(),
                media_root: media_root.clone(),
                source_roots: vec![SourceRoot {
                    label: "media".to_owned(),
                    path: media_root,
                }],
                config_dir,
                ui_dir,
                auth_enabled: auth.is_some(),
                auth_username: auth
                    .as_ref()
                    .map_or("admin", |auth| auth.username.as_str())
                    .to_owned(),
                secure_cookies: false,
                auth,
                provider_secret_overrides,
                request_body_limit_bytes: 1024,
                graceful_shutdown_seconds: 1,
            });
            let runtime = Arc::new(build_runtime(&config).unwrap());
            Self {
                _directory: directory,
                config,
                runtime,
            }
        }

        fn router(&self) -> Router {
            build_router(Arc::clone(&self.config), Arc::clone(&self.runtime)).unwrap()
        }
    }

    #[tokio::test]
    async fn container_provider_secrets_appear_configured_in_web_settings() {
        let host = TestHost::new_with_provider_secrets(
            None,
            BTreeMap::from([
                (
                    "provider.tvdb.api_key".to_owned(),
                    "container-tvdb-key".to_owned(),
                ),
                (
                    "provider.tvdb.pin".to_owned(),
                    "container-tvdb-pin".to_owned(),
                ),
                (
                    "provider.tmdb.api_key".to_owned(),
                    "container-tmdb-key".to_owned(),
                ),
            ]),
        );

        let settings = host.runtime.get_web_settings().await.unwrap();
        assert!(settings.has_tvdb_api_key);
        assert!(settings.has_tvdb_pin);
        assert!(settings.has_tmdb_api_key);
    }

    async fn body_text(response: axum::response::Response) -> String {
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[tokio::test]
    async fn session_login_logout_and_security_http_contract() {
        let host = TestHost::new(Some(BasicAuth {
            username: "curator".to_owned(),
            password: "secret".to_owned(),
        }));
        let router = host.router();
        for path in ["/api/status", "/api/security/settings", "/api/unknown"] {
            let response = router
                .clone()
                .oneshot(Request::get(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{path}");
        }
        let response = router
            .clone()
            .oneshot(Request::get("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let response = router
            .clone()
            .oneshot(
                Request::get("/api/auth/status")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        let status: serde_json::Value = serde_json::from_str(&body_text(response).await).unwrap();
        assert_eq!(status["username"], "curator");
        assert_eq!(status["authenticated"], false);
        assert_eq!(status["password_required"], true);
        assert!(status["idle_timeout_minutes"].is_null());
        assert!(status.get("password").is_none());
        for (username, password) in [
            ("admin", "secret"),
            ("CURATOR", "secret"),
            ("curator", "wrong"),
        ] {
            let response = router
                .clone()
                .oneshot(
                    Request::post("/api/auth/login")
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(Body::from(
                            serde_json::json!({"username": username, "password": password})
                                .to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
            assert_eq!(
                body_text(response).await,
                r#"{"detail":"Incorrect username or password."}"#
            );
        }
        let response = router
            .clone()
            .oneshot(
                Request::post("/api/auth/login")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"username":"curator","password":"secret"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        let cookie = response.headers()[header::SET_COOKIE].clone();
        for invalid in [
            r#"{"idle_timeout_minutes":0,"local_network_bypass":false}"#,
            r#"{"idle_timeout_minutes":1441,"local_network_bypass":false}"#,
            r#"{"idle_timeout_minutes":1.5,"local_network_bypass":false}"#,
            r#"{"idle_timeout_minutes":"30","local_network_bypass":false}"#,
        ] {
            let response = router
                .clone()
                .oneshot(
                    Request::put("/api/security/settings")
                        .header(header::COOKIE, &cookie)
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(Body::from(invalid))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        }
        let response = router
            .clone()
            .oneshot(
                Request::put("/api/security/settings")
                    .header(header::COOKIE, &cookie)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"idle_timeout_minutes":30,"local_network_bypass":false}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let response = router
            .clone()
            .oneshot(
                Request::post("/api/auth/activity")
                    .header(header::COOKIE, &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        let response = router
            .clone()
            .oneshot(
                Request::post("/api/auth/logout")
                    .header(header::COOKIE, &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(
            response.headers()[header::SET_COOKIE]
                .to_str()
                .unwrap()
                .contains("Max-Age=0")
        );
        let response = router
            .clone()
            .oneshot(
                Request::get("/api/status")
                    .header(header::COOKIE, &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let response = router
            .oneshot(
                Request::post("/api/auth/activity")
                    .header(header::COOKIE, &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let persisted: serde_json::Value = serde_json::from_slice(
            &fs::read(host.config.config_dir.join("security-settings.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(persisted["idle_timeout_minutes"], 30);
    }

    #[tokio::test]
    async fn disabled_login_has_no_barrier_and_no_idle_timeout() {
        let host = TestHost::new(None);
        fs::write(
            host.config.config_dir.join("security-settings.json"),
            r#"{"idle_timeout_minutes":1,"local_network_bypass":false}"#,
        )
        .unwrap();
        let router = host.router();
        let response = router
            .clone()
            .oneshot(
                Request::get("/api/auth/status")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status: serde_json::Value = serde_json::from_str(&body_text(response).await).unwrap();
        assert_eq!(status["authenticated"], true);
        assert_eq!(status["password_required"], false);
        assert!(status["idle_timeout_minutes"].is_null());
        let response = router
            .oneshot(Request::get("/api/status").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn lan_bypass_ignores_forwarded_headers_and_fails_closed_without_peer() {
        let host = TestHost::new(Some(BasicAuth {
            username: "admin".to_owned(),
            password: "secret".to_owned(),
        }));
        fs::write(
            host.config.config_dir.join("security-settings.json"),
            r#"{"idle_timeout_minutes":null,"local_network_bypass":true}"#,
        )
        .unwrap();
        let router = host.router();
        for peer in [
            None,
            Some("8.8.8.8:1234"),
            Some("100.64.0.1:1234"),
            Some("192.168.1.2:1234"),
        ] {
            let mut request = Request::get("/api/status")
                .header("X-Forwarded-For", "127.0.0.1")
                .header("Host", "localhost")
                .body(Body::empty())
                .unwrap();
            if let Some(peer) = peer {
                request.extensions_mut().insert(axum::extract::ConnectInfo(
                    peer.parse::<SocketAddr>().unwrap(),
                ));
            }
            let response = router.clone().oneshot(request).await.unwrap();
            assert_eq!(
                response.status(),
                if peer == Some("192.168.1.2:1234") {
                    StatusCode::OK
                } else {
                    StatusCode::UNAUTHORIZED
                }
            );
        }
    }

    #[tokio::test]
    async fn health_is_public_and_has_legacy_contract() {
        let host = TestHost::new(Some(BasicAuth {
            username: "mkvo".to_owned(),
            password: "secret".to_owned(),
        }));
        let response = host
            .router()
            .oneshot(Request::get("/api/health").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_text(response).await, r#"{"status":"ok"}"#);
    }

    #[tokio::test]
    async fn protects_api_and_accepts_valid_session_cookie() {
        let host = TestHost::new(Some(BasicAuth {
            username: "mkvo".to_owned(),
            password: "secret".to_owned(),
        }));
        let router = host.router();
        let unauthorized = router
            .clone()
            .oneshot(Request::get("/api/status").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
        assert!(
            !unauthorized
                .headers()
                .contains_key(header::WWW_AUTHENTICATE)
        );
        assert!(
            body_text(unauthorized)
                .await
                .contains("\"detail\":\"Authentication required.\"")
        );

        let login = router
            .clone()
            .oneshot(
                Request::post("/api/auth/login")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"username":"mkvo","password":"secret"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(login.status(), StatusCode::OK);
        let cookie = login.headers()[header::SET_COOKIE].clone();
        assert!(
            cookie
                .to_str()
                .unwrap()
                .contains("HttpOnly; SameSite=Strict")
        );
        let authorized = router
            .oneshot(
                Request::get("/api/status")
                    .header(header::COOKIE, cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(authorized.status(), StatusCode::OK);
        let status: serde_json::Value = serde_json::from_str(&body_text(authorized).await).unwrap();
        assert_eq!(status["name"], "MKV Orchestrator Web");
        assert!(status["sourceRoots"].is_array());
        assert!(status["tools"].is_array());
        assert!(status["contractVersion"].is_number());
    }

    #[tokio::test]
    async fn unknown_api_route_returns_json_instead_of_spa() {
        let host = TestHost::new(None);
        let response = host
            .router()
            .oneshot(
                Request::get("/api/does-not-exist")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert!(
            response.headers()[header::CONTENT_TYPE]
                .to_str()
                .unwrap()
                .starts_with("application/json")
        );
        let body = body_text(response).await;
        assert!(body.contains("\"code\":\"not_found\""));
        assert!(!body.contains("MKVO test UI"));
    }

    #[tokio::test]
    async fn runtime_errors_use_typed_json_and_status_mapping() {
        let host = TestHost::new(None);
        let request_correlation = "018f0000-0000-7000-8000-000000000999";
        let response = host
            .router()
            .oneshot(
                Request::get("/api/scans/not-a-job-id")
                    .header("x-correlation-id", request_correlation)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let header_correlation = response.headers()["x-correlation-id"]
            .to_str()
            .unwrap()
            .to_owned();
        assert_eq!(header_correlation, request_correlation);
        let error: serde_json::Value = serde_json::from_str(&body_text(response).await).unwrap();
        assert_eq!(error["code"], "invalid_request");
        assert_eq!(error["correlationId"], header_correlation);
        assert_eq!(error["retryable"], false);
    }

    #[tokio::test]
    async fn serves_spa_for_unknown_non_api_paths() {
        let host = TestHost::new(None);
        let response = host
            .router()
            .oneshot(Request::get("/rename").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert!(body_text(response).await.contains("MKVO test UI"));
    }

    #[tokio::test]
    async fn rejects_json_bodies_over_the_configured_limit() {
        let mut host = TestHost::new(None);
        Arc::get_mut(&mut host.config)
            .unwrap()
            .request_body_limit_bytes = 64;
        let body = format!(r#"{{"sourcePath":"{}"}}"#, "a".repeat(256));
        let response = host
            .router()
            .oneshot(
                Request::post("/api/scans")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }
}
