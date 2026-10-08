use std::collections::BTreeSet;
use std::path::Path;

use mkvo_application::{
    parse_episode_number, parse_season_episode, try_match_absolute_episode,
};
use mkvo_contracts::RenameScopeRow;
use mkvo_domain::{EpisodeMetadata, ExternalSubtitle, RemuxMode, RemuxPlanItem, TrackKind};

pub(super) fn selected_seasons(keys: &[String]) -> BTreeSet<u32> {
    keys.iter()
        .filter_map(|key| key.strip_prefix("season:"))
        .filter_map(|value| value.parse().ok())
        .collect()
}

pub(super) fn scope_rows(episodes: &[EpisodeMetadata]) -> Vec<RenameScopeRow> {
    let mut seasons: Vec<_> = episodes.iter().map(|episode| episode.season).collect();
    seasons.sort_unstable();
    seasons.dedup();
    let mut rows = vec![RenameScopeRow {
        key: "all".to_owned(),
        label: format!("All episodes ({})", episodes.len()),
        is_selected: true,
    }];
    rows.extend(seasons.into_iter().map(|season| RenameScopeRow {
        key: format!("season:{season}"),
        label: format!("Season {season}"),
        is_selected: false,
    }));
    rows
}

pub(super) fn match_episode_for_file<'a>(
    file_name: &str,
    episodes: &'a [EpisodeMetadata],
    selected_seasons: &BTreeSet<u32>,
) -> Option<&'a EpisodeMetadata> {
    let in_scope = |episode: &&EpisodeMetadata| {
        selected_seasons.is_empty() || selected_seasons.contains(&episode.season)
    };

    if let Some((season, number)) = parse_season_episode(file_name) {
        return episodes
            .iter()
            .filter(in_scope)
            .find(|episode| episode.season == season && episode.episode == number);
    }

    let number = rename_episode_number(file_name)?;
    let mut matches = episodes
        .iter()
        .filter(in_scope)
        .filter(|episode| episode.episode == number);
    let matched = matches.next();
    if matched.is_some() && matches.next().is_none() {
        return matched;
    }

    // Anime releases commonly use one continuously increasing episode number
    // instead of season/episode notation. When that number is ambiguous (or no
    // season contains it), treat it as a one-based position in the provider's
    // regular episode order: 1 = S01E01, then continue across seasons. A
    // concrete season scope remains authoritative and must not be silently
    // reinterpreted as an all-series absolute number.
    if !selected_seasons.is_empty() {
        return None;
    }
    let absolute = try_match_absolute_episode(episodes, Some(number))?;
    episodes
        .iter()
        .find(|episode| episode.id == absolute.episode.id)
}

/// Extract an episode number for rename matching.
///
/// The shared parser intentionally requires an episode marker or numeric
/// bracket. Rename additionally accepts the common release form `Title - 55`,
/// but only when the final stem component is entirely numeric. Keeping this
/// rule here avoids making library audits mistake unrelated numbers for episode
/// identities.
pub(super) fn rename_episode_number(file_name: &str) -> Option<u32> {
    parse_episode_number(file_name).or_else(|| {
        let stem = Path::new(file_name).file_stem()?.to_string_lossy();
        let (_, suffix) = stem.rsplit_once(" - ")?;
        let suffix = suffix.trim();
        (!suffix.is_empty()
            && suffix.len() <= 4
            && suffix.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| suffix.parse().ok())
        .flatten()
    })
}

pub(super) fn track_kind_label(kind: TrackKind) -> &'static str {
    match kind {
        TrackKind::Video => "Video",
        TrackKind::Audio => "Audio",
        TrackKind::Subtitle => "Subtitle",
        TrackKind::Buttons => "Buttons",
        TrackKind::Other => "Track",
    }
}

pub(super) fn remux_mode_label(mode: RemuxMode) -> &'static str {
    match mode {
        RemuxMode::Remux => "Remux",
        RemuxMode::ConvertToMkv => "Convert to MKV",
        RemuxMode::MuxSubtitles => "Mux subtitles",
        RemuxMode::ExtractSubtitles => "Extract subtitles",
    }
}

pub(super) fn remux_tool_name(mode: RemuxMode) -> &'static str {
    if mode == RemuxMode::ExtractSubtitles {
        "mkvextract"
    } else {
        "mkvmerge"
    }
}

pub(super) fn remux_description(item: &RemuxPlanItem) -> String {
    let operation = match item.mode {
        RemuxMode::ExtractSubtitles => {
            format!("Extract {} subtitle track(s)", item.extract_tracks.len())
        }
        RemuxMode::MuxSubtitles => format!(
            "Remux with {} matching external subtitle(s):\n{}",
            item.external_subtitles.len(),
            external_subtitle_list(&item.external_subtitles)
        ),
        RemuxMode::ConvertToMkv => "Losslessly copy streams into MKV".to_owned(),
        RemuxMode::Remux => format!("Keep {} selected track(s)", item.selected_track_ids.len()),
    };
    if !same_path(&item.source, &item.final_output) {
        format!("{operation}\nOutput: {}", file_name(&item.final_output))
    } else {
        operation
    }
}

