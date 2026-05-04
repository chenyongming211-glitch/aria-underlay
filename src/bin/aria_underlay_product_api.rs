use std::error::Error;
use std::path::PathBuf;

use aria_underlay::api::product_http_server::ProductHttpServer;
use aria_underlay::api::product_server_config::ProductApiServerConfig;
use aria_underlay::utils::time::now_unix_secs;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let config_path = product_api_config_path()?;
    eprintln!(
        "ts={} level=info component=product_api action=starting config_path={}",
        now_unix_secs(),
        config_path.display()
    );
    let config = ProductApiServerConfig::from_path(&config_path)?;
    let server = ProductHttpServer::new(config.router()?, config.listener_config())?;
    let bind_addr = server.config().bind_addr;
    eprintln!(
        "ts={} level=info component=product_api action=listening bind_addr={}",
        now_unix_secs(),
        bind_addr
    );
    if let Err(error) = server.serve_until_shutdown(shutdown_signal()).await {
        eprintln!(
            "ts={} level=error component=product_api action=failed error={:?}",
            now_unix_secs(),
            error.to_string()
        );
        return Err(error.into());
    }
    eprintln!(
        "ts={} level=info component=product_api action=stopped",
        now_unix_secs()
    );
    Ok(())
}

fn product_api_config_path() -> Result<PathBuf, Box<dyn Error>> {
    if let Some(path) = std::env::args_os().nth(1) {
        return Ok(path.into());
    }
    if let Ok(path) = std::env::var("ARIA_UNDERLAY_PRODUCT_API_CONFIG") {
        return Ok(path.into());
    }
    Err("usage: aria-underlay-product-api <config.json>".into())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}
