//! AniList metadata client.
//!
//! AniList exposes a single GraphQL endpoint and needs no credentials, so it is
//! usable without any Settings configuration.
//!
//! AniList models an anime as one media entry with an episode count rather than
//! a per-episode list, so episodes are synthesized from that count. Formats that
//! are not a numbered run — specials, OVAs, ONAs, music videos — are reported as
//! season 0 so they land in the specials scope rather than pretending to be a
//! first season.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use tokio_util::sync::CancellationToken;

use crate::common::{
    EpisodeMetadata, MediaKind, MetadataProviderClient, ProviderCredentials, ProviderError,
    ProviderKind, SearchResult, SelectedMedia, provider_http_client, send_with_retry,
};

const ENDPOINT: &str = "https://graphql.anilist.co";
const KIND: ProviderKind = ProviderKind::AniList;

const SEARCH_QUERY: &str = r"
query ($search: String) {
  Page(page: 1, perPage: 25) {
    media(search: $search, type: ANIME, sort: SEARCH_MATCH) {
      id
      title { romaji english native }
      description(asHtml: false)
      seasonYear
      format
      episodes
    }
  }
}";

const MEDIA_QUERY: &str = r"
query ($id: Int) {
  Media(id: $id, type: ANIME) {
    id
    title { romaji english native }
    seasonYear
    format
    episodes
  }
}";

#[derive(Debug, Clone)]
pub struct AniListClient {
    http: reqwest::Client,
    endpoint: String,
}

impl Default for AniListClient {
    fn default() -> Self {
        Self::new()
    }
}

impl AniListClient {
    #[must_use]
    pub fn new() -> Self {
        Self::with_endpoint(ENDPOINT)
    }

    #[must_use]
    pub fn with_endpoint(endpoint: impl Into<String>) -> Self {
        Self {
            http: provider_http_client(),
            endpoint: endpoint.into(),
        }
    }

    async fn post(
        &self,
        body: serde_json::Value,
        cancellation: CancellationToken,
    ) -> Result<GraphQlEnvelope, ProviderError> {
        let response = send_with_retry(KIND, &cancellation, || {
            self.http.post(&self.endpoint).json(&body)
        })
        .await?;
        let envelope: GraphQlEnvelope = response
            .json()
            .await
            .map_err(|error| ProviderError::invalid(KIND, error))?;
        // GraphQL reports failures in the body with HTTP 200, so a successful
        // status alone does not mean the query succeeded.
        if let Some(error) = envelope.errors.as_ref().and_then(|errors| errors.first()) {
            return Err(ProviderError::InvalidResponse {
                provider: KIND,
                message: error.message.clone(),
            });
        }
        Ok(envelope)
    }
}

#[derive(Debug, Deserialize)]
struct GraphQlEnvelope {
    #[serde(default)]
    data: Option<GraphQlData>,
    #[serde(default)]
    errors: Option<Vec<GraphQlError>>,
}

#[derive(Debug, Deserialize)]
struct GraphQlError {
    #[serde(default)]
    message: String,
}

#[derive(Debug, Default, Deserialize)]
struct GraphQlData {
    #[serde(default, rename = "Page")]
    page: Option<Page>,
    #[serde(default, rename = "Media")]
    media: Option<Media>,
}

