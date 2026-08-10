use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::common::{response_json, send_with_retry, year_from_date};
use crate::{
    EpisodeMetadata, MediaKind, MetadataProviderClient, ProviderCredentials, ProviderError,
    ProviderKind, SearchResult, SelectedMedia, normalize_language,
};

const DEFAULT_BASE_URL: &str = "https://api.themoviedb.org/3/";

#[derive(Debug, Clone)]
pub struct TmdbClient {
    client: Client,
    base_url: Url,
}

impl TmdbClient {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
            base_url: Url::parse(DEFAULT_BASE_URL).expect("TMDB base URL is valid"),
        }
    }

    pub fn with_client(client: Client, base_url: Url) -> Self {
        Self { client, base_url }
    }

    async fn get_json(
        &self,
        credentials: &ProviderCredentials,
        path: &str,
        query: &[(&str, String)],
        cancellation: &CancellationToken,
    ) -> Result<Value, ProviderError> {
        if credentials.api_key.is_empty() {
            return Err(ProviderError::MissingCredentials {
                provider: ProviderKind::Tmdb,
            });
        }
        let url = self
            .base_url
            .join(path)
            .map_err(|error| ProviderError::invalid(ProviderKind::Tmdb, error))?;
        let client = self.client.clone();
        let api_key = credentials.api_key.expose().trim().to_owned();
        let query = query.to_vec();
        let response = send_with_retry(ProviderKind::Tmdb, cancellation, move || {
            client
                .get(url.clone())
                .query(&[("api_key", api_key.as_str())])
                .query(&query)
        })
        .await?;
        response_json(ProviderKind::Tmdb, response).await
    }
}

impl Default for TmdbClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl MetadataProviderClient for TmdbClient {
    fn provider(&self) -> ProviderKind {
        ProviderKind::Tmdb
    }

    async fn search(
        &self,
        credentials: &ProviderCredentials,
        query: &str,
        language: &str,
        cancellation: CancellationToken,
    ) -> Result<Vec<SearchResult>, ProviderError> {
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }
        let language = normalize_language(language);
        let document = self
            .get_json(
                credentials,
                "search/multi",
                &[
                    ("query", query.trim().to_owned()),
                    ("language", language),
                    ("include_adult", "false".to_owned()),
                ],
                &cancellation,
            )
            .await?;
        Ok(normalize_search_results(&document))
    }

    async fn episodes(
        &self,
        credentials: &ProviderCredentials,
        selected: &SelectedMedia,
        language: &str,
        cancellation: CancellationToken,
    ) -> Result<Vec<EpisodeMetadata>, ProviderError> {
        let language = normalize_language(language);
        if selected.kind == MediaKind::Movie {
            let document = self
                .get_json(
                    credentials,
                    &format!("movie/{}", selected.id),
                    &[("language", language)],
                    &cancellation,
                )
                .await?;
            return Ok(vec![normalize_movie(selected, &document)]);
        }

        let details = self
            .get_json(
                credentials,
                &format!("tv/{}", selected.id),
                &[("language", language.clone())],
                &cancellation,
            )
            .await?;
        let mut season_numbers: Vec<u32> = details
            .get("seasons")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|season| season.get("season_number").and_then(Value::as_u64))
            .filter_map(|number| u32::try_from(number).ok())
            .collect();
        season_numbers.sort_unstable();
        season_numbers.dedup();

        let mut episodes = Vec::new();
        for season in season_numbers {
            if cancellation.is_cancelled() {
                return Err(ProviderError::Cancelled {
                    provider: ProviderKind::Tmdb,
                });
            }
            let document = self
                .get_json(
                    credentials,
                    &format!("tv/{}/season/{season}", selected.id),
                    &[("language", language.clone())],
                    &cancellation,
                )
                .await?;
            episodes.extend(normalize_season(&document, season));
        }
        Ok(episodes)
    }
}

fn normalize_search_results(document: &Value) -> Vec<SearchResult> {
    document
        .get("results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let media_type = item.get("media_type")?.as_str()?;
            let kind = match media_type {
                "tv" => MediaKind::Series,
                "movie" => MediaKind::Movie,
                _ => return None,
            };
            let id = item.get("id")?.as_u64()?;
            let name_key = if kind == MediaKind::Movie {
                "title"
            } else {
                "name"
            };
            let date_key = if kind == MediaKind::Movie {
                "release_date"
            } else {
                "first_air_date"
            };
            Some(SearchResult {
                provider: ProviderKind::Tmdb,
                id,
                kind,
                name: item.get(name_key)?.as_str()?.to_owned(),
                year: year_from_date(item.get(date_key).and_then(Value::as_str)),
                overview: item
                    .get("overview")
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned),
                database_url: Some(format!(
                    "https://www.themoviedb.org/{}/{id}",
                    if kind == MediaKind::Movie {
                        "movie"
                    } else {
                        "tv"
                    }
                )),
            })
        })
        .collect()
}

fn normalize_season(document: &Value, fallback_season: u32) -> Vec<EpisodeMetadata> {
    document
        .get("episodes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|episode| {
            let season = episode
                .get("season_number")
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())
                .unwrap_or(fallback_season);
            let episode_number = episode
                .get("episode_number")
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())?;
            Some(EpisodeMetadata {
                provider: ProviderKind::Tmdb,
                id: episode
                    .get("id")
                    .and_then(Value::as_u64)
                    .unwrap_or_else(|| u64::from(season) * 10_000 + u64::from(episode_number)),
                season_number: season,
                episode_number,
                absolute_number: None,
                name: episode
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("Untitled")
                    .to_owned(),
                scope_name: if season == 0 {
                    "Specials".to_owned()
                } else {
                    format!("Season {season}")
                },
                air_date: episode
                    .get("air_date")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            })
        })
        .collect()
}

fn normalize_movie(selected: &SelectedMedia, document: &Value) -> EpisodeMetadata {
    EpisodeMetadata {
        provider: ProviderKind::Tmdb,
        id: document
            .get("id")
            .and_then(Value::as_u64)
            .unwrap_or(selected.id),
        season_number: 1,
        episode_number: 1,
        absolute_number: Some(1),
        name: document
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or(&selected.name)
            .to_owned(),
        scope_name: "Movie".to_owned(),
        air_date: document
            .get("release_date")
            .and_then(Value::as_str)
            .map(str::to_owned),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalizes_only_tv_and_movie_results() {
        let results = normalize_search_results(&json!({"results":[
            {"id":1,"media_type":"tv","name":"Show","first_air_date":"2024-01-02"},
            {"id":2,"media_type":"movie","title":"Film","release_date":"2020-03-04"},
            {"id":3,"media_type":"person","name":"Actor"}
        ]}));
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].year, Some(2024));
        assert_eq!(results[1].kind, MediaKind::Movie);
    }

    #[test]
    fn special_season_has_provider_native_scope() {
        let episodes = normalize_season(
            &json!({"episodes":[{"id":4,"season_number":0,"episode_number":1,"name":"Special"}]}),
            0,
        );
        assert_eq!(episodes[0].scope_name, "Specials");
    }
}
