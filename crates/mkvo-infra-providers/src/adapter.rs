use async_trait::async_trait;
use chrono::NaiveDate;
use mkvo_application::{MetadataProviderClient as ProviderPort, PortError};
use mkvo_domain::{EpisodeMetadata as DomainEpisode, MetadataProvider, ProviderSearchResult};
use tokio_util::sync::CancellationToken;

use crate::{
    AniDbClient, AniListClient, MediaKind, MetadataProviderClient, ProviderCredentials,
    ProviderError, SearchResult, SelectedMedia, TmdbClient, TvdbClient,
};

#[derive(Debug, Clone)]
pub struct ConfiguredTvdbProvider {
    client: TvdbClient,
    credentials: ProviderCredentials,
    default_language: String,
}

impl ConfiguredTvdbProvider {
    pub fn new(
        client: TvdbClient,
        credentials: ProviderCredentials,
        default_language: impl Into<String>,
    ) -> Self {
        Self {
            client,
            credentials,
            default_language: default_language.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConfiguredTmdbProvider {
    client: TmdbClient,
    credentials: ProviderCredentials,
    default_language: String,
}

impl ConfiguredTmdbProvider {
    pub fn new(
        client: TmdbClient,
        credentials: ProviderCredentials,
        default_language: impl Into<String>,
    ) -> Self {
        Self {
            client,
            credentials,
            default_language: default_language.into(),
        }
    }
}

#[async_trait]
impl ProviderPort for ConfiguredTvdbProvider {
    fn provider(&self) -> MetadataProvider {
        MetadataProvider::Tvdb
    }

    async fn search(
        &self,
        query: &str,
        language: Option<&str>,
        cancel: CancellationToken,
    ) -> Result<Vec<ProviderSearchResult>, PortError> {
        provider_search(
            &self.client,
            &self.credentials,
            query,
            language.unwrap_or(&self.default_language),
            cancel,
        )
        .await
    }

    async fn episodes(
        &self,
        media_id: &str,
        language: Option<&str>,
        cancel: CancellationToken,
    ) -> Result<Vec<DomainEpisode>, PortError> {
        provider_episodes(
            &self.client,
            &self.credentials,
            media_id,
            language.unwrap_or(&self.default_language),
            cancel,
        )
        .await
    }

    async fn test(&self, cancel: CancellationToken) -> Result<(), PortError> {
        MetadataProviderClient::search(
            &self.client,
            &self.credentials,
            "test",
            &self.default_language,
            cancel,
        )
        .await
        .map(|_| ())
        .map_err(provider_port_error)
    }
}

#[async_trait]
impl ProviderPort for ConfiguredTmdbProvider {
    fn provider(&self) -> MetadataProvider {
        MetadataProvider::Tmdb
    }

    async fn search(
        &self,
        query: &str,
        language: Option<&str>,
        cancel: CancellationToken,
    ) -> Result<Vec<ProviderSearchResult>, PortError> {
        provider_search(
            &self.client,
            &self.credentials,
            query,
            language.unwrap_or(&self.default_language),
            cancel,
        )
        .await
    }

    async fn episodes(
        &self,
        media_id: &str,
        language: Option<&str>,
        cancel: CancellationToken,
    ) -> Result<Vec<DomainEpisode>, PortError> {
        provider_episodes(
            &self.client,
            &self.credentials,
            media_id,
            language.unwrap_or(&self.default_language),
            cancel,
        )
        .await
    }

    async fn test(&self, cancel: CancellationToken) -> Result<(), PortError> {
        MetadataProviderClient::search(
            &self.client,
            &self.credentials,
            "test",
            &self.default_language,
            cancel,
        )
        .await
        .map(|_| ())
        .map_err(provider_port_error)
    }
}

#[derive(Debug, Clone)]
pub struct ConfiguredAniListProvider {
    client: AniListClient,
    credentials: ProviderCredentials,
    default_language: String,
}

impl ConfiguredAniListProvider {
    pub fn new(client: AniListClient, default_language: impl Into<String>) -> Self {
        Self {
            client,
            // AniList is public, so a placeholder keeps the credential-carrying
            // client interface uniform without implying a key is configured.
            credentials: ProviderCredentials::api_key(String::new()),
            default_language: default_language.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConfiguredAniDbProvider {
    client: AniDbClient,
    credentials: ProviderCredentials,
    default_language: String,
}

impl ConfiguredAniDbProvider {
    pub fn new(
        client: AniDbClient,
        credentials: ProviderCredentials,
        default_language: impl Into<String>,
    ) -> Self {
        Self {
            client,
            credentials,
            default_language: default_language.into(),
        }
    }
}

#[async_trait]
impl ProviderPort for ConfiguredAniListProvider {
    fn provider(&self) -> MetadataProvider {
        MetadataProvider::AniList
    }

    async fn search(
        &self,
        query: &str,
        language: Option<&str>,
        cancel: CancellationToken,
    ) -> Result<Vec<ProviderSearchResult>, PortError> {
        provider_search(
            &self.client,
            &self.credentials,
            query,
            language.unwrap_or(&self.default_language),
            cancel,
        )
        .await
    }

    async fn episodes(
        &self,
        media_id: &str,
        language: Option<&str>,
        cancel: CancellationToken,
    ) -> Result<Vec<DomainEpisode>, PortError> {
        provider_episodes(
            &self.client,
            &self.credentials,
            media_id,
            language.unwrap_or(&self.default_language),
            cancel,
        )
        .await
    }

    async fn test(&self, cancel: CancellationToken) -> Result<(), PortError> {
        MetadataProviderClient::search(
            &self.client,
            &self.credentials,
            "test",
            &self.default_language,
            cancel,
        )
        .await
        .map(|_| ())
        .map_err(provider_port_error)
    }
}

#[async_trait]
impl ProviderPort for ConfiguredAniDbProvider {
    fn provider(&self) -> MetadataProvider {
        MetadataProvider::AniDb
    }

    async fn search(
        &self,
        query: &str,
        language: Option<&str>,
        cancel: CancellationToken,
    ) -> Result<Vec<ProviderSearchResult>, PortError> {
        provider_search(
            &self.client,
            &self.credentials,
            query,
            language.unwrap_or(&self.default_language),
            cancel,
        )
        .await
    }

    async fn episodes(
        &self,
        media_id: &str,
        language: Option<&str>,
        cancel: CancellationToken,
    ) -> Result<Vec<DomainEpisode>, PortError> {
        provider_episodes(
            &self.client,
            &self.credentials,
            media_id,
            language.unwrap_or(&self.default_language),
            cancel,
        )
        .await
    }

    /// A search exercises only the public title dump, so it verifies reachability
    /// without spending an HTTP API call against the registered client name.
    async fn test(&self, cancel: CancellationToken) -> Result<(), PortError> {
        MetadataProviderClient::search(
            &self.client,
            &self.credentials,
            "test",
            &self.default_language,
            cancel,
        )
        .await
        .map(|_| ())
        .map_err(provider_port_error)
    }
}

async fn provider_search<P: MetadataProviderClient>(
    client: &P,
    credentials: &ProviderCredentials,
    query: &str,
    language: &str,
    cancel: CancellationToken,
) -> Result<Vec<ProviderSearchResult>, PortError> {
    client
        .search(credentials, query, language, cancel)
        .await
        .map(|results| results.into_iter().map(to_domain_search).collect())
        .map_err(provider_port_error)
}

async fn provider_episodes<P: MetadataProviderClient>(
    client: &P,
    credentials: &ProviderCredentials,
    media_id: &str,
    language: &str,
    cancel: CancellationToken,
) -> Result<Vec<DomainEpisode>, PortError> {
    let (kind, id) = decode_media_id(media_id)?;
    client
        .episodes(
            credentials,
            &SelectedMedia {
                id,
                kind,
                name: String::new(),
            },
            language,
            cancel,
        )
        .await
        .map(|episodes| {
            episodes
                .into_iter()
                .map(|episode| DomainEpisode {
                    id: episode.id.to_string(),
                    season: episode.season_number,
                    episode: episode.episode_number,
                    absolute_episode: episode.absolute_number,
                    title: episode.name,
                    aired_at: episode
                        .air_date
                        .as_deref()
                        .and_then(|date| NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()),
                })
                .collect()
        })
        .map_err(provider_port_error)
}

fn to_domain_search(result: SearchResult) -> ProviderSearchResult {
    let provider = match result.provider {
        crate::ProviderKind::Tvdb => MetadataProvider::Tvdb,
        crate::ProviderKind::Tmdb => MetadataProvider::Tmdb,
        crate::ProviderKind::AniDb => MetadataProvider::AniDb,
        crate::ProviderKind::AniList => MetadataProvider::AniList,
    };
    ProviderSearchResult {
        provider,
        id: encode_media_id(result.kind, result.id),
        title: result.name,
        year: result.year,
        overview: result.overview,
    }
}

fn encode_media_id(kind: MediaKind, id: u64) -> String {
    match kind {
        MediaKind::Series => id.to_string(),
        MediaKind::Movie => format!("movie:{id}"),
    }
}

fn decode_media_id(value: &str) -> Result<(MediaKind, u64), PortError> {
    let (kind, value) = value
        .strip_prefix("movie:")
        .map_or((MediaKind::Series, value), |value| {
            (MediaKind::Movie, value)
        });
    let id = value
        .parse()
        .map_err(|_| PortError::InvalidData(format!("invalid provider media id `{value}`")))?;
    Ok((kind, id))
}

fn provider_port_error(error: ProviderError) -> PortError {
    match error {
        ProviderError::Cancelled { .. } => PortError::Canceled,
        ProviderError::Status { status, .. }
            if status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error() =>
        {
            PortError::unavailable(error.to_string(), true)
        }
        ProviderError::Network { .. } => PortError::unavailable(error.to_string(), true),
        ProviderError::MissingCredentials { .. } | ProviderError::Authentication { .. } => {
            PortError::unavailable(error.to_string(), false)
        }
        error => PortError::InvalidData(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn movie_identity_survives_provider_neutral_port() {
        assert_eq!(encode_media_id(MediaKind::Movie, 42), "movie:42");
        assert_eq!(
            decode_media_id("movie:42").expect("id"),
            (MediaKind::Movie, 42)
        );
        assert_eq!(decode_media_id("7").expect("id"), (MediaKind::Series, 7));
    }
}
