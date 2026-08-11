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

pub fn build_router(config: Arc<ServerConfig>, runtime: Arc<MkvoRuntime>) -> Router {
    let state = AppState { runtime };
    let protected = routes::api_router(state)
        .fallback_service(
            ServeDir::new(&config.ui_dir)
                .fallback(ServeFile::new(config.ui_dir.join("index.html"))),
        )
        .layer(DefaultBodyLimit::max(config.request_body_limit_bytes));

    let protected = if let Some(auth) = config.auth.clone() {
        protected.layer(middleware::from_fn_with_state(
            auth,
            auth::require_basic_auth,
        ))
    } else {
        protected
    };

    Router::new()
        .route("/api/health", axum::routing::get(routes::health))
        .merge(protected)
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
        .layer(middleware::from_fn(request_id::attach_correlation_id))
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
            build_router(Arc::clone(&self.config), Arc::clone(&self.runtime))
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
    async fn protects_api_and_accepts_valid_basic_auth() {
        let host = TestHost::new(Some(BasicAuth {
            username: "mkvo".to_owned(),
            password: "secret".to_owned(),
        }));
        let unauthorized = host
            .router()
            .oneshot(Request::get("/api/status").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            unauthorized.headers()[header::WWW_AUTHENTICATE],
            "Basic realm=\"MKV Orchestrator\", charset=\"UTF-8\""
        );
        assert!(
            body_text(unauthorized)
                .await
                .contains("\"code\":\"unauthorized\"")
        );

        let authorized = host
            .router()
            .oneshot(
                Request::get("/api/status")
                    .header(header::AUTHORIZATION, "Basic bWt2bzpzZWNyZXQ=")
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
