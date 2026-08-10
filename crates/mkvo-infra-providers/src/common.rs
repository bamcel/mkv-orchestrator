use std::{fmt, future::Future, time::Duration};

use async_trait::async_trait;
use reqwest::{RequestBuilder, Response, StatusCode};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio_util::sync::CancellationToken;

#[derive(Clone, PartialEq, Eq)]
pub struct SecretString(String);

impl SecretString {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.trim().is_empty()
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretString([REDACTED])")
    }
}

#[derive(Debug, Clone)]
pub struct ProviderCredentials {
    pub api_key: SecretString,
    pub pin: Option<SecretString>,
}

impl ProviderCredentials {
    pub fn api_key(api_key: impl Into<String>) -> Self {
        Self {
            api_key: SecretString::new(api_key),
            pin: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Tvdb,
    Tmdb,
}

impl fmt::Display for ProviderKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Tvdb => "TVDB",
            Self::Tmdb => "TMDB",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
    Series,
    Movie,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchResult {
    pub provider: ProviderKind,
    pub id: u64,
    pub kind: MediaKind,
    pub name: String,
    pub year: Option<u16>,
    pub overview: Option<String>,
    pub database_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectedMedia {
    pub id: u64,
    pub kind: MediaKind,
    pub name: String,
}

impl From<&SearchResult> for SelectedMedia {
    fn from(value: &SearchResult) -> Self {
        Self {
            id: value.id,
            kind: value.kind,
            name: value.name.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EpisodeMetadata {
    pub provider: ProviderKind,
    pub id: u64,
    pub season_number: u32,
    pub episode_number: u32,
    pub absolute_number: Option<u32>,
    pub name: String,
    pub scope_name: String,
    pub air_date: Option<String>,
}

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("{provider} credentials are not configured")]
    MissingCredentials { provider: ProviderKind },
    #[error("{provider} request could not be sent: {message}")]
    Network {
        provider: ProviderKind,
        message: String,
    },
    #[error("{provider} returned HTTP {status}")]
    Status {
        provider: ProviderKind,
        status: StatusCode,
    },
    #[error("{provider} returned an invalid response: {message}")]
    InvalidResponse {
        provider: ProviderKind,
        message: String,
    },
    #[error("{provider} authentication failed")]
    Authentication { provider: ProviderKind },
    #[error("{provider} request was cancelled")]
    Cancelled { provider: ProviderKind },
}

impl ProviderError {
    pub(crate) fn network(provider: ProviderKind, error: reqwest::Error) -> Self {
        // `without_url` prevents query credentials from entering logs or crash reports.
        Self::Network {
            provider,
            message: error.without_url().to_string(),
        }
    }

    pub(crate) fn invalid(provider: ProviderKind, error: impl fmt::Display) -> Self {
        Self::InvalidResponse {
            provider,
            message: error.to_string(),
        }
    }
}

#[async_trait]
pub trait MetadataProviderClient: Send + Sync {
    fn provider(&self) -> ProviderKind;

    async fn search(
        &self,
        credentials: &ProviderCredentials,
        query: &str,
        language: &str,
        cancellation: CancellationToken,
    ) -> Result<Vec<SearchResult>, ProviderError>;

    async fn episodes(
        &self,
        credentials: &ProviderCredentials,
        selected: &SelectedMedia,
        language: &str,
        cancellation: CancellationToken,
    ) -> Result<Vec<EpisodeMetadata>, ProviderError>;
}

pub(crate) async fn send_with_retry<F>(
    provider: ProviderKind,
    cancellation: &CancellationToken,
    mut build: F,
) -> Result<Response, ProviderError>
where
    F: FnMut() -> RequestBuilder,
{
    const MAX_ATTEMPTS: u32 = 3;
    for attempt in 0..MAX_ATTEMPTS {
        let send = build().send();
        let response = tokio::select! {
            () = cancellation.cancelled() => return Err(ProviderError::Cancelled { provider }),
            response = send => response.map_err(|error| ProviderError::network(provider, error))?,
        };
        if response.status().is_success() {
            return Ok(response);
        }

        let retryable = response.status() == StatusCode::TOO_MANY_REQUESTS
            || response.status().is_server_error();
        if !retryable || attempt + 1 == MAX_ATTEMPTS {
            return Err(ProviderError::Status {
                provider,
                status: response.status(),
            });
        }
        let retry_after = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
            .map(Duration::from_secs)
            .unwrap_or_else(|| Duration::from_millis(250 * 2_u64.pow(attempt)))
            .min(Duration::from_secs(10));
        wait_or_cancel(provider, cancellation, tokio::time::sleep(retry_after)).await?;
    }
    unreachable!("retry loop always returns")
}

async fn wait_or_cancel<F>(
    provider: ProviderKind,
    cancellation: &CancellationToken,
    wait: F,
) -> Result<(), ProviderError>
where
    F: Future<Output = ()>,
{
    tokio::pin!(wait);
    tokio::select! {
        () = cancellation.cancelled() => Err(ProviderError::Cancelled { provider }),
        () = &mut wait => Ok(()),
    }
}

pub fn normalize_language(value: &str) -> String {
    let value = value.trim().replace('_', "-").to_ascii_lowercase();
    match value.as_str() {
        "eng" => "en-US".to_owned(),
        "jpn" => "ja-JP".to_owned(),
        "spa" => "es-ES".to_owned(),
        "fre" | "fra" => "fr-FR".to_owned(),
        "ger" | "deu" => "de-DE".to_owned(),
        "en" => "en-US".to_owned(),
        "ja" => "ja-JP".to_owned(),
        "es" => "es-ES".to_owned(),
        "fr" => "fr-FR".to_owned(),
        "de" => "de-DE".to_owned(),
        value if value.len() == 5 && value.as_bytes()[2] == b'-' => {
            format!("{}-{}", &value[..2], value[3..].to_ascii_uppercase())
        }
        _ => "en-US".to_owned(),
    }
}

pub(crate) fn year_from_date(value: Option<&str>) -> Option<u16> {
    value?
        .split('-')
        .next()
        .and_then(|year| year.parse::<u16>().ok())
}

pub(crate) async fn response_json(
    provider: ProviderKind,
    response: Response,
) -> Result<serde_json::Value, ProviderError> {
    response
        .json()
        .await
        .map_err(|error| ProviderError::invalid(provider, error.without_url()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_are_redacted_from_debug() {
        let credentials = ProviderCredentials {
            api_key: SecretString::new("highly-secret-key"),
            pin: Some(SecretString::new("secret-pin")),
        };
        let debug = format!("{credentials:?}");
        assert!(!debug.contains("highly-secret-key"));
        assert!(!debug.contains("secret-pin"));
        assert!(debug.contains("REDACTED"));
    }

    #[test]
    fn normalizes_provider_languages() {
        assert_eq!(normalize_language("eng"), "en-US");
        assert_eq!(normalize_language("ja_jp"), "ja-JP");
        assert_eq!(normalize_language("unknown"), "en-US");
    }
}
