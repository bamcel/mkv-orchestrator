//! The parts of the CLI a script depends on: exit codes and the argument
//! surface. Both were inherited from the retired .NET CLI, so a change here
//! breaks somebody's automation rather than just their expectations.

use std::path::PathBuf;
use std::process::Command;

fn binary() -> PathBuf {
    // The integration test binary sits next to the one under test.
    let mut path = std::env::current_exe().expect("test binary path");
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.join(format!("mkvo{}", std::env::consts::EXE_SUFFIX))
}

fn run(args: &[&str]) -> (i32, String, String) {
    let output = Command::new(binary())
        .args(args)
        .output()
        .expect("the mkvo binary should be built alongside this test");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// Clap exits 2 on a usage error by default, which collides with this CLI's
/// "ran fine, nothing to do". A script testing for 2 must not see a typo.
#[test]
fn usage_errors_exit_one_not_two() {
    assert_eq!(run(&[]).0, 1, "no arguments");
    assert_eq!(run(&["definitely-not-a-command"]).0, 1, "unknown command");
    assert_eq!(run(&["scan"]).0, 1, "missing the folder");
}

#[test]
fn help_and_version_succeed() {
    let (code, stdout, _) = run(&["--help"]);
    assert_eq!(code, 0);
    for verb in ["scan", "inspect", "cleanup", "rename"] {
        assert!(
            stdout.contains(verb),
            "`{verb}` missing from help:\n{stdout}"
        );
    }

    assert_eq!(run(&["--version"]).0, 0);
}

/// Every flag the retired CLI accepted still parses, so existing invocations
/// keep working rather than failing at the argument layer.
#[test]
fn the_retired_flag_surface_still_parses() {
    let cases: &[&[&str]] = &[
        &["scan", "FOLDER", "--json", "--ignore", "Extras,Backdrops"],
        &["inspect", "FOLDER", "--json"],
        &[
            "cleanup",
            "FOLDER",
            "--json",
            "--apply",
            "--keep-container-title",
            "--keep-video-title",
            "--remove-audio-titles",
            "--remove-subtitle-titles",
            "--set-audio-language",
            "eng",
            "--set-subtitle-language",
            "eng",
        ],
        &[
            "rename",
            "FOLDER",
            "--template",
            "{series} - S{season:00}E{episode:00}",
            "--json",
            "--apply",
        ],
    ];

    for case in cases {
        let (code, _, stderr) = run(case);
        // The folder does not exist, so these stop at the runtime rather than
        // at parsing. What matters is which error it is.
        assert_eq!(code, 1, "{case:?} exited unexpectedly: {stderr}");
        assert!(
            !stderr.contains("unexpected argument") && !stderr.contains("unrecognized"),
            "{case:?} was rejected by the parser: {stderr}"
        );
    }
}

#[test]
fn a_missing_folder_is_reported_clearly() {
    let (code, _, stderr) = run(&["scan", "no-such-folder-here"]);
    assert_eq!(code, 1);
    assert!(
        stderr.contains("no such folder"),
        "unhelpful message: {stderr}"
    );
}

/// An empty folder is not an error, but a script should be able to skip its
/// next step without parsing output.
#[test]
fn an_empty_folder_scans_clean() {
    let directory = tempfile::tempdir().expect("temp dir");
    let config = directory.path().join("config");
    let media = directory.path().join("media");
    std::fs::create_dir_all(&media).expect("media dir");

    let (code, stdout, stderr) = run(&[
        "--config",
        config.to_str().expect("config path"),
        "scan",
        media.to_str().expect("media path"),
    ]);

    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains("Scanned 0 file(s)."), "{stdout}");
}
