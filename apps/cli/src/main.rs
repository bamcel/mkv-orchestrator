//! MKV Orchestrator command line host.
//!
//! A third host over the same runtime the desktop and server use, so a scan
//! here sees the same cache, the same settings, and the same provider keys as
//! the app. It replaces the retired .NET CLI verb for verb.

mod cli;
mod commands;
mod runtime;

use std::process::ExitCode;

use clap::Parser as _;

use crate::cli::{Cli, Command};

/// Exit codes, inherited from the CLI this replaces so scripts keep working.
///
/// `2` is the interesting one: it means the command ran fine and found nothing
/// to do, which callers use to skip a follow-up step.
pub const EXIT_OK: u8 = 0;
pub const EXIT_ERROR: u8 = 1;
pub const EXIT_NOTHING_TO_DO: u8 = 2;
pub const EXIT_CANCELED: u8 = 130;

/// Parses arguments, mapping clap's exit codes onto this CLI's.
///
/// Left to itself clap exits 2 on a usage error, which here means "ran fine,
/// found nothing to do" -- a script testing for that would read a typo as an
/// empty result. Usage errors are failures, so they exit 1, as they did in the
/// CLI this replaces.
fn parse() -> Result<Cli, ExitCode> {
    match Cli::try_parse() {
        Ok(cli) => Ok(cli),
        Err(error) => {
            let requested = matches!(
                error.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            );
            let _ = error.print();
            Err(ExitCode::from(if requested { EXIT_OK } else { EXIT_ERROR }))
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = match parse() {
        Ok(cli) => cli,
        Err(code) => return code,
    };

    let outcome = tokio::select! {
        biased;
        // Ctrl-C during a long scan should stop cleanly rather than leave the
        // shell reporting whatever the runtime happened to be doing.
        _ = tokio::signal::ctrl_c() => {
            eprintln!("Canceled.");
            return ExitCode::from(EXIT_CANCELED);
        }
        result = run(cli) => result,
    };

    match outcome {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("{error:#}");
            ExitCode::from(EXIT_ERROR)
        }
    }
}

async fn run(cli: Cli) -> anyhow::Result<u8> {
    let target = cli.command.config_path().clone();
    let runtime = runtime::compose(cli.config.as_deref(), &target)?;

    match cli.command {
        Command::Scan(args) => commands::scan(&runtime, args).await,
        // `inspect` was always `scan --json`.
        Command::Inspect(mut args) => {
            args.json = true;
            commands::scan(&runtime, args).await
        }
        Command::Cleanup(args) => commands::cleanup(&runtime, args).await,
        Command::Rename(args) => commands::rename(&runtime, args).await,
    }
}