#[derive(Debug, Default, Deserialize)]
struct Page {
    #[serde(default)]
    media: Vec<Media>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Media {
    #[serde(default)]
    id: u64,
    #[serde(default)]
    title: Title,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    season_year: Option<u16>,
    #[serde(default)]
    format: Option<String>,
    #[serde(default)]
    episodes: Option<u32>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Title {
    #[serde(default)]
    romaji: Option<String>,
    #[serde(default)]
    english: Option<String>,
    #[serde(default)]
    native: Option<String>,
}

impl Media {
    fn year(&self) -> Option<u16> {
        self.season_year
    }
}

fn pick_title(title: &Title, language: &str) -> String {
    let language = language.trim().to_ascii_lowercase();
    if matches!(language.as_str(), "eng" | "en")
        && let Some(english) = non_empty(title.english.as_deref())
    {
        return english;
    }
    if matches!(language.as_str(), "jpn" | "ja")
        && let Some(native) = non_empty(title.native.as_deref())
    {
        return native;
    }
    non_empty(title.romaji.as_deref())
        .or_else(|| non_empty(title.english.as_deref()))
        .or_else(|| non_empty(title.native.as_deref()))
        .unwrap_or_default()
}

fn non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

/// AniList descriptions carry HTML even with `asHtml: false`.
fn strip_html(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut in_tag = false;
    for character in value.chars() {
        match character {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => output.push(character),
            _ => {}
        }
    }
    output
        .replace("&quot;", "\"")
        .replace("&#039;", "'")
        .replace("&amp;", "&")
        .trim()
        .to_owned()
}

/// Formats that are not a numbered season run belong in the specials scope.
fn is_special_format(format: Option<&str>) -> bool {
    format.is_some_and(|format| {
        matches!(
            format.trim().to_ascii_uppercase().as_str(),
            "SPECIAL" | "OVA" | "ONA" | "MUSIC"
        )
    })
}

fn overview_with_format(media: &Media) -> Option<String> {
    let mut prefix = media.format.clone().unwrap_or_default().trim().to_owned();
    if let Some(count) = media.episodes.filter(|count| *count > 0) {
        prefix = if prefix.is_empty() {
            format!("{count} episodes")
        } else {
            format!("{prefix} • {count} episodes")
        };
    }
    let description = media
        .description
        .as_deref()
        .map(strip_html)
        .unwrap_or_default();
    let combined = match (prefix.is_empty(), description.is_empty()) {
        (true, true) => String::new(),
        (true, false) => description,
        (false, true) => prefix,
        (false, false) => format!("{prefix}\n{description}"),
    };
    (!combined.is_empty()).then_some(combined)
}

#[async_trait]
impl MetadataProviderClient for AniListClient {
    fn provider(&self) -> ProviderKind {
        KIND
    }

    async fn search(
        &self,
        _credentials: &ProviderCredentials,
        query: &str,
        language: &str,
        cancellation: CancellationToken,
    ) -> Result<Vec<SearchResult>, ProviderError> {
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }
        let envelope = self
            .post(
                json!({ "query": SEARCH_QUERY, "variables": { "search": query.trim() } }),
                cancellation,
            )
            .await?;
        let media = envelope
            .data
            .and_then(|data| data.page)
            .map(|page| page.media)
            .unwrap_or_default();

        Ok(media
            .into_iter()
            .filter(|item| item.id > 0)
            .filter_map(|item| {
                let name = pick_title(&item.title, language);
                if name.is_empty() {
                    return None;
                }
                Some(SearchResult {
                    provider: KIND,
                    id: item.id,
                    kind: MediaKind::Series,
                    year: item.year(),
                    overview: overview_with_format(&item),
                    database_url: Some(format!("https://anilist.co/anime/{}", item.id)),
                    name,
                })
            })
            .collect())
    }

    async fn episodes(
        &self,
        _credentials: &ProviderCredentials,
        selected: &SelectedMedia,
        _language: &str,
        cancellation: CancellationToken,
    ) -> Result<Vec<EpisodeMetadata>, ProviderError> {
        let envelope = self
            .post(
                json!({ "query": MEDIA_QUERY, "variables": { "id": selected.id } }),
                cancellation,
            )
            .await?;
        let Some(media) = envelope.data.and_then(|data| data.media) else {
            return Ok(Vec::new());
        };

        let count = media.episodes.filter(|count| *count > 0).unwrap_or(1);
        let season = u32::from(!is_special_format(media.format.as_deref()));
        let scope = if season == 0 {
            "Specials / OVAs"
        } else {
            "Main Series"
        };

        Ok((1..=count)
            .map(|number| EpisodeMetadata {
                provider: KIND,
                id: selected.id * 10_000 + u64::from(number),
                season_number: season,
                episode_number: number,
                absolute_number: Some(number),
                name: format!("Episode {number:02}"),
                scope_name: scope.to_owned(),
                air_date: None,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferred_language_selects_the_matching_title() {
        let title = Title {
            romaji: Some("Shingeki no Kyojin".to_owned()),
            english: Some("Attack on Titan".to_owned()),
            native: Some("進撃の巨人".to_owned()),
        };
        assert_eq!(pick_title(&title, "eng"), "Attack on Titan");
        assert_eq!(pick_title(&title, "jpn"), "進撃の巨人");
        // Romaji is the fallback because it is the most widely recognizable and
        // is what release names are usually based on.
        assert_eq!(pick_title(&title, "fre"), "Shingeki no Kyojin");
    }

    #[test]
    fn missing_titles_fall_back_rather_than_yielding_an_empty_name() {
        let title = Title {
            romaji: None,
            english: None,
            native: Some("進撃の巨人".to_owned()),
        };
        assert_eq!(pick_title(&title, "eng"), "進撃の巨人");
        assert!(pick_title(&Title::default(), "eng").is_empty());
    }

    /// Descriptions arrive with HTML even though the query asks for plain text.
    #[test]
    fn descriptions_are_stripped_of_markup_and_entities() {
        assert_eq!(
            strip_html("<i>Hello</i><br>world &quot;quoted&quot; &amp; more"),
            "Helloworld \"quoted\" & more"
        );
    }

    /// A special or OVA is not a first season; putting it in season 1 would make
    /// the rename scope selector produce wrong episode numbers.
    #[test]
    fn non_series_formats_are_treated_as_specials() {
        for format in ["SPECIAL", "ova", "ONA", "Music"] {
            assert!(is_special_format(Some(format)), "{format}");
        }
        for format in ["TV", "MOVIE", "TV_SHORT"] {
            assert!(!is_special_format(Some(format)), "{format}");
        }
        assert!(!is_special_format(None));
    }

    /// AniList returns `seasonYear`; without the camelCase rename it silently
    /// deserializes as `None` and every search result loses its year.
    #[test]
    fn media_deserializes_anilists_camel_case_fields() {
        let media: Media = serde_json::from_value(serde_json::json!({
            "id": 1,
            "title": { "romaji": "Cowboy Bebop" },
            "seasonYear": 1998,
            "format": "TV",
            "episodes": 26
        }))
        .expect("media");
        assert_eq!(media.year(), Some(1998));
        assert_eq!(media.episodes, Some(26));
    }

    #[test]
    fn overview_leads_with_format_and_episode_count() {
        let media = Media {
            id: 1,
            format: Some("TV".to_owned()),
            episodes: Some(25),
            description: Some("<p>Great show</p>".to_owned()),
            ..Media::default()
        };
        assert_eq!(
            overview_with_format(&media).expect("overview"),
            "TV • 25 episodes\nGreat show"
        );
    }
}
