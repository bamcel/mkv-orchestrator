use std::{path::PathBuf, sync::Arc};

use mkvo_runtime::{MkvoRuntime, MkvoRuntimeBuilder};
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

    let runtime = MkvoRuntimeBuilder::new(&media_root, &app_data_root)
        .app_name("MKV Orchestrator")
        .version(app.package_info().version.to_string())
        .tool_directory(tools_root)
        .build()?;

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
