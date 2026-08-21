use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use reqwest::Client;
use serde_json::{Value, json};
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::common::{provider_http_client, response_json, send_with_retry, year_from_date};
use crate::{
    EpisodeMetadata, MediaKind, MetadataProviderClient, ProviderCredentials, ProviderError,
    ProviderKind, SearchResult, SecretString, SelectedMedia,
};

const DEFAULT_BASE_URL: &str = "https://api4.thetvdb.com/v4/";

#[derive(Debug)]
struct CachedToken {
    token: SecretString,
    credentials_fingerprint: blake3::Hash,
    expires_at: Instant,
}

#[derive(Debug, Clone)]
pub struct TvdbClient {
    client: Client,
    base_url: Url,
    token: Arc<RwLock<Option<CachedToken>>>,
}

impl TvdbClient {
    pub fn new() -> Self {
        Self {
            client: provider_http_client(),
            base_url: Url::parse(DEFAULT_BASE_URL).expect("TVDB base URL is valid"),
            token: Arc::new(RwLock::new(None)),
        }
    }

    pub fn with_client(client: Client, base_url: Url) -> Self {
        Self {
            client,
            base_url,
            token: Arc::new(RwLock::new(None)),
        }
    }

    async fn bearer(
        &self,
        credentials: &ProviderCredentials,
        cancellation: &CancellationToken,
    ) -> Result<SecretString, ProviderError> {
        if credentials.api_key.is_empty() {
            return Err(ProviderError::MissingCredentials {
                provider: ProviderKind::Tvdb,
            });
        }
        let mut hasher = blake3::Hasher::new();
        hasher.update(credentials.api_key.expose().as_bytes());
        if let Some(pin) = &credentials.pin {
            hasher.update(pin.expose().as_bytes());
        }
        let fingerprint = hasher.finalize();
        if let Some(cached) = self.token.read().await.as_ref()
            && cached.credentials_fingerprint == fingerprint
            && cached.expires_at > Instant::now()
        {
            return Ok(cached.token.clone());
        }

        let url = self
            .base_url
            .join("login")
            .map_err(|error| ProviderError::invalid(ProviderKind::Tvdb, error))?;
        let payload = match credentials.pin.as_ref().filter(|pin| !pin.is_empty()) {
            Some(pin) => json!({"apikey": credentials.api_key.expose(), "pin": pin.expose()}),
            None => json!({"apikey": credentials.api_key.expose()}),
        };
        let client = self.client.clone();
        let response = send_with_retry(ProviderKind::Tvdb, cancellation, move || {
            client.post(url.clone()).json(&payload)
        })
        .await?;
        let document = response_json(ProviderKind::Tvdb, response).await?;
        let token = document
            .pointer("/data/token")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(SecretString::new)
            .ok_or(ProviderError::Authentication {
                provider: ProviderKind::Tvdb,
            })?;
        *self.token.write().await = Some(CachedToken {
            token: token.clone(),
            credentials_fingerprint: fingerprint,
            expires_at: Instant::now() + Duration::from_secs(23 * 60 * 60),
        });
        Ok(token)
    }

    async fn get_json(
        &self,
        credentials: &ProviderCredentials,
        path: &str,
        query: &[(&str, String)],
        cancellation: &CancellationToken,
    ) -> Result<Value, ProviderError> {
        let bearer = self.bearer(credentials, cancellation).await?;
        let url = self
            .base_url
            .join(path)
            .map_err(|error| ProviderError::invalid(ProviderKind::Tvdb, error))?;
        let query = query.to_vec();
        let client = self.client.clone();
        let response = send_with_retry(ProviderKind::Tvdb, cancellation, move || {
            client
                .get(url.clone())
                .bearer_auth(bearer.expose())
                .query(&query)
        })
        .await?;
        response_json(ProviderKind::Tvdb, response).await
    }

