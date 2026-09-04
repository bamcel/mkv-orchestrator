use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};

use axum::{
    Json, Router,
    extract::{ConnectInfo, Request, State, rejection::JsonRejection},
    http::{HeaderMap, HeaderValue, header},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;
use tracing::warn;

const COOKIE_NAME: &str = "mkvo_session";

pub fn public_router(auth: AuthState) -> Router {
    Router::new()
        .route("/api/auth/status", get(auth_status))
        .route("/api/auth/login", post(auth_login))
        .route("/api/auth/logout", post(auth_logout))
        .layer(axum::middleware::from_fn(no_store))
        .with_state(auth)
}

pub fn protected_router(auth: AuthState) -> Router {
    Router::new()
        .route("/api/auth/activity", post(auth_activity))
        .route(
            "/api/security/settings",
            get(security_settings).put(update_security_settings),
        )
        .layer(axum::middleware::from_fn(no_store))
        .with_state(auth)
}

async fn no_store(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

#[derive(Serialize)]
struct AuthStatus {
    username: String,
    authenticated: bool,
    password_required: bool,
    idle_timeout_minutes: Option<u32>,
}

fn status(auth: &AuthState, peer: Option<SocketAddr>, authenticated: bool) -> AuthStatus {
    let password_required = auth.password_required(peer);
    AuthStatus {
        username: auth.username().to_owned(),
        authenticated,
        password_required,
        idle_timeout_minutes: if password_required {
            auth.security_settings().idle_timeout_minutes
        } else {
            None
        },
    }
}

fn peer(request: &Request) -> Option<SocketAddr> {
    request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|info| info.0)
}

async fn auth_status(State(auth): State<AuthState>, request: Request) -> Json<AuthStatus> {
    Json(status(
        &auth,
        peer(&request),
        !auth.password_required(peer(&request)) || auth.authenticated(request.headers()),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LoginRequest {
    username: String,
    password: String,
}

fn error(code: axum::http::StatusCode, detail: &str) -> Response {
    (code, Json(serde_json::json!({ "detail": detail }))).into_response()
}

async fn auth_login(
    State(auth): State<AuthState>,
    peer: Option<axum::Extension<ConnectInfo<SocketAddr>>>,
    input: Result<Json<LoginRequest>, JsonRejection>,
) -> Response {
    let Ok(Json(input)) = input else {
        return error(
            axum::http::StatusCode::BAD_REQUEST,
            "Username and password are required.",
        );
    };
    let Some((_, cookie)) = auth.login(&input.username, &input.password) else {
        return error(
            axum::http::StatusCode::UNAUTHORIZED,
            "Incorrect username or password.",
        );
    };
    (
        [(header::SET_COOKIE, cookie)],
        Json(status(&auth, peer.map(|info| info.0.0), true)),
    )
        .into_response()
}

async fn auth_logout(State(auth): State<AuthState>, request: Request) -> Response {
    (
        [(header::SET_COOKIE, auth.logout(request.headers()))],
        Json(status(&auth, peer(&request), false)),
    )
        .into_response()
}

pub async fn require_auth(State(auth): State<AuthState>, request: Request, next: Next) -> Response {
    if !auth.password_required(peer(&request)) || auth.authenticated(request.headers()) {
        next.run(request).await
    } else {
        unauthorized()
    }
}

async fn auth_activity(State(auth): State<AuthState>, request: Request) -> Response {
    if !auth.password_required(peer(&request)) || auth.activity(request.headers()) {
        axum::http::StatusCode::NO_CONTENT.into_response()
    } else {
        unauthorized()
    }
}

async fn security_settings(State(auth): State<AuthState>) -> Json<SecuritySettings> {
    Json(auth.security_settings())
}

async fn update_security_settings(
    State(auth): State<AuthState>,
    input: Result<Json<SecuritySettings>, JsonRejection>,
) -> Response {
    let Ok(Json(settings)) = input else {
        return error(
            axum::http::StatusCode::BAD_REQUEST,
            "Invalid security settings.",
        );
    };
    if let Err(detail) = settings.validate() {
        return error(axum::http::StatusCode::BAD_REQUEST, detail);
    }
    if let Err(cause) = auth.set_security_settings(settings.clone()) {
        tracing::error!(%cause, "could not save security settings");
        return error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Could not save security settings.",
        );
    }
    Json(settings).into_response()
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SecuritySettings {
    pub idle_timeout_minutes: Option<u32>,
    pub local_network_bypass: bool,
}

impl SecuritySettings {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self
            .idle_timeout_minutes
            .is_some_and(|minutes| !(1..=1440).contains(&minutes))
        {
            return Err("Inactivity timeout must be between 1 and 1440 minutes.");
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct AuthState {
    auth_enabled: bool,
    username: Arc<str>,
    password: Option<Arc<str>>,
    sessions: Arc<RwLock<HashMap<String, Instant>>>,
    secure_cookie: bool,
    settings: Arc<RwLock<SecuritySettings>>,
    settings_path: Option<PathBuf>,
}

impl AuthState {
    pub fn load(
        data_dir: &Path,
        configured_password: Option<&str>,
        username: &str,
        secure_cookie: bool,
    ) -> std::io::Result<Self> {
        if username.trim().is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Username cannot be empty",
            ));
        }
        std::fs::create_dir_all(data_dir)?;
        let settings_path = data_dir.join("security-settings.json");
        let settings: SecuritySettings = match std::fs::read(&settings_path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(std::io::Error::other)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                SecuritySettings::default()
            }
            Err(error) => return Err(error),
        };
        settings.validate().map_err(std::io::Error::other)?;
        let password = if let Some(password) = configured_password {
            if password.trim().is_empty() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "MKVO_PASSWORD cannot be empty",
                ));
            }
            password.to_owned()
        } else {
            std::fs::create_dir_all(data_dir)?;
            let path = data_dir.join("admin-password.txt");
            if path.exists() {
                std::fs::read_to_string(path)?.trim().to_owned()
            } else {
                let password = format!(
                    "{}{}",
                    uuid::Uuid::new_v4().simple(),
                    uuid::Uuid::new_v4().simple()
                );
                use std::io::Write;
                let mut options = std::fs::OpenOptions::new();
                options.write(true).create_new(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.mode(0o600);
                }
                let mut file = options.open(&path)?;
                writeln!(file, "{password}")?;
                file.sync_all()?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
                }
                warn!(path = %path.display(), "Generated the MKVO administrator password; retrieve it from this file");
                password
            }
        };
        if password.trim().is_empty() {
            return Err(std::io::Error::other(
                "Administrator password file is empty",
            ));
        }
        #[cfg(unix)]
        if configured_password.is_none() {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                data_dir.join("admin-password.txt"),
                std::fs::Permissions::from_mode(0o600),
            )?;
        }
        Ok(Self {
            auth_enabled: true,
            username: username.trim().into(),
            password: Some(password.into()),
            sessions: Arc::new(RwLock::new(HashMap::new())),
            secure_cookie,
            settings: Arc::new(RwLock::new(settings)),
            settings_path: Some(settings_path),
        })
    }

    #[cfg(test)]
    pub fn for_tests(password: &str) -> Self {
        Self {
            auth_enabled: true,
            username: "admin".into(),
            password: (!password.is_empty()).then(|| Arc::from(password.to_owned())),
            sessions: Arc::new(RwLock::new(HashMap::new())),
            secure_cookie: false,
            settings: Arc::new(RwLock::new(SecuritySettings::default())),
            settings_path: None,
        }
    }

    pub fn authenticated(&self, headers: &HeaderMap) -> bool {
        if !self.auth_enabled || self.password.is_none() {
            return true;
        }
        let Some(token) = cookie(headers, COOKIE_NAME) else {
            return false;
        };
        let timeout = self.security_settings().idle_timeout_minutes;
        let Ok(mut sessions) = self.sessions.write() else {
            return false;
        };
        sessions.retain(|_, last_activity| !expired(*last_activity, timeout));
        sessions.contains_key(token)
    }

    pub fn security_settings(&self) -> SecuritySettings {
        self.settings
            .read()
            .expect("security settings lock poisoned")
            .clone()
    }

    pub fn with_authentication(mut self, enabled: bool) -> Self {
        self.auth_enabled = enabled;
        if !enabled {
            warn!(
                "Administrator login is disabled: all connections, including reverse proxies, have full access"
            );
        }
        self
    }

    pub fn password_required(&self, peer: Option<SocketAddr>) -> bool {
        self.auth_enabled && !self.local_bypass(peer)
    }

    pub fn username(&self) -> &str {
        &self.username
    }

    pub fn set_security_settings(&self, settings: SecuritySettings) -> std::io::Result<()> {
        settings.validate().map_err(std::io::Error::other)?;
        let mut current = self
            .settings
            .write()
            .map_err(|_| std::io::Error::other("security settings lock poisoned"))?;
        if let Some(path) = &self.settings_path {
            let temp = path.with_extension("tmp");
            std::fs::write(&temp, serde_json::to_vec_pretty(&settings)?)?;
            std::fs::rename(&temp, path)?;
        }
        *current = settings;
        Ok(())
    }

    // Deliberately use the TCP peer, never client-controlled Forwarded headers.
    // A private reverse proxy therefore qualifies when this opt-in is enabled.
    pub fn local_bypass(&self, peer: Option<SocketAddr>) -> bool {
        self.security_settings().local_network_bypass
            && peer.is_some_and(|peer| is_local(peer.ip()))
    }

    pub fn activity(&self, headers: &HeaderMap) -> bool {
        let timeout = self.security_settings().idle_timeout_minutes;
        let Some(token) = cookie(headers, COOKIE_NAME) else {
            return false;
        };
        let Ok(mut sessions) = self.sessions.write() else {
            return false;
        };
        sessions.retain(|_, last_activity| !expired(*last_activity, timeout));
        if let Some(last_activity) = sessions.get_mut(token) {
            *last_activity = Instant::now();
            return true;
        }
        false
    }

    pub fn login(&self, username: &str, candidate: &str) -> Option<(String, HeaderValue)> {
        let password = self.password.as_deref()?;
        let credentials_match = self.username.as_bytes().ct_eq(username.as_bytes())
            & password.as_bytes().ct_eq(candidate.as_bytes());
        if credentials_match.unwrap_u8() != 1 {
            return None;
        }
        let token = uuid::Uuid::new_v4().simple().to_string();
        let timeout = self.security_settings().idle_timeout_minutes;
        let mut sessions = self.sessions.write().ok()?;
        sessions.retain(|_, last_activity| !expired(*last_activity, timeout));
        sessions.insert(token.clone(), Instant::now());
        let secure = if self.secure_cookie { "; Secure" } else { "" };
        let value = HeaderValue::from_str(&format!(
            "{COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Strict{secure}"
        ))
        .ok()?;
        Some((token, value))
    }

    pub fn logout(&self, headers: &HeaderMap) -> HeaderValue {
        if let Some(token) = cookie(headers, COOKIE_NAME)
            && let Ok(mut sessions) = self.sessions.write()
        {
            sessions.remove(token);
        }
        HeaderValue::from_static("mkvo_session=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0")
    }
}

