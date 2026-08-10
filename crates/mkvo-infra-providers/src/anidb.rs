//! AniDB metadata client.
//!
//! AniDB has no search endpoint. Searching means downloading the full
//! gzip-compressed title dump and matching locally, so the dump is cached for
//! the interval AniDB's terms require rather than refetched per keystroke —
//! refetching it on every search risks a client ban.
//!
//! Episode lookup uses the HTTP API, which requires a registered client name
//! and version supplied from Settings. AniDB publishes that endpoint on plain
//! HTTP port 9001; HTTPS is not reliably available there.

use std::{
    io::Read,
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use flate2::read::GzDecoder;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

use crate::common::{
    EpisodeMetadata, MediaKind, MetadataProviderClient, ProviderCredentials, ProviderError,
    ProviderKind, SearchResult, SelectedMedia, send_with_retry,
};

const TITLES_URL: &str = "https://anidb.net/api/anime-titles.xml.gz";
const HTTP_API: &str = "http://api.anidb.net:9001/httpapi";
const KIND: ProviderKind = ProviderKind::AniDb;
/// AniDB asks clients not to fetch the title dump more than once a day.
const TITLE_CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const MAX_RESULTS: usize = 50;

#[derive(Debug, Clone)]
struct AnimeTitle {
    aid: u64,
    text: String,
    language: String,
    kind: String,
}

#[derive(Debug, Default)]
struct TitleCache {
    titles: Vec<AnimeTitle>,
    fetched_at: Option<Instant>,
}

impl TitleCache {
    fn is_fresh(&self) -> bool {
        self.fetched_at
            .is_some_and(|at| at.elapsed() < TITLE_CACHE_TTL)
    }
}

#[derive(Debug, Clone)]
pub struct AniDbClient {
    http: reqwest::Client,
    titles_url: String,
    api_url: String,
    cache: Arc<RwLock<TitleCache>>,
}

impl Default for AniDbClient {
    fn default() -> Self {
        Self::new()
    }
}

impl AniDbClient {
    #[must_use]
    pub fn new() -> Self {
        Self::with_endpoints(TITLES_URL, HTTP_API)
    }

    #[must_use]
    pub fn with_endpoints(titles_url: impl Into<String>, api_url: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            titles_url: titles_url.into(),
            api_url: api_url.into(),
            cache: Arc::new(RwLock::new(TitleCache::default())),
        }
    }

    async fn titles(
        &self,
        cancellation: CancellationToken,
    ) -> Result<Vec<AnimeTitle>, ProviderError> {
        if let Some(cached) = self.cached_titles().await {
            return Ok(cached);
        }

        let response =
            send_with_retry(KIND, &cancellation, || self.http.get(&self.titles_url)).await?;
        let bytes = response
            .bytes()
            .await
            .map_err(|error| ProviderError::network(KIND, error))?;
        let xml = decompress(&bytes)?;
        let titles = parse_titles(&xml)?;

        let mut cache = self.cache.write().await;
        cache.titles = titles.clone();
        cache.fetched_at = Some(Instant::now());
        Ok(titles)
    }

    async fn cached_titles(&self) -> Option<Vec<AnimeTitle>> {
        let cache = self.cache.read().await;
        cache.is_fresh().then(|| cache.titles.clone())
    }
}

/// The dump is served gzipped, but a proxy or CDN may have decompressed it
/// already, so plain XML is accepted rather than treated as corrupt.
fn decompress(bytes: &[u8]) -> Result<String, ProviderError> {
    if bytes.starts_with(&[0x1f, 0x8b]) {
        let mut decoder = GzDecoder::new(bytes);
        let mut xml = String::new();
        decoder
            .read_to_string(&mut xml)
            .map_err(|error| ProviderError::invalid(KIND, error))?;
        return Ok(xml);
    }
    String::from_utf8(bytes.to_vec()).map_err(|error| ProviderError::invalid(KIND, error))
}

fn parse_titles(xml: &str) -> Result<Vec<AnimeTitle>, ProviderError> {
    let document =
        roxmltree::Document::parse(xml).map_err(|error| ProviderError::invalid(KIND, error))?;
    let mut titles = Vec::new();
    for anime in document
        .descendants()
        .filter(|node| node.has_tag_name("anime"))
    {
        let Some(aid) = anime
            .attribute("aid")
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|aid| *aid > 0)
        else {
            continue;
        };
        for title in anime.children().filter(|node| node.has_tag_name("title")) {
            let text = title.text().unwrap_or_default().trim().to_owned();
            if text.is_empty() {
                continue;
            }
            titles.push(AnimeTitle {
                aid,
                text,
                language: normalize_language(
                    title
                        .attribute(("http://www.w3.org/XML/1998/namespace", "lang"))
                        .unwrap_or_default(),
                ),
                kind: title
                    .attribute("type")
                    .unwrap_or_default()
                    .to_ascii_lowercase(),
            });
        }
    }
    Ok(titles)
}

