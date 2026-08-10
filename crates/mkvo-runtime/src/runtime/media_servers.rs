use mkvo_domain::MediaServerKind;
use url::Url;

use crate::{RuntimeError, RuntimeResult};

pub(super) fn parse_server_kind(value: &str) -> RuntimeResult<MediaServerKind> {
    match value.trim().to_ascii_lowercase().as_str() {
        "emby" => Ok(MediaServerKind::Emby),
        "jellyfin" => Ok(MediaServerKind::Jellyfin),
        "plex" => Ok(MediaServerKind::Plex),
        _ => Err(RuntimeError::invalid(format!(
            "unknown media server type: {value}"
        ))),
    }
}

pub(super) fn validate_media_server_url(value: &str) -> RuntimeResult<Url> {
    let url = Url::parse(value.trim())
        .map_err(|error| RuntimeError::invalid(format!("invalid media server URL: {error}")))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(RuntimeError::invalid(
            "media server URLs require HTTP(S), a host, and no credentials, query, or fragment",
        ));
    }
    Ok(url)
}

pub(super) fn media_server_urls_equivalent(left: &str, right: &str) -> bool {
    let (Ok(left), Ok(right)) = (
        validate_media_server_url(left),
        validate_media_server_url(right),
    ) else {
        return false;
    };
    left.scheme().eq_ignore_ascii_case(right.scheme())
        && left
            .host_str()
            .zip(right.host_str())
            .is_some_and(|(left, right)| left.eq_ignore_ascii_case(right))
        && left.port_or_known_default() == right.port_or_known_default()
        && left.path().trim_end_matches('/') == right.path().trim_end_matches('/')
}

pub(super) fn server_kind_name(value: MediaServerKind) -> &'static str {
    match value {
        MediaServerKind::Emby => "emby",
        MediaServerKind::Jellyfin => "jellyfin",
        MediaServerKind::Plex => "plex",
    }
}
