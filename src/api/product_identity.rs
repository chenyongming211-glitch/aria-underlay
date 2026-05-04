use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::api::product_api::{
    ProductApiRequestMetadata, ProductSession, ProductSessionExtractor,
};
use crate::{UnderlayError, UnderlayResult};

const AUTHORIZATION_HEADER: &str = "authorization";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductAuthenticatedPrincipal {
    pub operator_id: String,
}

pub trait ProductIdentityVerifier: std::fmt::Debug + Send + Sync {
    fn verify_bearer_token(&self, token: &str) -> UnderlayResult<ProductAuthenticatedPrincipal>;
}

#[derive(Debug, Clone, Default)]
pub struct StaticProductIdentityVerifier {
    principals_by_token: BTreeMap<String, ProductAuthenticatedPrincipal>,
}

#[derive(Debug, Clone)]
pub struct BearerTokenProductSessionExtractor {
    verifier: Arc<dyn ProductIdentityVerifier>,
}

impl ProductAuthenticatedPrincipal {
    pub fn new(operator_id: impl Into<String>) -> Self {
        Self {
            operator_id: operator_id.into(),
        }
    }

    fn validate(&self) -> UnderlayResult<()> {
        validate_non_empty("product authenticated principal operator_id", &self.operator_id)
    }
}

impl StaticProductIdentityVerifier {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_token(
        mut self,
        token: impl Into<String>,
        principal: ProductAuthenticatedPrincipal,
    ) -> Self {
        self.principals_by_token.insert(token.into(), principal);
        self
    }
}

impl ProductIdentityVerifier for StaticProductIdentityVerifier {
    fn verify_bearer_token(&self, token: &str) -> UnderlayResult<ProductAuthenticatedPrincipal> {
        validate_non_empty("product bearer token", token).map_err(auth_error)?;
        let principal = self
            .principals_by_token
            .get(token)
            .cloned()
            .ok_or_else(|| {
                UnderlayError::AuthenticationFailed("product bearer token is not trusted".into())
            })?;
        principal.validate().map_err(auth_error)?;
        Ok(principal)
    }
}

impl BearerTokenProductSessionExtractor {
    pub fn new(verifier: Arc<dyn ProductIdentityVerifier>) -> Self {
        Self { verifier }
    }
}

impl ProductSessionExtractor for BearerTokenProductSessionExtractor {
    fn extract(&self, metadata: &ProductApiRequestMetadata) -> UnderlayResult<ProductSession> {
        validate_non_empty("product api request_id", &metadata.request_id)?;
        let token = bearer_token(&metadata.headers)?;
        let principal = self
            .verifier
            .verify_bearer_token(&token)?;
        Ok(ProductSession {
            operator_id: principal.operator_id,
        })
    }
}

fn bearer_token(headers: &BTreeMap<String, String>) -> UnderlayResult<String> {
    let value = header_value(headers, AUTHORIZATION_HEADER).ok_or_else(|| {
        UnderlayError::AuthenticationFailed(
            "missing required product Authorization bearer token".into(),
        )
    })?;
    let mut parts = value.split_whitespace();
    let Some(scheme) = parts.next() else {
        return Err(UnderlayError::AuthenticationFailed(
            "missing required product Authorization bearer token".into(),
        ));
    };
    if !scheme.eq_ignore_ascii_case("Bearer") {
        return Err(UnderlayError::AuthenticationFailed(
            "product Authorization header must use Bearer scheme".into(),
        ));
    }
    let Some(token) = parts.next() else {
        return Err(UnderlayError::AuthenticationFailed(
            "product Authorization bearer token must not be empty".into(),
        ));
    };
    if parts.next().is_some() {
        return Err(UnderlayError::AuthenticationFailed(
            "product Authorization bearer token must not contain whitespace".into(),
        ));
    }
    validate_non_empty("product bearer token", token).map_err(auth_error)?;
    Ok(token.to_string())
}

fn header_value(headers: &BTreeMap<String, String>, name: &str) -> Option<String> {
    headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn validate_non_empty(field: &str, value: &str) -> UnderlayResult<()> {
    if value.trim().is_empty() {
        return Err(UnderlayError::InvalidIntent(format!(
            "{field} must not be empty"
        )));
    }
    Ok(())
}

fn auth_error(error: UnderlayError) -> UnderlayError {
    match error {
        UnderlayError::AuthenticationFailed(_) => error,
        other => UnderlayError::AuthenticationFailed(other.to_string()),
    }
}