    async fn localized_series_name(
        &self,
        credentials: &ProviderCredentials,
        id: u64,
        language: &str,
        cancellation: &CancellationToken,
    ) -> Result<Option<String>, ProviderError> {
        let document = self
            .get_json(
                credentials,
                &format!("series/{id}/extended"),
                &[
                    ("meta", "translations".to_owned()),
                    ("short", "true".to_owned()),
                ],
                cancellation,
            )
            .await?;
        Ok(translation_name(&document, language))
    }
}

impl Default for TvdbClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl MetadataProviderClient for TvdbClient {
    fn provider(&self) -> ProviderKind {
        ProviderKind::Tvdb
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
        let language = tvdb_language(language);
        let mut results = Vec::new();
        for (media_type, kind) in [("series", MediaKind::Series), ("movie", MediaKind::Movie)] {
            let document = self
                .get_json(
                    credentials,
                    "search",
                    &[
                        ("type", media_type.to_owned()),
                        ("query", query.trim().to_owned()),
                        ("language", language.clone()),
                    ],
                    &cancellation,
                )
                .await?;
            results.extend(normalize_search_results(&document, kind));
        }
        for result in &mut results {
            if result.kind != MediaKind::Series {
                continue;
            }
            if let Ok(Some(name)) = self
                .localized_series_name(credentials, result.id, &language, &cancellation)
                .await
            {
                result.name = name;
            }
        }
        Ok(results)
    }

    async fn episodes(
        &self,
        credentials: &ProviderCredentials,
        selected: &SelectedMedia,
        language: &str,
        cancellation: CancellationToken,
    ) -> Result<Vec<EpisodeMetadata>, ProviderError> {
        let language = tvdb_language(language);
        if selected.kind == MediaKind::Movie {
            let document = self
                .get_json(
                    credentials,
                    &format!("movies/{}/extended", selected.id),
                    &[
                        ("meta", "translations".to_owned()),
                        ("short", "true".to_owned()),
                    ],
                    &cancellation,
                )
                .await?;
            return Ok(vec![normalize_movie(selected, &document)]);
        }

        let mut episodes = Vec::new();
        for page in 0..100_u32 {
            let document = self
                .get_json(
                    credentials,
                    &format!("series/{}/episodes/default/{language}", selected.id),
                    &[("page", page.to_string())],
                    &cancellation,
                )
                .await?;
            let page_episodes = normalize_episode_page(&document);
            let has_next = tvdb_has_next_page(&document, page);
            episodes.extend(page_episodes);
            if !has_next {
                break;
            }
        }
        Ok(episodes)
    }
}

fn normalize_search_results(document: &Value, kind: MediaKind) -> Vec<SearchResult> {
    document
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let id = value_u64(item.get("tvdb_id").or_else(|| item.get("id"))?)?;
            let name = item
                .get("name")
                .or_else(|| item.get("primary_name"))
                .and_then(Value::as_str)?;
            Some(SearchResult {
                provider: ProviderKind::Tvdb,
                id,
                kind,
                name: name.to_owned(),
                year: item
                    .get("year")
                    .and_then(|value| {
                        value
                            .as_str()
                            .and_then(|year| year.parse().ok())
                            .or_else(|| value.as_u64().and_then(|year| u16::try_from(year).ok()))
                    })
                    .or_else(|| year_from_date(item.get("first_air_time").and_then(Value::as_str))),
                overview: item
                    .get("overview")
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned),
                database_url: item
                    .get("tvdb_url")
                    .or_else(|| item.get("url"))
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            })
        })
        .collect()
}

