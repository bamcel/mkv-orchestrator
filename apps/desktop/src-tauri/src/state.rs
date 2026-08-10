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

    let media_root = placeholder_media_root(&app_data_root)?;
    let tools_root = app_config_root.join("tools");
    std::fs::create_dir_all(&tools_root)?;

    let mut builder = MkvoRuntimeBuilder::new(&media_root, &app_data_root)
        .app_name("MKV Orchestrator")
        .version(app.package_info().version.to_string())
        .tool_directory(tools_root)
        // The desktop user already has full filesystem access through their
        // own file manager, so confining the in-app browser only prevents them
        // reaching their own library.
        .unrestricted_browsing();

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

/// The desktop has no library until the user names one.
///
/// This used to default to the OS Videos folder, which guessed at a library the
/// user may not keep there and then anchored browsing to it. The runtime now
/// takes its root from the library folders in Settings, and reports none until
/// there are some, so browsing opens at the volume list instead. What remains
/// here is only a placeholder for the parts of the runtime that need some path
/// to exist; it is never presented as the user's library.
fn placeholder_media_root(app_data_root: &std::path::Path) -> Result<PathBuf, std::io::Error> {
    let placeholder = app_data_root.join("media");
    std::fs::create_dir_all(&placeholder)?;
    Ok(placeholder)
}
