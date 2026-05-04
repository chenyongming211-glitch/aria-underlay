use std::error::Error;
use std::path::PathBuf;

use aria_underlay::utils::time::now_unix_secs;
use aria_underlay::worker::daemon::UnderlayWorkerDaemon;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let config_path = worker_config_path()?;
    eprintln!(
        "ts={} level=info component=worker action=starting config_path={}",
        now_unix_secs(),
        config_path.display()
    );
    let report = match UnderlayWorkerDaemon::run_config_path_until_shutdown(
        config_path,
        shutdown_signal(),
    )
    .await
    {
        Ok(report) => report,
        Err(error) => {
            eprintln!(
                "ts={} level=error component=worker action=failed error={:?}",
                now_unix_secs(),
                error.to_string()
            );
            return Err(error.into());
        }
    };

    eprintln!(
        "ts={} level=info component=worker action=stopped",
        now_unix_secs()
    );
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
