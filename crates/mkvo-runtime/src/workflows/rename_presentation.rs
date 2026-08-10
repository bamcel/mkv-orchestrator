use std::collections::BTreeSet;
use std::path::Path;

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