/// Fold the language spellings AniDB and MKVO each use onto one form.
fn normalize_language(language: &str) -> String {
    match language.trim().to_ascii_lowercase().as_str() {
        "eng" | "en" => "en",
        "jpn" | "ja" => "ja",
        "jpn-romaji" | "romaji" | "x_jat" | "x-jat" => "x-jat",
        "spa" | "es" => "es",
        "fre" | "fra" | "fr" => "fr",
        "ger" | "deu" | "de" => "de",
        "kor" | "ko" => "ko",
        "chi" | "zh" => "zh",
        other => other,
    }
    .to_owned()
}

/// Compare on letters and digits only so punctuation and spacing differences
/// between a release name and an AniDB title do not prevent a match.
fn search_key(value: &str) -> String {
    let mut key = String::with_capacity(value.len());
    let mut pending_space = false;
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            if pending_space && !key.is_empty() {
                key.push(' ');
            }
            pending_space = false;
            key.push(character.to_ascii_lowercase());
        } else {
            pending_space = true;
        }
    }
    key
}

/// Prefer the main title in the requested language, then the romanized main
/// title, then English, then whatever main title exists.
fn pick_title(titles: &[&AnimeTitle], language: &str) -> String {
    let preferred = normalize_language(language);
    let main = |lang: &str| {
        titles
            .iter()
            .find(|title| title.language == lang && title.kind == "main")
            .map(|title| title.text.clone())
    };
    main(&preferred)
        .or_else(|| main("x-jat"))
        .or_else(|| main("en"))
        .or_else(|| {
            titles
                .iter()
                .find(|title| title.kind == "main")
                .map(|title| title.text.clone())
        })
        .or_else(|| titles.first().map(|title| title.text.clone()))
        .unwrap_or_default()
}

fn parse_episode_number(value: &str) -> Option<u32> {
    let digits: String = value.chars().filter(char::is_ascii_digit).collect();
    digits.parse().ok().filter(|number| *number > 0)
}

fn build_episode_id(anime_id: u64, season: u32, episode: u32) -> u64 {
    anime_id * 100_000 + u64::from(season) * 10_000 + u64::from(episode)
}

