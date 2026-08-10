use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use chrono::{Duration, Utc};
use mkvo_domain::{
    EpisodeMetadata, IdempotencyKey, MediaFile, MetadataProvider, OperationKind, OperationPlan,
    PlanConflict, PlanConflictKind, PlanContext, RenamePlan, RenamePlanItem, RenamePlanPayload,
    RenameTokens, ResourceClaim,
};
use serde::{Deserialize, Serialize};

use crate::{ApplicationError, ApplicationResult, FileAccessState};

pub const DEFAULT_SERIES_TEMPLATE: &str = "{series} - S{season:00}E{episode:00} - {episodeTitle}";
pub const DEFAULT_MOVIE_TEMPLATE: &str = "{title} ({year})";
/// Used when the provider knows the film but not its year.
pub const DEFAULT_MOVIE_TITLE_ONLY_TEMPLATE: &str = "{title}";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenamePlanRequest {
    pub files: Vec<MediaFile>,
    pub template: String,
    pub provider: Option<MetadataProvider>,
    #[serde(default)]
    pub check_existing_files: bool,
    #[serde(default)]
    pub existing_paths: BTreeSet<PathBuf>,
    /// Access state of each source file, keyed by portable path.
    ///
    /// Gathered by the host rather than probed here so the planner stays pure
    /// and testable. An absent entry means "not probed" and never blocks.
    #[serde(default)]
    pub source_access: BTreeMap<String, FileAccessState>,
    /// Target parent directories known to exist, by portable path.
    #[serde(default)]
    pub existing_parents: BTreeSet<String>,
    #[serde(default)]
    pub authorized_roots: Vec<PathBuf>,
    pub settings_fingerprint: String,
    #[serde(default = "default_plan_ttl_seconds")]
    pub expires_in_seconds: u64,
    pub idempotency_key: IdempotencyKey,
}

const fn default_plan_ttl_seconds() -> u64 {
    900
}

