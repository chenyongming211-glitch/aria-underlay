use std::collections::BTreeMap;
use std::error::Error;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use aria_underlay::api::product_api::ProductOpsApi;
use aria_underlay::api::product_http::ProductHttpRouter;
use aria_underlay::api::product_http_server::{
    ProductHttpListenerConfig, ProductHttpServer,
};
use aria_underlay::api::product_identity::{
    BearerTokenProductSessionExtractor, ProductAuthenticatedPrincipal,
    StaticProductIdentityVerifier,
};
use aria_underlay::telemetry::{
    JsonFileOperationSummaryStore, JsonFileProductAuditStore,
};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
struct ProductApiServerConfig {
    bind_addr: SocketAddr,
    #[serde(default = "default_max_body_bytes")]
    max_body_bytes: usize,
    operation_summary_path: PathBuf,
    product_audit_path: PathBuf,
    #[serde(default)]
    static_tokens: BTreeMap<String, ProductAuthenticatedPrincipal>,
}

impl ProductApiServerConfig {
    fn from_path(path: impl AsRef<Path>) -> Result<Self, Box<dyn Error>> {
        let payload = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&payload)?)
    }

    fn listener_config(&self) -> ProductHttpListenerConfig {
        ProductHttpListenerConfig {
            bind_addr: self.bind_addr,
            max_body_bytes: self.max_body_bytes,
        }
    }

    fn router(&self) -> ProductHttpRouter {
        let mut verifier = StaticProductIdentityVerifier::new();
        for (token, principal) in &self.static_tokens {
            verifier = verifier.with_token(token.clone(), principal.clone());
        }
        ProductHttpRouter::new(ProductOpsApi::new(
            Arc::new(BearerTokenProductSessionExtractor::new(Arc::new(verifier))),
            Arc::new(JsonFileOperationSummaryStore::new(
                self.operation_summary_path.clone(),
            )),
            Arc::new(JsonFileProductAuditStore::new(
                self.product_audit_path.clone(),
            )),
        ))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let config_path = product_api_config_path()?;
    let config = ProductApiServerConfig::from_path(&config_path)?;
    let server = ProductHttpServer::new(config.router(), config.listener_config())?;
    server.serve_until_shutdown(shutdown_signal()).await?;
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

fn default_max_body_bytes() -> usize {
    1024 * 1024
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}
