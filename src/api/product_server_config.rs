use std::collections::BTreeMap;
use std::error::Error;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Deserialize;

use crate::api::product_api::ProductOpsApi;
use crate::api::product_http::ProductHttpRouter;
use crate::api::product_http_server::ProductHttpListenerConfig;
use crate::api::product_identity::{
    BearerTokenProductSessionExtractor, ProductAuthenticatedPrincipal, ProductIdentityVerifier,
    StaticProductIdentityVerifier,
};
use crate::telemetry::{JsonFileOperationSummaryStore, JsonFileProductAuditStore};
use crate::{UnderlayError, UnderlayResult};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductApiServerConfig {
    pub bind_addr: SocketAddr,
    #[serde(default = "default_max_body_bytes")]
    pub max_body_bytes: usize,
    pub operation_summary_path: PathBuf,
    pub product_audit_path: PathBuf,
    #[serde(default)]
    pub static_tokens: BTreeMap<String, ProductAuthenticatedPrincipal>,
}

impl ProductApiServerConfig {
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, Box<dyn Error>> {
        let payload = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&payload)?)
    }

    pub fn validate(&self) -> UnderlayResult<()> {
        self.listener_config().validate()?;
        validate_non_empty_path("operation_summary_path", &self.operation_summary_path)?;
        validate_non_empty_path("product_audit_path", &self.product_audit_path)?;
        if self.static_tokens.is_empty() {
            return Err(UnderlayError::InvalidIntent(
                "product API config static_tokens must not be empty".into(),
            ));
        }
        if !self.bind_addr.ip().is_loopback() {
            return Err(UnderlayError::InvalidIntent(
                "product API must bind to a loopback address".into(),
            ));
        }
        Ok(())
    }

    pub fn listener_config(&self) -> ProductHttpListenerConfig {
        ProductHttpListenerConfig {
            bind_addr: self.bind_addr,
            max_body_bytes: self.max_body_bytes,
        }
    }

    pub fn router(&self) -> Result<ProductHttpRouter, Box<dyn Error>> {
        self.validate()?;
        let verifier = self.identity_verifier()?;
        Ok(ProductHttpRouter::new(ProductOpsApi::new(
            Arc::new(BearerTokenProductSessionExtractor::new(verifier)),
            Arc::new(JsonFileOperationSummaryStore::new(
                self.operation_summary_path.clone(),
            )),
            Arc::new(JsonFileProductAuditStore::new(
                self.product_audit_path.clone(),
            )),
        )))
    }

    fn identity_verifier(&self) -> Result<Arc<dyn ProductIdentityVerifier>, Box<dyn Error>> {
        let mut verifier = StaticProductIdentityVerifier::new();
        for (token, principal) in &self.static_tokens {
            verifier = verifier.with_token(token.clone(), principal.clone());
        }
        Ok(Arc::new(verifier))
    }
}

fn validate_non_empty_path(field: &str, path: &Path) -> UnderlayResult<()> {
    if path.as_os_str().is_empty() {
        return Err(UnderlayError::InvalidIntent(format!(
            "product API {field} must not be empty"
        )));
    }
    Ok(())
}

fn default_max_body_bytes() -> usize {
    1024 * 1024
}
