use std::error::Error;
use std::path::PathBuf;

use aria_underlay::worker::daemon::UnderlayWorkerDaemon;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let config_path = worker_config_path()?;
    let report = UnderlayWorkerDaemon::run_config_path_until_shutdown(
        config_path,
        shutdown_signal(),
    )
        .await?;

    println!("{report:?}");
    Ok(())
}

fn worker_config_path() -> Result<PathBuf, Box<dyn Error>> {
    if let Some(path) = std::env::args_os().nth(1) {
        return Ok(path.into());
    }
    if let Ok(path) = std::env::var("ARIA_UNDERLAY_WORKER_CONFIG") {
        return Ok(path.into());
    }
    Err("usage: aria_underlay_worker <config.json>".into())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}
