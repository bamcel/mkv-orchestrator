use std::{path::PathBuf, sync::Arc};

use mkvo_runtime::{KeyringSecretStore, MkvoRuntime, MkvoRuntimeBuilder};
use tauri::{AppHandle, Manager, Runtime};

pub type RuntimeState = Arc<MkvoRuntime>;

pub fn compose_runtime<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<RuntimeState, Box<dyn std::error::Error>> {
    let app_data_root = app.path().app_data_dir()?;
    let app_config_root = app.path().app_config_dir()?;
    std::fs::create_dir_all(&app_data_root)?;
    std::fs::create_dir_all(&app_config_root)?;

    let media_root = initial_media_root(app, &app_data_root)?;
    let tools_root = app_config_root.join("tools");
    std::fs::create_dir_all(&tools_root)?;

    let mut builder = MkvoRuntimeBuilder::new(&media_root, &app_data_root)
        .app_name("MKV Orchestrator")
        .version(app.package_info().version.to_string())
        .tool_directory(tools_root);

    // Provider API keys belong in the OS credential store on desktop rather than
    // a plaintext file. A Linux desktop with no running Secret Service, or a
    // locked keychain, falls back to the file store — losing the user's keys at
    // save time would be worse than storing them as the server host does.
    let keyring = KeyringSecretStore::new("MKV Orchestrator");
    if keyring.is_usable() {
        builder = builder.secret_store(std::sync::Arc::new(keyring));
    } else {
        tracing::warn!(
            "OS credential store is unavailable; secrets will use the protected config file"
        );
    }

    let runtime = builder.build()?;

    Ok(Arc::new(runtime))
}

fn initial_media_root<R: Runtime>(
    app: &AppHandle<R>,
    app_data_root: &std::path::Path,
) -> Result<PathBuf, std::io::Error> {
    if let Ok(videos) = app.path().video_dir()
        && videos.is_dir()
    {
        return Ok(videos);
    }

    let fallback = app_data_root.join("media");
    std::fs::create_dir_all(&fallback)?;
    Ok(fallback)
}
