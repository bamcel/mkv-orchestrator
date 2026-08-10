use std::{future::IntoFuture, sync::Arc, time::Duration};

use anyhow::{Context, Result};
use mkvo_server::{ServerConfig, build_router, build_runtime, shutdown_signal};
use tokio::{net::TcpListener, sync::oneshot, time::timeout};
use tracing::{info, warn};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_target(false)
        .init();

    let config = Arc::new(ServerConfig::from_env()?);
    let runtime = Arc::new(build_runtime(&config)?);
    let bind = config.bind;
    let app = build_router(Arc::clone(&config), runtime);
    let listener = TcpListener::bind(bind)
        .await
        .with_context(|| format!("failed to bind MKVO server at {bind}"))?;

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let server = axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = shutdown_rx.await;
        })
        .into_future();
    tokio::pin!(server);

    info!(%bind, "MKVO Rust server listening");
    tokio::select! {
        result = &mut server => result.context("MKVO server failed")?,
        () = shutdown_signal() => {
            info!("shutdown requested; draining HTTP connections");
            let _ = shutdown_tx.send(());
            match timeout(Duration::from_secs(config.graceful_shutdown_seconds), &mut server).await {
                Ok(result) => result.context("MKVO server failed during graceful shutdown")?,
                Err(_) => warn!(
                    timeout_seconds = config.graceful_shutdown_seconds,
                    "graceful shutdown deadline reached"
                ),
            }
        }
    }
    Ok(())
}
