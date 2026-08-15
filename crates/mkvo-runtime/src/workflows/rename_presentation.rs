use std::collections::BTreeSet;
use std::path::Path;

use mkvo_application::{parse_episode_number, parse_season_episode};
use mkvo_contracts::RenameScopeRow;
use mkvo_domain::{EpisodeMetadata, RemuxMode, RemuxPlanItem, TrackKind};

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

    let number = parse_episode_number(file_name)?;
    let mut matches = episodes
        .iter()
        .filter(in_scope)
        .filter(|episode| episode.episode == number);
    let matched = matches.next()?;
    matches.next().is_none().then_some(matched)
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
    match item.mode {
        RemuxMode::ExtractSubtitles => {
            format!("Extract {} subtitle track(s)", item.extract_tracks.len())
        }
        RemuxMode::MuxSubtitles => format!(
            "Remux with {} matching external subtitle(s)",
            item.external_subtitles.len()
        ),
        RemuxMode::ConvertToMkv => "Losslessly copy streams into MKV".to_owned(),
        RemuxMode::Remux => format!("Keep {} selected track(s)", item.selected_track_ids.len()),
    }
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

pub(super) fn path_key(value: &str) -> String {
    value
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
        assert!(
            match_episode_for_file("Superstore - Episode 1.mkv", &episodes, &BTreeSet::new())
                .is_none()
        );

        let selected = BTreeSet::from([6]);
        let matched = match_episode_for_file("Superstore - Episode 1.mkv", &episodes, &selected)
            .expect("unique selected-season match");
        assert_eq!(matched.season, 6);
    }
}
