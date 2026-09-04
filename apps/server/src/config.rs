use std::{collections::BTreeMap, net::SocketAddr, path::PathBuf};

use anyhow::{Context, Result, anyhow, bail};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceRoot {
    pub label: String,
    pub path: PathBuf,
}

#[derive(Clone, Eq, PartialEq)]
pub struct BasicAuth {
    pub username: String,
    pub password: String,
}

#[derive(Clone)]
pub struct ServerConfig {
    pub bind: SocketAddr,
    pub media_root: PathBuf,
    pub source_roots: Vec<SourceRoot>,
    pub config_dir: PathBuf,
    pub ui_dir: PathBuf,
    pub auth: Option<BasicAuth>,
    pub auth_enabled: bool,
    pub auth_username: String,
    pub secure_cookies: bool,
    pub provider_secret_overrides: BTreeMap<String, String>,
    pub request_body_limit_bytes: usize,
    pub graceful_shutdown_seconds: u64,
}

impl ServerConfig {
    pub fn from_env() -> Result<Self> {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    pub(crate) fn from_lookup(mut get: impl FnMut(&str) -> Option<String>) -> Result<Self> {
        let bind: SocketAddr = get("MKVO_BIND")
            .unwrap_or_else(|| "127.0.0.1:8080".to_owned())
            .parse()
            .context("MKVO_BIND must be an IP:port socket address")?;
        let media_root =
            PathBuf::from(get("MKVO_MEDIA_ROOT").unwrap_or_else(|| "/media".to_owned()));
        let config_dir =
            PathBuf::from(get("MKVO_CONFIG_DIR").unwrap_or_else(|| "/config".to_owned()));
        let ui_dir = PathBuf::from(get("MKVO_UI_DIR").unwrap_or_else(|| "web/dist".to_owned()));
        let request_body_limit_bytes = parse_positive(
            "MKVO_REQUEST_BODY_LIMIT_BYTES",
            get("MKVO_REQUEST_BODY_LIMIT_BYTES"),
            16 * 1024 * 1024,
        )?;
        let graceful_shutdown_seconds = parse_positive(
            "MKVO_GRACEFUL_SHUTDOWN_SECONDS",
            get("MKVO_GRACEFUL_SHUTDOWN_SECONDS"),
            15,
        )?;

        let source_roots = parse_source_roots(get("MKVO_SOURCE_ROOTS").as_deref(), &media_root)?;
        let legacy_mode = get("MKVO_AUTH_MODE");
        let auth_mode = legacy_mode
            .clone()
            .unwrap_or_else(|| "auto".to_owned())
            .trim()
            .to_ascii_lowercase();
        let new_username = get("MKVO_USERNAME").filter(|value| !value.trim().is_empty());
        let new_password = get("MKVO_PASSWORD").filter(|value| !value.trim().is_empty());
        let legacy_username = get("MKVO_AUTH_USERNAME").filter(|value| !value.trim().is_empty());
        let legacy_password = get("MKVO_AUTH_PASSWORD").filter(|value| !value.trim().is_empty());
        if new_username.is_none()
            && new_password.is_none()
            && legacy_username.is_some() != legacy_password.is_some()
        {
            bail!("MKVO_AUTH_USERNAME and MKVO_AUTH_PASSWORD must be configured together");
        }
        let username = new_username
            .or(legacy_username)
            .unwrap_or_else(|| "admin".to_owned());
        let password = new_password.or(legacy_password);
        let auth_username = username.trim().to_owned();
        let credentials = password.map(|password| BasicAuth {
            username: auth_username.clone(),
            password,
        });
        let enabled_override = parse_bool("MKVO_AUTH_ENABLED", get("MKVO_AUTH_ENABLED"))?;
        let secure_cookies =
            parse_bool("MKVO_SECURE_COOKIES", get("MKVO_SECURE_COOKIES"))?.unwrap_or(false);
        let auth_enabled = enabled_override.unwrap_or(auth_mode != "disabled");
        let auth = match auth_mode.as_str() {
            "auto" => {
                if legacy_mode.is_some()
                    && enabled_override.is_none()
                    && !bind.ip().is_loopback()
                    && credentials.is_none()
                {
                    bail!(
                        "MKVO authentication is required for a non-loopback MKVO_BIND; configure credentials or explicitly set MKVO_AUTH_MODE=disabled for a trusted network"
                    );
                }
                credentials
            }
            "basic" if enabled_override.is_none() => Some(credentials.context(
                "MKVO_AUTH_MODE=basic requires MKVO_AUTH_USERNAME and MKVO_AUTH_PASSWORD",
            )?),
            "basic" | "disabled" => credentials,
            _ => bail!("MKVO_AUTH_MODE must be one of: auto, basic, disabled"),
        };
        let provider_secret_overrides = [
            ("MKVO_TVDB_API_KEY", "provider.tvdb.api_key"),
            ("MKVO_TVDB_PIN", "provider.tvdb.pin"),
            ("MKVO_TMDB_API_KEY", "provider.tmdb.api_key"),
        ]
        .into_iter()
        .filter_map(|(variable, key)| {
            get(variable)
                .filter(|value| !value.is_empty())
                .map(|value| (key.to_owned(), value))
        })
        .collect();

        Ok(Self {
            bind,
            media_root,
            source_roots,
            config_dir,
            ui_dir,
            auth,
            auth_enabled,
            auth_username,
            secure_cookies,
            provider_secret_overrides,
            request_body_limit_bytes,
            graceful_shutdown_seconds,
        })
    }
}

fn parse_bool(name: &str, value: Option<String>) -> Result<Option<bool>> {
    match value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        None => Ok(None),
        Some("true") => Ok(Some(true)),
        Some("false") => Ok(Some(false)),
        _ => bail!("{name} must be true or false"),
    }
}