fn parse_episodes(
    xml: &str,
    anime_id: u64,
    language: &str,
) -> Result<Vec<EpisodeMetadata>, ProviderError> {
    let document =
        roxmltree::Document::parse(xml).map_err(|error| ProviderError::invalid(KIND, error))?;

    if let Some(error) = document
        .descendants()
        .find(|node| node.has_tag_name("error"))
        .and_then(|node| node.text())
    {
        return Err(ProviderError::InvalidResponse {
            provider: KIND,
            message: error.trim().to_owned(),
        });
    }

    let preferred = normalize_language(language);
    let mut episodes: Vec<EpisodeMetadata> = Vec::new();
    for episode in document
        .descendants()
        .filter(|node| node.has_tag_name("episode"))
    {
        let Some(epno) = episode.children().find(|node| node.has_tag_name("epno")) else {
            continue;
        };
        // AniDB epno type 1 is the regular numbered run; every other type is a
        // special, OVA, credit, or trailer, which belong in the specials scope.
        let season = u32::from(epno.attribute("type").unwrap_or("1").trim() == "1");
        let Some(number) = parse_episode_number(epno.text().unwrap_or_default()) else {
            continue;
        };

        let titles: Vec<_> = episode
            .children()
            .filter(|node| node.has_tag_name("title"))
            .map(|node| {
                (
                    normalize_language(
                        node.attribute(("http://www.w3.org/XML/1998/namespace", "lang"))
                            .unwrap_or_default(),
                    ),
                    node.text().unwrap_or_default().trim().to_owned(),
                )
            })
            .filter(|(_, text)| !text.is_empty())
            .collect();
        let name = titles
            .iter()
            .find(|(lang, _)| *lang == preferred)
            .or_else(|| titles.iter().find(|(lang, _)| lang == "en"))
            .or_else(|| titles.iter().find(|(lang, _)| lang == "x-jat"))
            .or_else(|| titles.first())
            .map(|(_, text)| text.clone())
            .unwrap_or_else(|| {
                if season == 0 {
                    format!("Special {number:02}")
                } else {
                    format!("Episode {number:02}")
                }
            });

        episodes.push(EpisodeMetadata {
            provider: KIND,
            id: build_episode_id(anime_id, season, number),
            season_number: season,
            episode_number: number,
            absolute_number: (season == 1).then_some(number),
            name,
            scope_name: if season == 0 {
                "Specials / OVAs".to_owned()
            } else {
                "Main Series".to_owned()
            },
            air_date: episode
                .children()
                .find(|node| node.has_tag_name("airdate"))
                .and_then(|node| node.text())
                .map(|value| value.trim().to_owned()),
        });
    }

    // Some entries carry only a count rather than an episode list.
    if episodes.is_empty()
        && let Some(count) = document
            .descendants()
            .find(|node| node.has_tag_name("episodecount"))
            .and_then(|node| node.text())
            .and_then(|value| value.trim().parse::<u32>().ok())
            .filter(|count| *count > 0)
    {
        episodes = (1..=count)
            .map(|number| EpisodeMetadata {
                provider: KIND,
                id: build_episode_id(anime_id, 1, number),
                season_number: 1,
                episode_number: number,
                absolute_number: Some(number),
                name: format!("Episode {number:02}"),
                scope_name: "Main Series".to_owned(),
                air_date: None,
            })
            .collect();
    }

    episodes.sort_by_key(|episode| (episode.season_number, episode.episode_number));
    episodes.dedup_by_key(|episode| (episode.season_number, episode.episode_number));
    Ok(episodes)
}

#[async_trait]
impl MetadataProviderClient for AniDbClient {
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
        let key = search_key(query);
        if key.is_empty() {
            return Ok(Vec::new());
        }
        let titles = self.titles(cancellation).await?;

        let mut matches: Vec<u64> = Vec::new();
        for title in &titles {
            if search_key(&title.text).contains(&key) && !matches.contains(&title.aid) {
                matches.push(title.aid);
                if matches.len() >= MAX_RESULTS {
                    break;
                }
            }
        }

        Ok(matches
            .into_iter()
            .filter_map(|aid| {
                let group: Vec<&AnimeTitle> =
                    titles.iter().filter(|title| title.aid == aid).collect();
                let name = pick_title(&group, language);
                (!name.is_empty()).then(|| SearchResult {
                    provider: KIND,
                    id: aid,
                    kind: MediaKind::Series,
                    name,
                    year: None,
                    overview: Some(
                        "AniDB title match. Select to load episodes and specials.".to_owned(),
                    ),
                    database_url: Some(format!("https://anidb.net/anime/{aid}")),
                })
            })
            .collect())
    }

    async fn episodes(
        &self,
        credentials: &ProviderCredentials,
        selected: &SelectedMedia,
        language: &str,
        cancellation: CancellationToken,
    ) -> Result<Vec<EpisodeMetadata>, ProviderError> {
        // AniDB identifies callers by a registered client name rather than a
        // key, so the api_key slot carries `name/version`.
        let (client_name, client_version) = split_client(credentials.api_key.expose());
        if client_name.is_empty() {
            return Err(ProviderError::MissingCredentials { provider: KIND });
        }

        let url = self.api_url.clone();
        let response = send_with_retry(KIND, &cancellation, || {
            self.http.get(&url).query(&[
                ("request", "anime"),
                ("client", client_name.as_str()),
                ("clientver", client_version.as_str()),
                ("protover", "1"),
                ("aid", &selected.id.to_string()),
            ])
        })
        .await?;
        let xml = response
            .text()
            .await
            .map_err(|error| ProviderError::network(KIND, error))?;
        parse_episodes(&xml, selected.id, language)
    }
}