fn normalize_episode_page(document: &Value) -> Vec<EpisodeMetadata> {
    document
        .pointer("/data/episodes")
        .or_else(|| document.pointer("/data/series/episodes"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|episode| {
            let season = value_u64(episode.get("seasonNumber")?)
                .and_then(|value| u32::try_from(value).ok())?;
            let episode_number =
                value_u64(episode.get("number")?).and_then(|value| u32::try_from(value).ok())?;
            Some(EpisodeMetadata {
                provider: ProviderKind::Tvdb,
                id: value_u64(episode.get("id")?).unwrap_or(0),
                season_number: season,
                episode_number,
                absolute_number: episode
                    .get("absoluteNumber")
                    .and_then(value_u64)
                    .and_then(|value| u32::try_from(value).ok()),
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
                    .get("aired")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            })
        })
        .collect()
}

fn normalize_movie(selected: &SelectedMedia, document: &Value) -> EpisodeMetadata {
    let data = document.get("data").unwrap_or(document);
    EpisodeMetadata {
        provider: ProviderKind::Tvdb,
        id: data.get("id").and_then(value_u64).unwrap_or(selected.id),
        season_number: 1,
        episode_number: 1,
        absolute_number: Some(1),
        name: data
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(&selected.name)
            .to_owned(),
        scope_name: "Movie".to_owned(),
        air_date: data
            .get("first_release")
            .and_then(Value::as_str)
            .map(str::to_owned),
    }
}

fn tvdb_has_next_page(document: &Value, current_page: u32) -> bool {
    if let Some(next) = document.pointer("/data/links/next") {
        return !next.is_null() && next.as_str().is_some_and(|value| !value.is_empty());
    }
    let page = document.pointer("/data/page").and_then(Value::as_u64);
    let pages = document.pointer("/data/pages").and_then(Value::as_u64);
    match (page, pages) {
        (Some(page), Some(pages)) => page + 1 < pages,
        _ => {
            current_page == 0
                && document
                    .pointer("/data/episodes")
                    .and_then(Value::as_array)
                    .is_some_and(|episodes| !episodes.is_empty())
        }
    }
}

fn tvdb_language(value: &str) -> String {
    let value = value.trim().to_ascii_lowercase();
    match value.as_str() {
        "en" | "en-us" | "eng" => "eng".to_owned(),
        "ja" | "ja-jp" | "jpn" => "jpn".to_owned(),
        "es" | "es-es" | "spa" => "spa".to_owned(),
        "fr" | "fr-fr" | "fra" | "fre" => "fra".to_owned(),
        "de" | "de-de" | "deu" | "ger" => "deu".to_owned(),
        _ => "eng".to_owned(),
    }
}

fn translation_name(document: &Value, language: &str) -> Option<String> {
    document
        .pointer("/data/translations/nameTranslations")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|translation| {
            translation
                .get("language")
                .and_then(Value::as_str)
                .is_some_and(|value| value.eq_ignore_ascii_case(language))
        })
        .and_then(|translation| translation.get("name").and_then(Value::as_str))
        .filter(|name| !name.trim().is_empty())
        .map(str::to_owned)
}

fn value_u64(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| value.as_str()?.parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalizes_string_and_numeric_tvdb_ids() {
        let results = normalize_search_results(
            &json!({"data":[{"tvdb_id":"42","name":"Example","year":"2022"}]}),
            MediaKind::Series,
        );
        assert_eq!(results[0].id, 42);
        assert_eq!(results[0].year, Some(2022));
    }

    #[test]
    fn selects_the_requested_series_translation() {
        let document = json!({
            "data": {
                "translations": {
                    "nameTranslations": [
                        {"language": "jpn", "name": "転生したらスライムだった件"},
                        {"language": "eng", "name": "That Time I Got Reincarnated as a Slime"}
                    ]
                }
            }
        });
        assert_eq!(
            translation_name(&document, "eng").as_deref(),
            Some("That Time I Got Reincarnated as a Slime")
        );
    }

    #[test]
    fn page_normalization_preserves_absolute_numbers() {
        let episodes = normalize_episode_page(&json!({"data":{"episodes":[{
            "id":5,"seasonNumber":1,"number":2,"absoluteNumber":12,"name":"Episode"
        }]}}));
        assert_eq!(episodes[0].absolute_number, Some(12));
        assert_eq!(episodes[0].scope_name, "Season 1");
    }

    #[test]
    fn language_episode_pages_support_tvdbs_nested_response() {
        let episodes = normalize_episode_page(&json!({"data":{"series":{"episodes":[{
            "id":5,"seasonNumber":1,"number":2,"name":"Meeting the Goblins"
        }]}}}));
        assert_eq!(episodes[0].name, "Meeting the Goblins");
    }
}