fn parse_positive<T>(name: &str, value: Option<String>, default: T) -> Result<T>
where
    T: std::str::FromStr + PartialOrd + Default,
    T::Err: std::fmt::Display,
{
    let Some(value) = value else {
        return Ok(default);
    };
    let parsed = value
        .parse::<T>()
        .map_err(|error| anyhow!("{name} must be a positive integer: {error}"))?;
    if parsed <= T::default() {
        bail!("{name} must be greater than zero");
    }
    Ok(parsed)
}

fn parse_source_roots(
    value: Option<&str>,
    media_root: &std::path::Path,
) -> Result<Vec<SourceRoot>> {
    let mut roots = BTreeMap::<String, PathBuf>::new();
    roots.insert("media".to_owned(), media_root.to_path_buf());

    for item in value.unwrap_or_default().split([';', ',']) {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }

        let (label, path) = item.split_once('=').with_context(|| {
            format!("invalid MKVO_SOURCE_ROOTS entry '{item}', expected label=/path")
        })?;
        let label = label.trim();
        let path = path.trim();
        if label.is_empty() || path.is_empty() {
            bail!("invalid MKVO_SOURCE_ROOTS entry '{item}', label and path are required");
        }
        roots.insert(label.to_owned(), PathBuf::from(path));
    }

    Ok(roots
        .into_iter()
        .map(|(label, path)| SourceRoot { label, path })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_login_options_override_legacy_without_discarding_credentials() {
        let values = BTreeMap::from([
            ("MKVO_AUTH_MODE", "basic"),
            ("MKVO_AUTH_USERNAME", "legacy"),
            ("MKVO_AUTH_PASSWORD", "old-password"),
            ("MKVO_AUTH_ENABLED", "false"),
        ]);
        let config =
            ServerConfig::from_lookup(|key| values.get(key).map(ToString::to_string)).unwrap();
        assert!(!config.auth_enabled);
        assert_eq!(config.auth_username, "legacy");
        assert_eq!(config.auth.unwrap().password, "old-password");
        let values = BTreeMap::from([
            ("MKVO_AUTH_MODE", "disabled"),
            ("MKVO_AUTH_ENABLED", "true"),
            ("MKVO_USERNAME", "curator"),
            ("MKVO_PASSWORD", "new-password"),
            ("MKVO_SECURE_COOKIES", "true"),
        ]);
        let config =
            ServerConfig::from_lookup(|key| values.get(key).map(ToString::to_string)).unwrap();
        assert!(config.auth_enabled && config.secure_cookies);
        assert_eq!(config.auth_username, "curator");
        assert_eq!(config.auth.unwrap().password, "new-password");
    }

    #[test]
    fn rejects_invalid_boolean_and_blank_username_uses_admin() {
        assert!(
            ServerConfig::from_lookup(|key| (key == "MKVO_AUTH_ENABLED").then(|| "yes".to_owned()))
                .is_err()
        );
        let config =
            ServerConfig::from_lookup(|key| (key == "MKVO_USERNAME").then(|| " ".to_owned()))
                .unwrap();
        assert_eq!(config.auth_username, "admin");
    }

    #[test]
    fn parses_roots_and_auth() {
        let values = BTreeMap::from([
            ("MKVO_MEDIA_ROOT", "D:/media"),
            ("MKVO_SOURCE_ROOTS", "downloads=D:/downloads;anime=E:/anime"),
            ("MKVO_AUTH_USERNAME", "mkvo"),
            ("MKVO_AUTH_PASSWORD", "secret"),
        ]);
        let config =
            ServerConfig::from_lookup(|key| values.get(key).map(ToString::to_string)).unwrap();

        assert_eq!(config.source_roots.len(), 3);
        assert_eq!(config.auth.unwrap().username, "mkvo");
    }

    #[test]
    fn parses_container_provider_secret_overrides() {
        let values = BTreeMap::from([
            ("MKVO_TVDB_API_KEY", "tvdb-key"),
            ("MKVO_TVDB_PIN", "tvdb-pin"),
            ("MKVO_TMDB_API_KEY", "tmdb-key"),
        ]);
        let config =
            ServerConfig::from_lookup(|key| values.get(key).map(ToString::to_string)).unwrap();

        assert_eq!(
            config.provider_secret_overrides["provider.tvdb.api_key"],
            "tvdb-key"
        );
        assert_eq!(
            config.provider_secret_overrides["provider.tvdb.pin"],
            "tvdb-pin"
        );
        assert_eq!(
            config.provider_secret_overrides["provider.tmdb.api_key"],
            "tmdb-key"
        );
    }

    #[test]
    fn rejects_partial_auth() {
        let result = ServerConfig::from_lookup(|key| {
            (key == "MKVO_AUTH_USERNAME").then(|| "mkvo".to_owned())
        });
        assert!(result.is_err());
    }

    #[test]
    fn defaults_to_session_login_on_non_loopback_bind() {
        let result = ServerConfig::from_lookup(|key| {
            (key == "MKVO_BIND").then(|| "0.0.0.0:8080".to_owned())
        });
        let config = result.unwrap();
        assert!(config.auth_enabled);
        assert_eq!(config.auth_username, "admin");
        assert!(config.auth.is_none()); // Startup generates a persistent password.
    }

    #[test]
    fn permits_explicit_unauthenticated_non_loopback_bind() {
        let values = BTreeMap::from([
            ("MKVO_BIND", "0.0.0.0:8080"),
            ("MKVO_AUTH_MODE", "disabled"),
        ]);
        let config =
            ServerConfig::from_lookup(|key| values.get(key).map(ToString::to_string)).unwrap();

        assert!(!config.bind.ip().is_loopback());
        assert!(config.auth.is_none());
    }

    #[test]
    fn basic_mode_requires_credentials() {
        let result =
            ServerConfig::from_lookup(|key| (key == "MKVO_AUTH_MODE").then(|| "basic".to_owned()));
        assert!(result.is_err());
    }

    #[test]
    fn disabled_mode_preserves_credentials() {
        let values = BTreeMap::from([
            ("MKVO_AUTH_MODE", "disabled"),
            ("MKVO_AUTH_USERNAME", "mkvo"),
            ("MKVO_AUTH_PASSWORD", "secret"),
        ]);
        let result = ServerConfig::from_lookup(|key| values.get(key).map(ToString::to_string));
        let config = result.unwrap();
        assert!(!config.auth_enabled);
        assert_eq!(config.auth.unwrap().password, "secret");
    }

    #[test]
    fn rejects_unknown_auth_mode() {
        let result = ServerConfig::from_lookup(|key| {
            (key == "MKVO_AUTH_MODE").then(|| "optional".to_owned())
        });
        assert!(result.is_err());
    }

    #[test]
    fn defaults_to_session_login_on_loopback_bind() {
        let config = ServerConfig::from_lookup(|_| None).unwrap();
        assert!(config.bind.ip().is_loopback());
        assert!(config.auth.is_none());
        assert!(config.auth_enabled);
    }

    #[test]
    fn parses_runtime_limits() {
        let values = BTreeMap::from([
            ("MKVO_REQUEST_BODY_LIMIT_BYTES", "4096"),
            ("MKVO_GRACEFUL_SHUTDOWN_SECONDS", "7"),
        ]);
        let config =
            ServerConfig::from_lookup(|key| values.get(key).map(ToString::to_string)).unwrap();

        assert_eq!(config.request_body_limit_bytes, 4096);
        assert_eq!(config.graceful_shutdown_seconds, 7);
    }

    #[test]
    fn rejects_zero_request_limit() {
        let result = ServerConfig::from_lookup(|key| {
            (key == "MKVO_REQUEST_BODY_LIMIT_BYTES").then(|| "0".to_owned())
        });
        assert!(result.is_err());
    }
}