/// Turn a probed access state into a blocking conflict.
///
/// `None` means the host did not probe the path, which must not block: a host
/// that cannot probe should not be worse off than one that never checked.
pub(crate) fn access_conflict(
    state: Option<&FileAccessState>,
    path: &Path,
) -> Option<PlanConflict> {
    match state {
        Some(FileAccessState::ReadOnly) => Some(PlanConflict::blocking(
            PlanConflictKind::ReadOnly,
            "File is read-only or permission is denied",
            Some(path.to_path_buf()),
        )),
        Some(FileAccessState::Busy) => Some(PlanConflict::blocking(
            PlanConflictKind::Busy,
            "File is open in another program",
            Some(path.to_path_buf()),
        )),
        Some(FileAccessState::Missing) => Some(PlanConflict::blocking(
            PlanConflictKind::MissingSource,
            "Source file no longer exists",
            Some(path.to_path_buf()),
        )),
        Some(FileAccessState::Available) | None => None,
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RenamePlanner;

impl RenamePlanner {
    pub fn build_plan(&self, request: RenamePlanRequest) -> ApplicationResult<RenamePlan> {
        if request.files.is_empty() {
            return Err(ApplicationError::InvalidRequest(
                "rename preview requires at least one file".to_owned(),
            ));
        }
        if request.expires_in_seconds == 0 {
            return Err(ApplicationError::InvalidRequest(
                "rename plan expiration must be positive".to_owned(),
            ));
        }

        let template = if request.template.trim().is_empty() {
            DEFAULT_SERIES_TEMPLATE.to_owned()
        } else {
            request.template.trim().to_owned()
        };
        let existing: BTreeSet<_> = request
            .existing_paths
            .iter()
            .map(|path| portable_path_key(path))
            .collect();
        let mut items = Vec::with_capacity(request.files.len());
        for file in &request.files {
            let new_file_name = build_file_name(file, &template);
            let target = file
                .path
                .parent()
                .unwrap_or_else(|| Path::new(""))
                .join(&new_file_name);
            let mut conflicts = Vec::new();
            let extension_only = if file.extension().is_empty() {
                String::new()
            } else {
                format!(".{}", file.extension())
            };
            if new_file_name.trim().is_empty() || new_file_name == extension_only {
                conflicts.push(PlanConflict::blocking(
                    PlanConflictKind::EmptyOutput,
                    "Rendered filename is empty",
                    Some(target.clone()),
                ));
            }
            if portable_path_key(&file.path) == portable_path_key(&target) {
                conflicts.push(PlanConflict::informational(
                    PlanConflictKind::NoChange,
                    "Source already has the requested name",
                    Some(target.clone()),
                ));
            }
            if request.check_existing_files
                && existing.contains(&portable_path_key(&target))
                && portable_path_key(&file.path) != portable_path_key(&target)
            {
                conflicts.push(PlanConflict::blocking(
                    PlanConflictKind::ExistingTarget,
                    "Target file already exists",
                    Some(target.clone()),
                ));
            }
            if !request.authorized_roots.is_empty()
                && !request
                    .authorized_roots
                    .iter()
                    .any(|root| portable_contains(root, &target))
            {
                conflicts.push(PlanConflict::blocking(
                    PlanConflictKind::UnauthorizedPath,
                    "Target is outside the authorized roots",
                    Some(target.clone()),
                ));
            }
            // A rename needs write access to the source. Catching this during
            // preview turns a mid-apply OS error into a row the user can act on.
            if let Some(conflict) = access_conflict(
                request.source_access.get(&portable_path_key(&file.path)),
                &file.path,
            ) {
                conflicts.push(conflict);
            }
            if !request.existing_parents.is_empty()
                && let Some(parent) = target.parent()
                && !request
                    .existing_parents
                    .contains(&portable_path_key(parent))
            {
                conflicts.push(PlanConflict::blocking(
                    PlanConflictKind::MissingParent,
                    "Target directory does not exist",
                    Some(target.clone()),
                ));
            }
            items.push(RenamePlanItem {
                source: file.path.clone(),
                target,
                source_fingerprint: file.fingerprint.clone(),
                new_file_name,
                conflicts,
            });
        }

        let mut targets: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (index, item) in items.iter().enumerate() {
            if portable_path_key(&item.source) != portable_path_key(&item.target) {
                targets
                    .entry(portable_path_key(&item.target))
                    .or_default()
                    .push(index);
            }
        }
        for duplicate_indices in targets.values().filter(|indices| indices.len() > 1) {
            for &index in duplicate_indices {
                let target = items[index].target.clone();
                items[index].conflicts.push(PlanConflict::blocking(
                    PlanConflictKind::DuplicateTarget,
                    "Multiple files render to the same target",
                    Some(target),
                ));
            }
        }

        let payload = RenamePlanPayload {
            template,
            provider: request.provider,
            items,
        };
        let context = PlanContext {
            settings_fingerprint: request.settings_fingerprint.clone(),
            tool_fingerprints: BTreeMap::new(),
            input_fingerprints: request
                .files
                .iter()
                .map(|file| file.fingerprint.clone())
                .collect(),
            resources: payload
                .items
                .iter()
                .flat_map(|item| {
                    [
                        ResourceClaim::write(item.source.clone()),
                        ResourceClaim::write(item.target.clone()),
                    ]
                })
                .collect(),
            attributes: BTreeMap::new(),
        };
        let now = Utc::now();
        let ttl = i64::try_from(request.expires_in_seconds).unwrap_or(i64::MAX);
        OperationPlan::new(
            OperationKind::Rename,
            &request,
            payload,
            context,
            now,
            now + Duration::seconds(ttl),
            request.idempotency_key.clone(),
        )
        .map_err(|error| ApplicationError::Internal(error.to_string()))
    }
}

#[must_use]
pub fn rename_tokens(file: &MediaFile) -> RenameTokens {
    let episode = file.episode.as_ref();
    let provider = file.provider_match.as_ref();
    let file_stem = file
        .path
        .file_stem()
        .map_or_else(String::new, |value| value.to_string_lossy().into_owned());
    let series = first_non_blank([
        provider.map(|value| value.title.as_str()),
        episode.and_then(|value| value.series_title.as_deref()),
        Some(file_stem.as_str()),
    ]);
    let episode_title = first_non_blank([
        provider.and_then(|value| value.episode_title.as_deref()),
        episode.and_then(|value| value.episode_title.as_deref()),
        Some(""),
    ]);
    let season = episode.and_then(|value| value.season);
    let episode_number = episode.and_then(|value| value.episode);
    let absolute = episode
        .and_then(|value| value.absolute_episode)
        .or_else(|| {
            season.zip(episode_number).map(|(season, episode)| {
                (season.max(1).saturating_sub(1))
                    .saturating_mul(1_000)
                    .saturating_add(episode)
            })
        });
    RenameTokens {
        title: series.clone(),
        series,
        year: episode.and_then(|value| value.year),
        season,
        episode: episode_number,
        absolute,
        episode_title,
    }
}

/// Whether a template positions a file within a series.
///
/// Season, episode, and absolute number are the tokens a film cannot answer.
/// Episode title is not one of them: a film's title is a reasonable thing to
/// put there, and the provider fills it in.
fn mentions_episode_position(template: &str) -> bool {
    let lowered = template.to_ascii_lowercase();
    // `{episode}` and `{episode:00}` are positions; `{episodeTitle}` is not, so
    // the episode token has to be matched closed rather than by prefix.
    lowered.contains("{season")
        || lowered.contains("{absolute")
        || lowered.contains("{episode}")
        || lowered.contains("{episode:")
}

#[must_use]
pub fn build_file_name(file: &MediaFile, template: &str) -> String {
    let tokens = rename_tokens(file);
    let is_movie = file.episode.as_ref().is_some_and(|value| value.is_movie);
    let mut active = template.trim();
    if active.is_empty() {
        active = DEFAULT_SERIES_TEMPLATE;
    }
    // A film has no season or episode, so a template asking for them renders
    // them as nothing and leaves the punctuation around them behind -- "S01E01
    // - " with no numbers, or worse. This used to catch only the exact default
    // series template, so any customised one produced that wreckage.
    //
    // A template that asks for neither is left alone: someone who wrote their
    // own movie template meant it.
    if is_movie && mentions_episode_position(active) {
        // Without a year the parenthesised form renders as "Title ()", so the
        // year is only asked for when the provider supplied one.
        active = if tokens.year.is_some() {
            DEFAULT_MOVIE_TEMPLATE
        } else {
            DEFAULT_MOVIE_TITLE_ONLY_TEMPLATE
        };
    }

    let mut rendered = active.to_owned();
    for (token, value) in [
        ("{episodeTitle}", tokens.episode_title),
        ("{absolute:000}", format_optional(tokens.absolute, 3)),
        ("{season:00}", format_optional(tokens.season, 2)),
        ("{episode:00}", format_optional(tokens.episode, 2)),
        ("{absolute}", format_optional(tokens.absolute, 0)),
        ("{season}", format_optional(tokens.season, 0)),
        ("{episode}", format_optional(tokens.episode, 0)),
        ("{series}", tokens.series),
        ("{title}", tokens.title),
        (
            "{year}",
            tokens
                .year
                .map_or_else(String::new, |value| value.to_string()),
        ),
    ] {
        rendered = replace_ascii_case_insensitive(&rendered, token, &value);
    }
    let stem = sanitize_file_name(&rendered);
    let extension = file.extension();
    if extension.is_empty() {
        stem
    } else {
        format!("{stem}.{extension}")
    }
}

#[must_use]
pub fn sanitize_file_name(value: &str) -> String {
    let replaced: String = value
        .chars()
        .map(|character| {
            if character.is_control() || "\\/:*?\"<>|".contains(character) {
                '-'
            } else {
                character
            }
        })
        .collect();
    let collapsed = replaced.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut clean = collapsed.trim().trim_end_matches(['.', ' ']).to_owned();
    while clean.contains(" -  ") {
        clean = clean.replace(" -  ", " - ");
    }
    let base = clean.split('.').next().unwrap_or_default();
    let upper = base.to_ascii_uppercase();
    let reserved = matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (upper.len() == 4
            && (upper.starts_with("COM") || upper.starts_with("LPT"))
            && upper.as_bytes()[3].is_ascii_digit()
            && upper.as_bytes()[3] != b'0');
    if reserved {
        clean.insert(0, '_');
    }
    clean
}

/// Apply the legacy planner sanitizer used for metadata-provided file names.
///
/// The filename builder intentionally substitutes portable-invalid characters
/// with hyphens, while the planner historically used spaces before collapsing
/// whitespace. Both behaviors are user-visible and are therefore kept as
/// separate operations.
#[must_use]
pub fn sanitize_planner_file_name(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_control() || "\\/:*?\"<>|".contains(character) {
                ' '
            } else {
                character
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_end_matches(['.', ' '])
        .to_owned()
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AbsoluteEpisodeMatch {
    pub absolute_episode: u32,
    pub episode: EpisodeMetadata,
}

impl AbsoluteEpisodeMatch {
    #[must_use]
    pub fn status_text(&self) -> String {
        format!(
            "Episode {} = S{:02}E{:02}",
            self.absolute_episode, self.episode.season, self.episode.episode
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderedEpisodeMatch {
    pub row_number: usize,
    pub episode: EpisodeMetadata,
}

impl OrderedEpisodeMatch {
    #[must_use]
    pub fn status_text(&self) -> String {
        format!(
            "List order match: row {} = S{:02}E{:02}",
            self.row_number, self.episode.season, self.episode.episode
        )
    }
}

/// Match a one-based absolute episode index against regular provider episodes.
/// Specials (season zero) never consume an absolute index.
#[must_use]
pub fn try_match_absolute_episode(
    provider_episodes: &[EpisodeMetadata],
    absolute_episode: Option<u32>,
) -> Option<AbsoluteEpisodeMatch> {
    let absolute_episode = absolute_episode.filter(|value| *value > 0)?;
    let episode = ordered_regular_episodes(provider_episodes)
        .into_iter()
        .nth(usize::try_from(absolute_episode - 1).ok()?)?
        .clone();
    Some(AbsoluteEpisodeMatch {
        absolute_episode,
        episode,
    })
}

/// Match files to provider episodes only when the regular-episode count is an
/// exact fit. This prevents silent row shifting when provider data is partial.
#[must_use]
pub fn match_by_list_order(
    provider_episodes: &[EpisodeMetadata],
    file_count: usize,
) -> Vec<OrderedEpisodeMatch> {
    if file_count == 0 {
        return Vec::new();
    }
    let ordered = ordered_regular_episodes(provider_episodes);
    if ordered.len() != file_count {
        return Vec::new();
    }
    ordered
        .into_iter()
        .enumerate()
        .map(|(index, episode)| OrderedEpisodeMatch {
            row_number: index + 1,
            episode: episode.clone(),
        })
        .collect()
}

fn ordered_regular_episodes(provider_episodes: &[EpisodeMetadata]) -> Vec<&EpisodeMetadata> {
    let mut ordered = provider_episodes
        .iter()
        .filter(|episode| episode.season > 0)
        .collect::<Vec<_>>();
    ordered.sort_by(|left, right| {
        left.season
            .cmp(&right.season)
            .then_with(|| left.episode.cmp(&right.episode))
            .then_with(|| left.title.to_lowercase().cmp(&right.title.to_lowercase()))
    });
    ordered
}

fn replace_ascii_case_insensitive(source: &str, token: &str, replacement: &str) -> String {
    let mut result = String::with_capacity(source.len());
    let mut remaining = source;
    let token_lower = token.to_ascii_lowercase();
    loop {
        let lower = remaining.to_ascii_lowercase();
        let Some(index) = lower.find(&token_lower) else {
            result.push_str(remaining);
            break;
        };
        result.push_str(&remaining[..index]);
        result.push_str(replacement);
        remaining = &remaining[index + token.len()..];
    }
    result
}

fn format_optional(value: Option<u32>, width: usize) -> String {
    value.map_or_else(String::new, |number| {
        if width == 0 {
            number.to_string()
        } else {
            format!("{number:0width$}")
        }
    })
}

fn first_non_blank<const N: usize>(values: [Option<&str>; N]) -> String {
    values
        .into_iter()
        .flatten()
        .find(|value| !value.trim().is_empty())
        .map_or_else(String::new, |value| value.trim().to_owned())
}

/// One path-key definition across planners. The local copy this replaced did
/// not strip the Windows verbatim prefix, so rename disagreed with the remux
/// and propedit planners about whether two spellings were the same file.
fn portable_path_key(path: &Path) -> String {
    crate::paths::path_key(path)
}

fn portable_contains(root: &Path, child: &Path) -> bool {
    let root = portable_path_key(root).trim_end_matches('/').to_owned();
    let child = portable_path_key(child);
    child == root
        || child
            .strip_prefix(&root)
            .is_some_and(|tail| tail.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use mkvo_domain::{
        ContainerMetadata, EpisodeIdentity, EpisodeMetadata, FileFingerprint, MediaStatus,
    };
    use serde_json::Value;

    use super::*;

    fn media(path: &str, episode: u32) -> MediaFile {
        MediaFile {
            path: PathBuf::from(path),
            original_file_name: None,
            watch_root: None,
            relative_path: None,
            fingerprint: FileFingerprint {
                path: PathBuf::from(path),
                size_bytes: 1,
                modified_at: Utc::now(),
                quick_hash: None,
            },
            container: ContainerMetadata::default(),
            tracks: Vec::new(),
            attachments: Vec::new(),
            episode: Some(EpisodeIdentity {
                series_title: Some("Show: Name".to_owned()),
                season: Some(1),
                episode: Some(episode),
                absolute_episode: None,
                episode_title: Some("Pilot / Part 1".to_owned()),
                year: Some(2025),
                is_movie: false,
            }),
            provider_match: None,
            status: MediaStatus::Ready,
        }
    }

    /// A film named with a series template used to render as the template's
    /// bare punctuation -- "SE -.mkv" -- because season and episode have
    /// nothing to say about a film.
    #[test]
    fn a_film_ignores_a_template_that_positions_it_in_a_series() {
        let mut file = media("Obsession (Bluray) 2026.mkv", 1);
        let episode = file.episode.as_mut().expect("episode");
        episode.is_movie = true;
        episode.series_title = Some("Obsession".to_owned());
        episode.episode_title = Some("Obsession".to_owned());
        episode.year = Some(2026);

        // The custom template the report came from.
        let name = build_file_name(&file, "S{season:00}E{episode:00} - {episodeTitle}");

        assert_eq!(name, "Obsession (2026).mkv");
    }

    /// A film whose year the provider does not know must not be renamed
    /// "Obsession ()".
    #[test]
    fn a_film_without_a_year_is_named_by_title_alone() {
        let mut file = media("Obsession (Bluray).mkv", 1);
        let episode = file.episode.as_mut().expect("episode");
        episode.is_movie = true;
        episode.series_title = Some("Obsession".to_owned());
        episode.episode_title = Some("Obsession".to_owned());
        episode.year = None;

        let name = build_file_name(&file, DEFAULT_SERIES_TEMPLATE);

        assert_eq!(name, "Obsession.mkv");
    }

    /// Someone who wrote a movie template meant it, so it is left alone.
    #[test]
    fn a_film_keeps_a_template_that_does_not_position_it() {
        let mut file = media("Obsession (Bluray) 2026.mkv", 1);
        let episode = file.episode.as_mut().expect("episode");
        episode.is_movie = true;
        episode.series_title = Some("Obsession".to_owned());
        episode.episode_title = Some("Obsession".to_owned());
        episode.year = Some(2026);

        let name = build_file_name(&file, "{title} [{year}]");

        assert_eq!(name, "Obsession [2026].mkv");
    }

    /// `{episodeTitle}` is a name, not a position, so it must not be mistaken
    /// for one by a prefix match on `{episode`.
    #[test]
    fn an_episode_title_alone_does_not_count_as_a_position() {
        let mut file = media("Obsession (Bluray) 2026.mkv", 1);
        let episode = file.episode.as_mut().expect("episode");
        episode.is_movie = true;
        episode.series_title = Some("Obsession".to_owned());
        episode.episode_title = Some("Obsession".to_owned());

        let name = build_file_name(&file, "{episodeTitle}");

        assert_eq!(name, "Obsession.mkv");
    }

    /// Series renaming must keep working exactly as before.
    #[test]
    fn a_series_still_uses_the_template_it_was_given() {
        let name = build_file_name(
            &media("old.mkv", 3),
            "S{season:00}E{episode:00} - {episodeTitle}",
        );
        assert_eq!(name, "S01E03 - Pilot - Part 1.mkv");
    }

    #[test]
    fn renders_and_sanitizes_series_template() {
        let name = build_file_name(&media("old.MKV", 2), DEFAULT_SERIES_TEMPLATE);
        assert_eq!(name, "Show- Name - S01E02 - Pilot - Part 1.MKV");
    }

    #[test]
    fn reserved_windows_name_is_portable() {
        assert_eq!(sanitize_file_name("CON"), "_CON");
    }

    #[test]
    fn duplicate_targets_block_every_collision() {
        let request = RenamePlanRequest {
            source_access: BTreeMap::new(),
            existing_parents: BTreeSet::new(),
            files: vec![media("a.mkv", 1), media("b.mkv", 1)],
            template: DEFAULT_SERIES_TEMPLATE.to_owned(),
            provider: None,
            check_existing_files: false,
            existing_paths: BTreeSet::new(),
            authorized_roots: Vec::new(),
            settings_fingerprint: "settings".to_owned(),
            expires_in_seconds: 60,
            idempotency_key: IdempotencyKey::generate(),
        };
        let plan = RenamePlanner.build_plan(request).unwrap();
        assert_eq!(plan.payload.rename_count(), 0);
        assert!(plan.payload.has_blocking_issues());
    }

    fn fixture_media(input: &Value) -> MediaFile {
        let source = input["sourcePath"]
            .as_str()
            .or_else(|| input["filePath"].as_str())
            .expect("fixture source path");
        let episode = input.get("episode").unwrap_or(input);
        let title = input["title"]
            .as_str()
            .or_else(|| input["seriesTitle"].as_str())
            .unwrap_or_default();
        let episode_title = episode["name"]
            .as_str()
            .or_else(|| input["episodeTitle"].as_str())
            .unwrap_or_default();
        MediaFile {
            path: PathBuf::from(source),
            original_file_name: None,
            watch_root: None,
            relative_path: None,
            fingerprint: FileFingerprint {
                path: PathBuf::from(source),
                size_bytes: 1,
                modified_at: Utc::now(),
                quick_hash: None,
            },
            container: ContainerMetadata::default(),
            tracks: Vec::new(),
            attachments: Vec::new(),
            episode: Some(EpisodeIdentity {
                series_title: Some(title.to_owned()),
                season: episode["seasonNumber"]
                    .as_u64()
                    .or_else(|| input["season"].as_u64())
                    .and_then(|value| u32::try_from(value).ok()),
                episode: episode["episodeNumber"]
                    .as_u64()
                    .or_else(|| input["episode"].as_u64())
                    .and_then(|value| u32::try_from(value).ok()),
                absolute_episode: None,
                episode_title: Some(episode_title.to_owned()),
                year: input["year"]
                    .as_u64()
                    .and_then(|value| u16::try_from(value).ok()),
                is_movie: input["isMovie"].as_bool().unwrap_or(false),
            }),
            provider_match: None,
            status: MediaStatus::Ready,
        }
    }

    fn provider_episodes(input: &Value) -> Vec<EpisodeMetadata> {
        input
            .as_array()
            .into_iter()
            .flatten()
            .map(|episode| EpisodeMetadata {
                id: episode["id"]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| episode["id"].to_string()),
                season: u32::try_from(episode["seasonNumber"].as_u64().unwrap_or_default())
                    .expect("season fits u32"),
                episode: u32::try_from(episode["episodeNumber"].as_u64().unwrap_or_default())
                    .expect("episode fits u32"),
                absolute_episode: None,
                title: episode["name"].as_str().unwrap_or_default().to_owned(),
                aired_at: None,
            })
            .collect()
    }

    fn portable(path: &Path) -> String {
        path.to_string_lossy().replace('\\', "/")
    }

    #[test]
    fn executes_every_rename_fixture_case() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../tests/parity-fixtures/rename-filenames.json"
        ))
        .expect("rename fixture JSON");
        let builder_cases = fixture["builderCases"].as_array().expect("builder cases");
        let matching_cases = fixture["matchingCases"].as_array().expect("matching cases");
        let planning_cases = fixture["planningCases"].as_array().expect("planning cases");
        assert_eq!(
            builder_cases.len() + matching_cases.len() + planning_cases.len(),
            10,
            "fixture case count changed"
        );

        for case in builder_cases {
            let id = case["id"].as_str().expect("case id");
            let expected = case["expected"].as_str().expect("string result");
            if case["operation"] == "RenamePlanner.SanitizeFileName" {
                assert_eq!(
                    sanitize_planner_file_name(case["input"].as_str().expect("input")),
                    expected,
                    "fixture case `{id}`"
                );
            } else {
                let file = fixture_media(&case["input"]);
                assert_eq!(
                    build_file_name(&file, case["input"]["template"].as_str().expect("template")),
                    expected,
                    "fixture case `{id}`"
                );
            }
        }

        for case in matching_cases {
            let id = case["id"].as_str().expect("case id");
            let input = &case["input"];
            let episodes = if input["providerEpisodes"].is_array() {
                provider_episodes(&input["providerEpisodes"])
            } else {
                (1..=input["regularEpisodeCount"].as_u64().unwrap_or_default())
                    .map(|number| EpisodeMetadata {
                        id: number.to_string(),
                        season: 1,
                        episode: u32::try_from(number).expect("episode fits u32"),
                        absolute_episode: None,
                        title: format!("Episode {number}"),
                        aired_at: None,
                    })
                    .collect()
            };
            if case["operation"] == "RenameEpisodeMatcher.TryMatchAbsoluteEpisode" {
                let matched = try_match_absolute_episode(
                    &episodes,
                    input["absoluteEpisode"]
                        .as_u64()
                        .and_then(|value| u32::try_from(value).ok()),
                );
                assert_eq!(
                    matched.is_some(),
                    case["expected"]["matched"].as_bool().expect("matched"),
                    "fixture case `{id}`"
                );
                if let Some(matched) = matched {
                    assert_eq!(
                        matched.episode.id,
                        case["expected"]["episodeId"].to_string()
                    );
                    assert_eq!(
                        matched.status_text(),
                        case["expected"]["statusText"]
                            .as_str()
                            .expect("status text"),
                        "fixture case `{id}`"
                    );
                }
            } else {
                let matches = match_by_list_order(
                    &episodes,
                    usize::try_from(input["fileCount"].as_u64().expect("file count"))
                        .expect("file count fits usize"),
                );
                let expected = case["expected"].as_array().expect("ordered matches");
                assert_eq!(matches.len(), expected.len(), "fixture case `{id}`");
                for (actual, expected) in matches.iter().zip(expected) {
                    assert_eq!(
                        actual.row_number,
                        expected["rowNumber"].as_u64().unwrap() as usize
                    );
                    assert_eq!(actual.episode.id, expected["episodeId"].to_string());
                    assert_eq!(
                        actual.status_text(),
                        expected["statusText"].as_str().unwrap()
                    );
                }
            }
        }

        for case in planning_cases {
            let input = &case["input"];
            let request = RenamePlanRequest {
                source_access: BTreeMap::new(),
                existing_parents: BTreeSet::new(),
                files: input["files"]
                    .as_array()
                    .expect("files")
                    .iter()
                    .map(fixture_media)
                    .collect(),
                template: input["template"].as_str().expect("template").to_owned(),
                provider: None,
                check_existing_files: input["checkExistingFiles"].as_bool().unwrap_or(false),
                existing_paths: BTreeSet::new(),
                authorized_roots: Vec::new(),
                settings_fingerprint: "fixture-settings".to_owned(),
                expires_in_seconds: 60,
                idempotency_key: IdempotencyKey::generate(),
            };
            let plan = RenamePlanner.build_plan(request).expect("rename plan");
            let expected = &case["expected"];
            assert_eq!(
                plan.payload.rename_count(),
                expected["renameCount"].as_u64().unwrap() as usize
            );
            assert_eq!(
                plan.payload.skip_count(),
                expected["skipCount"].as_u64().unwrap() as usize
            );
            assert_eq!(
                plan.payload.has_blocking_issues(),
                expected["hasBlockingIssues"].as_bool().unwrap()
            );
            for (actual, expected) in plan
                .payload
                .items
                .iter()
                .zip(expected["items"].as_array().expect("expected items"))
            {
                assert_eq!(
                    portable(&actual.source),
                    expected["sourcePath"].as_str().unwrap()
                );
                assert_eq!(
                    portable(&actual.target),
                    expected["targetPath"].as_str().unwrap()
                );
                assert_eq!(
                    actual.new_file_name,
                    expected["newFileName"].as_str().unwrap()
                );
                assert_eq!(actual.can_apply(), expected["canApply"].as_bool().unwrap());
                assert!(
                    actual
                        .conflicts
                        .iter()
                        .any(|conflict| conflict.kind == PlanConflictKind::DuplicateTarget)
                );
            }
        }
    }
}

#[cfg(test)]
mod access_conflict_tests {
    use super::*;
    use crate::FileAccessState;
    use mkvo_domain::{ContainerMetadata, EpisodeIdentity, FileFingerprint};

    fn media(path: &str) -> MediaFile {
        MediaFile {
            path: PathBuf::from(path),
            original_file_name: None,
            watch_root: None,
            relative_path: None,
            fingerprint: FileFingerprint {
                path: PathBuf::from(path),
                size_bytes: 1,
                modified_at: Utc::now(),
                quick_hash: None,
            },
            container: ContainerMetadata::default(),
            tracks: Vec::new(),
            attachments: Vec::new(),
            episode: Some(EpisodeIdentity {
                series_title: Some("Show".to_owned()),
                season: Some(1),
                episode: Some(1),
                absolute_episode: None,
                episode_title: Some("Pilot".to_owned()),
                year: None,
                is_movie: false,
            }),
            provider_match: None,
            status: mkvo_domain::MediaStatus::Ready,
        }
    }

    fn request(files: Vec<MediaFile>) -> RenamePlanRequest {
        RenamePlanRequest {
            files,
            template: DEFAULT_SERIES_TEMPLATE.to_owned(),
            provider: None,
            check_existing_files: false,
            existing_paths: BTreeSet::new(),
            source_access: BTreeMap::new(),
            existing_parents: BTreeSet::new(),
            authorized_roots: Vec::new(),
            settings_fingerprint: "settings".to_owned(),
            expires_in_seconds: 60,
            idempotency_key: IdempotencyKey::generate(),
        }
    }

    fn conflict_kinds(plan: &RenamePlan) -> Vec<PlanConflictKind> {
        plan.payload
            .items
            .iter()
            .flat_map(|item| item.conflicts.iter())
            .map(|conflict| conflict.kind)
            .collect()
    }

    /// The case this exists for: a media server or player holding the file open
    /// while MKVO plans a rename against it. Without the check the plan looks
    /// clean and the rename fails partway through.
    #[test]
    fn a_busy_source_blocks_the_rename_at_preview() {
        let file = media("/media/Show/a.mkv");
        let mut plan_request = request(vec![file.clone()]);
        plan_request
            .source_access
            .insert(portable_path_key(&file.path), FileAccessState::Busy);

        let plan = RenamePlanner.build_plan(plan_request).expect("plan");
        assert!(conflict_kinds(&plan).contains(&PlanConflictKind::Busy));
        assert!(plan.payload.has_blocking_issues());
        assert_eq!(plan.payload.rename_count(), 0);
    }

    #[test]
    fn a_read_only_source_blocks_the_rename_at_preview() {
        let file = media("/media/Show/a.mkv");
        let mut plan_request = request(vec![file.clone()]);
        plan_request
            .source_access
            .insert(portable_path_key(&file.path), FileAccessState::ReadOnly);

        let plan = RenamePlanner.build_plan(plan_request).expect("plan");
        assert!(conflict_kinds(&plan).contains(&PlanConflictKind::ReadOnly));
        assert!(plan.payload.has_blocking_issues());
    }

    #[test]
    fn a_missing_target_directory_blocks_the_rename() {
        let file = media("/media/Show/a.mkv");
        let mut plan_request = request(vec![file.clone()]);
        // A non-empty set that omits this parent means "probed, and absent".
        plan_request
            .existing_parents
            .insert("/media/other".to_owned());

        let plan = RenamePlanner.build_plan(plan_request).expect("plan");
        assert!(conflict_kinds(&plan).contains(&PlanConflictKind::MissingParent));
        assert!(plan.payload.has_blocking_issues());
    }

    /// A host that cannot probe must not be worse off than one that never
    /// checked, so an unprobed path stays renameable.
    #[test]
    fn an_unprobed_source_does_not_block() {
        let plan = RenamePlanner
            .build_plan(request(vec![media("/media/Show/a.mkv")]))
            .expect("plan");
        let kinds = conflict_kinds(&plan);
        assert!(!kinds.contains(&PlanConflictKind::Busy));
        assert!(!kinds.contains(&PlanConflictKind::ReadOnly));
        assert!(!kinds.contains(&PlanConflictKind::MissingParent));
        assert_eq!(plan.payload.rename_count(), 1);
    }

    /// The probe map is keyed by portable path, so a canonical Windows spelling
    /// on one side and a plain one on the other must still match — the same
    /// class of bug that made cache deletions silently no-op.
    #[test]
    fn probe_keys_match_across_windows_path_spellings() {
        let file = media(r"\\?\C:\media\Show\a.mkv");
        let mut plan_request = request(vec![file]);
        plan_request.source_access.insert(
            portable_path_key(Path::new(r"C:\media\Show\a.mkv")),
            FileAccessState::Busy,
        );

        let plan = RenamePlanner.build_plan(plan_request).expect("plan");
        assert!(conflict_kinds(&plan).contains(&PlanConflictKind::Busy));
    }
}