fn expired(last_activity: Instant, timeout: Option<u32>) -> bool {
    timeout.is_some_and(|minutes| {
        last_activity.elapsed() >= Duration::from_secs(u64::from(minutes) * 60)
    })
}

fn is_local(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => ip.is_private() || ip.is_loopback() || ip.is_link_local(),
        IpAddr::V6(ip) => ip
            .to_ipv4_mapped()
            .map(|ip| is_local(IpAddr::V4(ip)))
            .unwrap_or_else(|| {
                ip.is_loopback() || ip.is_unique_local() || ip.is_unicast_link_local()
            }),
    }
}

pub fn unauthorized() -> Response {
    (
        axum::http::StatusCode::UNAUTHORIZED,
        [
            (header::CONTENT_TYPE, "application/json"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        r#"{"detail":"Authentication required."}"#,
    )
        .into_response()
}

fn cookie<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .filter_map(|part| part.trim().split_once('='))
        .find_map(|(key, value)| (key == name).then_some(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabling_login_preserves_credentials_and_sessions_do_not_survive_restart() {
        let directory = tempfile::tempdir().unwrap();
        let auth = AuthState::load(directory.path(), Some("password"), "admin", true).unwrap();
        let (_, cookie) = auth.login("admin", "password").unwrap();
        assert!(cookie.to_str().unwrap().ends_with("; Secure"));
        let mut headers = HeaderMap::new();
        headers.insert(header::COOKIE, cookie);
        let disabled = auth.clone().with_authentication(false);
        assert!(disabled.authenticated(&HeaderMap::new()));
        assert!(!disabled.password_required(None));
        let enabled = disabled.with_authentication(true);
        assert!(enabled.login("admin", "password").is_some());
        let restarted = AuthState::load(directory.path(), Some("password"), "admin", true).unwrap();
        assert!(!restarted.authenticated(&headers));
    }

    #[test]
    fn failed_settings_write_does_not_change_effective_policy() {
        let directory = tempfile::tempdir().unwrap();
        let auth = AuthState::load(directory.path(), Some("password"), "admin", false).unwrap();
        std::fs::create_dir(directory.path().join("security-settings.tmp")).unwrap();
        assert!(
            auth.set_security_settings(SecuritySettings {
                idle_timeout_minutes: Some(30),
                local_network_bypass: true,
            })
            .is_err()
        );
        assert!(!auth.security_settings().local_network_bypass);
        assert_eq!(auth.security_settings().idle_timeout_minutes, None);
    }

    #[test]
    fn login_requires_the_configured_username_and_password() {
        let directory = tempfile::tempdir().unwrap();
        let auth = AuthState::load(directory.path(), Some("password"), "curator", false).unwrap();
        assert!(auth.login("curator", "password").is_some());
        assert!(auth.login("admin", "password").is_none());
        assert!(auth.login("CURATOR", "password").is_none());
        assert!(auth.login("curator", "wrong").is_none());
        assert!(auth.login("", "password").is_none());
        assert!(AuthState::load(directory.path(), Some("password"), " ", false).is_err());
    }

    #[test]
    fn local_bypass_is_opt_in_and_uses_only_local_peers() {
        let auth = AuthState::for_tests("password");
        let lan = Some("192.168.1.5:1234".parse().unwrap());
        assert!(!auth.local_bypass(lan));
        auth.set_security_settings(SecuritySettings {
            local_network_bypass: true,
            idle_timeout_minutes: None,
        })
        .unwrap();
        for address in [
            "192.168.1.5:1234",
            "10.0.0.2:80",
            "172.18.0.5:80",
            "127.0.0.1:80",
            "[::1]:80",
            "[fd00::1]:80",
            "[::ffff:192.168.1.5]:80",
        ] {
            assert!(
                auth.local_bypass(Some(address.parse().unwrap())),
                "{address}"
            );
        }
        for address in ["8.8.8.8:80", "100.64.0.2:80", "[2606:4700::1111]:80"] {
            assert!(
                !auth.local_bypass(Some(address.parse().unwrap())),
                "{address}"
            );
        }
        assert!(!auth.local_bypass(None));
    }

    #[test]
    fn inactivity_expires_sessions_but_background_checks_do_not_extend_them() {
        let auth = AuthState::for_tests("password");
        auth.set_security_settings(SecuritySettings {
            idle_timeout_minutes: Some(1),
            local_network_bypass: false,
        })
        .unwrap();
        let (token, value) = auth.login("admin", "password").unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(header::COOKIE, value);
        let old = Instant::now() - Duration::from_secs(30);
        auth.sessions.write().unwrap().insert(token.clone(), old);
        assert!(auth.authenticated(&headers));
        assert_eq!(auth.sessions.read().unwrap()[&token], old);
        assert!(auth.activity(&headers));
        assert!(auth.sessions.read().unwrap()[&token] > old);
        auth.sessions
            .write()
            .unwrap()
            .insert(token.clone(), Instant::now() - Duration::from_secs(61));
        assert!(!auth.activity(&headers));
        assert!(!auth.authenticated(&headers));
        assert!(!auth.sessions.read().unwrap().contains_key(&token));
    }

    #[test]
    fn disabled_timeout_keeps_sessions_until_logout() {
        let auth = AuthState::for_tests("password");
        let (token, value) = auth.login("admin", "password").unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(header::COOKIE, value);
        auth.sessions
            .write()
            .unwrap()
            .insert(token, Instant::now() - Duration::from_secs(86400));
        assert!(auth.authenticated(&headers));
        auth.logout(&headers);
        assert!(!auth.authenticated(&headers));
    }

    #[test]
    fn security_settings_survive_restarts_and_reject_invalid_values() {
        let directory = tempfile::tempdir().unwrap();
        let auth = AuthState::load(directory.path(), Some("password"), "admin", false).unwrap();
        auth.set_security_settings(SecuritySettings {
            idle_timeout_minutes: Some(20),
            local_network_bypass: true,
        })
        .unwrap();
        let reloaded = AuthState::load(directory.path(), Some("password"), "admin", false).unwrap();
        assert_eq!(reloaded.security_settings().idle_timeout_minutes, Some(20));
        assert!(reloaded.security_settings().local_network_bypass);
        for minutes in [0, 1441] {
            assert!(
                reloaded
                    .set_security_settings(SecuritySettings {
                        idle_timeout_minutes: Some(minutes),
                        local_network_bypass: false
                    })
                    .is_err()
            );
        }
        reloaded
            .set_security_settings(SecuritySettings::default())
            .unwrap();
        let reset = AuthState::load(directory.path(), Some("password"), "admin", false).unwrap();
        assert!(!reset.local_bypass(Some("192.168.1.5:80".parse().unwrap())));
        assert!(reset.login("admin", "password").is_some());
    }

    #[test]
    fn first_run_password_is_persisted_and_reused() {
        let directory = tempfile::tempdir().unwrap();
        let first = AuthState::load(directory.path(), None, "admin", false).unwrap();
        let password =
            std::fs::read_to_string(directory.path().join("admin-password.txt")).unwrap();
        assert!(password.trim().len() >= 32);
        assert!(first.login("admin", password.trim()).is_some());

        let second = AuthState::load(directory.path(), None, "admin", false).unwrap();
        assert!(second.login("admin", password.trim()).is_some());
    }
}