fn external_subtitle_list(subtitles: &[ExternalSubtitle]) -> String {
    subtitles
        .iter()
        .map(|subtitle| {
            let mut details = Vec::new();
            if !subtitle.language.trim().is_empty() {
                details.push(subtitle.language.trim().to_owned());
            }
            if let Some(name) = subtitle
                .name
                .as_deref()
                .map(str::trim)
                .filter(|name| !name.is_empty())
            {
                details.push(name.to_owned());
            }
            if subtitle.default {
                details.push("default".to_owned());
            }
            if subtitle.forced {
                details.push("forced".to_owned());
            }

            let suffix = (!details.is_empty())
                .then(|| format!(" ({})", details.join(" · ")))
                .unwrap_or_default();
            format!("• {}{suffix}", file_name(&subtitle.path))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn redacted_remux_command(item: &RemuxPlanItem) -> String {
    format!(
        "{} [structured arguments] \"{}\"",
        remux_tool_name(item.mode),
        item.source.display()
    )
}

pub(super) fn same_path(left: &Path, right: &Path) -> bool {
    path_key(&left.to_string_lossy()) == path_key(&right.to_string_lossy())
}

/// Comparison key for a path in workflow lookups.
///
/// Scanned files carry the canonical path, which on Windows has the
/// extended-length `\\?\` prefix, while rows from the UI carry the plain form.
/// The prefix is dropped first so both spellings of one file share a key;
/// otherwise row lookups miss and fall back to re-fingerprinting the file.
pub(super) fn path_key(value: &str) -> String {
    mkvo_domain::normalized_path_text(Path::new(value))
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_ascii_lowercase()
}

pub(super) fn file_name(path: &Path) -> String {
    path.file_name()
        .map_or_else(String::new, |value| value.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn episode(season: u32, number: u32, title: &str) -> EpisodeMetadata {
        EpisodeMetadata {
            id: format!("{season}-{number}"),
            season,
            episode: number,
            absolute_episode: None,
            title: title.to_owned(),
            aired_at: None,
        }
    }

    #[test]
    fn multi_season_filename_matches_its_encoded_season() {
        let episodes = vec![episode(1, 1, "Pilot"), episode(6, 1, "Essential")];
        let matched = match_episode_for_file(
            "Superstore (2015) - S06E01 - Essential.mkv",
            &episodes,
            &BTreeSet::new(),
        )
        .expect("season six match");
        assert_eq!((matched.season, matched.episode), (6, 1));
        assert_eq!(matched.title, "Essential");
    }

    #[test]
    fn episode_only_filename_must_be_unambiguous_in_scope() {
        let episodes = vec![episode(1, 1, "Pilot"), episode(6, 1, "Essential")];
        let release_order = match_episode_for_file(
            "Superstore - Episode 1.mkv",
            &episodes,
            &BTreeSet::new(),
        )
        .expect("first regular episode by release order");
        assert_eq!((release_order.season, release_order.episode), (1, 1));

        let selected = BTreeSet::from([6]);
        let matched = match_episode_for_file("Superstore - Episode 1.mkv", &episodes, &selected)
            .expect("unique selected-season match");
        assert_eq!(matched.season, 6);
    }

    #[test]
    fn episode_only_filename_falls_back_to_cross_season_release_order() {
        let episodes = vec![
            episode(2, 2, "Fourth"),
            episode(1, 2, "Second"),
            episode(2, 1, "Third"),
            episode(1, 1, "First"),
        ];

        let matched = match_episode_for_file(
            "Example Show - 3.mkv",
            &episodes,
            &BTreeSet::new(),
        )
        .expect("third regular episode");

        assert_eq!((matched.season, matched.episode), (2, 1));
        assert_eq!(matched.title, "Third");
    }

    #[test]
    fn explicit_season_episode_still_wins_over_release_order() {
        let episodes = vec![episode(1, 1, "First"), episode(2, 1, "Second season")];

        let matched = match_episode_for_file(
            "Example Show - S02E01.mkv",
            &episodes,
            &BTreeSet::new(),
        )
        .expect("explicit season match");

        assert_eq!((matched.season, matched.episode), (2, 1));
    }

    #[test]
    fn trailing_release_number_is_a_rename_only_episode_number() {
        assert_eq!(
            rename_episode_number("[Anime Time] My Hero Academia - 55.mkv"),
            Some(55)
        );
        assert_eq!(rename_episode_number("Example Show - 20240.mkv"), None);
        assert_eq!(rename_episode_number("Example Show 55.mkv"), None);
    }

    #[test]
    fn extended_length_and_plain_windows_paths_compare_equal() {
        assert!(same_path(
            Path::new(r"\\?\C:\Media\Show\Episode 01.mkv"),
            Path::new(r"C:\Media\Show\Episode 01.mkv")
        ));
        assert!(same_path(
            Path::new(r"\\?\UNC\nas\media\Episode 01.mkv"),
            Path::new(r"\\nas\media\Episode 01.mkv")
        ));
        assert_eq!(path_key(r"\\?\C:\Media\Show"), path_key("c:/media/show/"));
        assert!(!same_path(
            Path::new(r"\\?\C:\Media\Show\Episode 01.mkv"),
            Path::new(r"C:\Media\Show\Episode 02.mkv")
        ));
    }

    #[test]
    fn external_subtitle_list_names_every_matching_sidecar() {
        let subtitles = vec![
            ExternalSubtitle {
                path: "shows/Episode 01.eng.srt".into(),
                language: "eng".to_owned(),
                name: Some("English".to_owned()),
                default: true,
                forced: false,
            },
            ExternalSubtitle {
                path: "shows/Episode 01.jpn.forced.ass".into(),
                language: "jpn".to_owned(),
                name: None,
                default: false,
                forced: true,
            },
        ];

        assert_eq!(
            external_subtitle_list(&subtitles),
            "• Episode 01.eng.srt (eng · English · default)\n• Episode 01.jpn.forced.ass (jpn · forced)"
        );
    }
}
