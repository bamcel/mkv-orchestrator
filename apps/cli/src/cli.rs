//! The command surface.
//!
//! Kept deliberately close to the retired .NET CLI: the same four verbs, the
//! same flag names, and the same exit codes, so existing scripts keep working.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "mkvo",
    about = "MKV Orchestrator command line",
    version,
    disable_help_subcommand = true
)]
pub struct Cli {
    /// Directory holding settings, cache, and logs.
    ///
    /// Falls back to the MKVO_CONFIG_DIR environment variable, then the OS
    /// configuration directory. It is the same store the server host uses, so
    /// provider keys and templates configured there apply here too.
    #[arg(long, global = true, value_name = "DIR")]
    pub config: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Scan a folder and report what is in each file.
    Scan(ScanArgs),
    /// Scan a folder and emit JSON. Identical to `scan --json`.
    Inspect(ScanArgs),
    /// Plan MKV property edits, and optionally apply them.
    Cleanup(CleanupArgs),
    /// Plan renames from provider metadata, and optionally apply them.
    Rename(RenameArgs),
}

#[derive(Debug, Args, Clone)]
pub struct ScanArgs {
    /// Folder to scan.
    #[arg(value_name = "FOLDER")]
    pub path: PathBuf,

    /// Emit JSON instead of a table.
    #[arg(long)]
    pub json: bool,

    /// Subfolder names to skip, comma separated.
    #[arg(long, value_name = "NAMES", value_delimiter = ',')]
    pub ignore: Vec<String>,

    /// Re-probe every file instead of trusting the cache.
    #[arg(long)]
    pub force_refresh: bool,

    #[command(flatten)]
    pub tools: ToolArgs,
}

/// Tool overrides. Omitted, the runtime resolves them from settings and PATH.
#[derive(Debug, Args, Clone, Default)]
pub struct ToolArgs {
    #[arg(long, value_name = "PATH")]
    pub mkvmerge: Option<String>,

    #[arg(long, value_name = "PATH")]
    pub ffprobe: Option<String>,
}

#[derive(Debug, Args, Clone)]
pub struct CleanupArgs {
    #[arg(value_name = "FOLDER")]
    pub path: PathBuf,

    #[arg(long)]
    pub json: bool,

    #[arg(long, value_name = "NAMES", value_delimiter = ',')]
    pub ignore: Vec<String>,

    /// Execute the planned edits. Without this the plan is only printed.
    #[arg(long)]
    pub apply: bool,

    /// Leave the container title alone. It is removed by default.
    #[arg(long)]
    pub keep_container_title: bool,

    /// Leave video track titles alone. They are removed by default.
    #[arg(long)]
    pub keep_video_title: bool,

    /// Also strip audio track titles.
    #[arg(long)]
    pub remove_audio_titles: bool,

    /// Also strip subtitle track titles.
    #[arg(long)]
    pub remove_subtitle_titles: bool,

    /// Force every audio track to this language code.
    #[arg(long, value_name = "CODE")]
    pub set_audio_language: Option<String>,

    /// Force every subtitle track to this language code.
    #[arg(long, value_name = "CODE")]
    pub set_subtitle_language: Option<String>,

    #[command(flatten)]
    pub tools: ToolArgs,
}

#[derive(Debug, Args, Clone)]
pub struct RenameArgs {
    #[arg(value_name = "FOLDER")]
    pub path: PathBuf,

    /// Series or film to match against. Defaults to the folder's name.
    ///
    /// Renaming goes through the same provider lookup the app uses, so a match
    /// is required -- this is what supplies episode titles.
    #[arg(long, value_name = "TITLE")]
    pub query: Option<String>,

    /// Metadata provider: TVDB, TMDB, AniDB, or AniList.
    #[arg(long, value_name = "NAME")]
    pub provider: Option<String>,

    #[arg(long, value_name = "CODE")]
    pub language: Option<String>,

    /// Which search result to use, 1-based. Defaults to the first.
    #[arg(long, value_name = "N", default_value_t = 1)]
    pub pick: usize,

    /// List the matches and stop, so a script can choose deliberately.
    #[arg(long)]
    pub list_matches: bool,

    #[arg(long, value_name = "TEMPLATE")]
    pub template: Option<String>,

    #[arg(long)]
    pub json: bool,

    #[arg(long, value_name = "NAMES", value_delimiter = ',')]
    pub ignore: Vec<String>,

    /// Execute the planned renames. Without this the plan is only printed.
    #[arg(long)]
    pub apply: bool,

    #[command(flatten)]
    pub tools: ToolArgs,
}

impl Command {
    pub fn config_path(&self) -> &PathBuf {
        match self {
            Command::Scan(args) | Command::Inspect(args) => &args.path,
            Command::Cleanup(args) => &args.path,
            Command::Rename(args) => &args.path,
        }
    }
}
