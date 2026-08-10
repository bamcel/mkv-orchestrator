//! Composing the shared runtime for a command line host.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use mkvo_runtime::{MkvoRuntime, MkvoRuntimeBuilder};

/// Where settings, cache, and logs live when nothing is specified.
///
/// The OS configuration directory rather than a hidden folder beside the media:
/// a CLI run against a NAS share should not scatter databases through it, and
/// sharing the store means keys configured once are available to every host.
fn default_config_dir() -> Result<PathBuf> {
    // The same variable the server host reads, so a machine that already sets
    // it for a container gets the same store here.
    if let Some(from_env) = std::env::var_os("MKVO_CONFIG_DIR")
        && !from_env.is_empty()
    {
        return Ok(PathBuf::from(from_env));
    }
    dirs::config_dir()
        .map(|base| base.join("mkv-orchestrator"))
        .context("no OS configuration directory; pass --config or set MKVO_CONFIG_DIR")
}

pub fn compose(config: Option<&Path>, target: &Path) -> Result<MkvoRuntime> {
    let config_root = match config {
        Some(path) => path.to_path_buf(),
        None => default_config_dir()?,
    };
    std::fs::create_dir_all(&config_root)
        .with_context(|| format!("cannot create {}", config_root.display()))?;

    let target = target
        .canonicalize()
        .with_context(|| format!("no such folder: {}", target.display()))?;
    if !target.is_dir() {
        anyhow::bail!("not a folder: {}", target.display());
    }

    MkvoRuntimeBuilder::new(&target, &config_root)
        .app_name("MKV Orchestrator CLI")
        .version(env!("CARGO_PKG_VERSION"))
        // Naming a folder on the command line is the authorization: the caller
        // already has a shell on this machine, and confining them to roots
        // configured elsewhere would make the tool useless for one-off runs.
        // Everything outside that folder is still refused.
        .unrestricted_browsing()
        .authorized_root(&target, true)
        .build()
        .context("could not start the MKVO runtime")
}