/// AniDB clients register a name and a numeric version. Settings stores them as
/// one value so the credential surface stays uniform across providers.
fn split_client(value: &str) -> (String, String) {
    let (name, version) = value.split_once('/').unwrap_or((value, "1"));
    let version: String = version.chars().filter(char::is_ascii_digit).collect();
    (
        name.trim().to_ascii_lowercase(),
        if version.is_empty() {
            "1".to_owned()
        } else {
            version
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const TITLES_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<animetitles>
  <anime aid="1">
    <title xml:lang="x-jat" type="main">Seikai no Monshou</title>
    <title xml:lang="en" type="official">Crest of the Stars</title>
  </anime>
  <anime aid="2">
    <title xml:lang="x-jat" type="main">Cowboy Bebop</title>
  </anime>
</animetitles>"#;

    #[test]
    fn titles_parse_with_language_and_type() {
        let titles = parse_titles(TITLES_XML).expect("titles");
        assert_eq!(titles.len(), 3);
        let first = &titles[0];
        assert_eq!(first.aid, 1);
        assert_eq!(first.language, "x-jat");
        assert_eq!(first.kind, "main");
    }

    /// Release names rarely punctuate the way AniDB does, so matching ignores
    /// everything except letters and digits.
    #[test]
    fn search_matching_ignores_punctuation_and_case() {
        assert_eq!(search_key("Seikai no Monshou!"), "seikai no monshou");
        assert_eq!(search_key("COWBOY-BEBOP"), "cowboy bebop");
        assert!(search_key("Fate/stay night").contains("fate stay night"));
    }

    #[test]
    fn main_title_is_preferred_in_the_requested_language() {
        let titles = parse_titles(TITLES_XML).expect("titles");
        let group: Vec<&AnimeTitle> = titles.iter().filter(|title| title.aid == 1).collect();
        // No English *main* title exists, so the romanized main title wins over
        // the English official one.
        assert_eq!(pick_title(&group, "eng"), "Seikai no Monshou");
        assert_eq!(pick_title(&group, "x-jat"), "Seikai no Monshou");
    }

    #[test]
    fn gzip_and_plain_payloads_both_decompress() {
        use flate2::{Compression, write::GzEncoder};
        use std::io::Write;

        let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
        encoder.write_all(TITLES_XML.as_bytes()).expect("write");
        let gzipped = encoder.finish().expect("finish");

        assert_eq!(decompress(&gzipped).expect("gzip"), TITLES_XML);
        assert_eq!(
            decompress(TITLES_XML.as_bytes()).expect("plain"),
            TITLES_XML
        );
    }

    /// epno type 1 is the numbered run; every other type is a special. Treating
    /// them all as season 1 would renumber specials over real episodes.
    #[test]
    fn only_type_one_episodes_are_the_main_series() {
        let xml = r#"<anime>
          <episodes>
            <episode><epno type="1">1</epno><title xml:lang="en">First</title><airdate>2001-01-05</airdate></episode>
            <episode><epno type="2">S1</epno><title xml:lang="en">A Special</title></episode>
            <episode><epno type="1">2</epno><title xml:lang="ja">二番</title></episode>
          </episodes>
        </anime>"#;
        let episodes = parse_episodes(xml, 7, "eng").expect("episodes");
        assert_eq!(episodes.len(), 3);
        assert_eq!(episodes[0].season_number, 0);
        assert_eq!(episodes[0].name, "A Special");
        assert_eq!(episodes[0].scope_name, "Specials / OVAs");
        assert_eq!(episodes[1].season_number, 1);
        assert_eq!(episodes[1].name, "First");
        assert_eq!(episodes[1].air_date.as_deref(), Some("2001-01-05"));
        // No English title on the third, so it falls back rather than dropping.
        assert_eq!(episodes[2].name, "二番");
    }

    #[test]
    fn an_error_body_is_reported_rather_than_parsed_as_zero_episodes() {
        let xml = r#"<error>Banned</error>"#;
        let error = parse_episodes(xml, 1, "eng").expect_err("error");
        assert!(error.to_string().contains("Banned"), "{error}");
    }

    #[test]
    fn an_episode_count_stands_in_for_a_missing_episode_list() {
        let xml = r#"<anime><episodecount>3</episodecount></anime>"#;
        let episodes = parse_episodes(xml, 5, "eng").expect("episodes");
        assert_eq!(episodes.len(), 3);
        assert!(episodes.iter().all(|episode| episode.season_number == 1));
    }

    #[test]
    fn client_credentials_split_into_name_and_numeric_version() {
        assert_eq!(split_client("mkvo/2"), ("mkvo".to_owned(), "2".to_owned()));
        assert_eq!(split_client("MKVO"), ("mkvo".to_owned(), "1".to_owned()));
        assert_eq!(
            split_client("mkvo/v3.1"),
            ("mkvo".to_owned(), "31".to_owned())
        );
    }
}
