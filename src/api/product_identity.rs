use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::api::product_api::{
    ProductApiRequestMetadata, ProductSession, ProductSessionExtractor,
};
use crate::authz::RbacRole;
use crate::utils::time::now_unix_secs;
use crate::{UnderlayError, UnderlayResult};

const AUTHORIZATION_HEADER: &str = "authorization";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductAuthenticatedPrincipal {
    pub operator_id: String,
    pub role: RbacRole,
    pub issuer: Option<String>,
    pub subject: Option<String>,
    pub session_id: Option<String>,
    pub expires_at_unix_secs: Option<u64>,
}

pub trait ProductIdentityVerifier: std::fmt::Debug + Send + Sync {
    fn verify_bearer_token(
        &self,
        token: &str,
        now_unix_secs: u64,
    ) -> UnderlayResult<ProductAuthenticatedPrincipal>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProductJwtAlgorithm {
    HS256,
    HS384,
    HS512,
    RS256,
    RS384,
    RS512,
    PS256,
    PS384,
    PS512,
    ES256,
    ES384,
    EdDSA,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductJwtJwksVerifierConfig {
    pub issuer: String,
    pub audiences: Vec<String>,
    #[serde(default = "default_product_jwt_algorithms")]
    pub allowed_algorithms: Vec<ProductJwtAlgorithm>,
    #[serde(default = "default_product_jwt_role_claim")]
    pub role_claim: String,
    #[serde(default)]
    pub operator_id_claim: Option<String>,
    #[serde(default)]
    pub session_id_claim: Option<String>,
    #[serde(default = "default_product_jwt_leeway_secs")]
    pub leeway_secs: u64,
    #[serde(default = "default_product_jwt_role_mappings")]
    pub role_mappings: BTreeMap<String, RbacRole>,
    pub jwks: JwkSet,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductJwtJwksFileVerifierConfig {
    pub issuer: String,
    pub audiences: Vec<String>,
    #[serde(default = "default_product_jwt_algorithms")]
    pub allowed_algorithms: Vec<ProductJwtAlgorithm>,
    #[serde(default = "default_product_jwt_role_claim")]
    pub role_claim: String,
    #[serde(default)]
    pub operator_id_claim: Option<String>,
    #[serde(default)]
    pub session_id_claim: Option<String>,
    #[serde(default = "default_product_jwt_leeway_secs")]
    pub leeway_secs: u64,
    #[serde(default = "default_product_jwt_role_mappings")]
    pub role_mappings: BTreeMap<String, RbacRole>,
    pub jwks_path: PathBuf,
    #[serde(default = "default_product_jwks_refresh_interval_secs")]
    pub refresh_interval_secs: u64,
    #[serde(default = "default_product_jwks_max_stale_secs")]
    pub max_stale_secs: u64,
}

#[derive(Debug, Clone)]
pub struct JwtJwksProductIdentityVerifier {
    config: ProductJwtJwksVerifierConfig,
}

#[derive(Debug)]
pub struct RefreshingJwtJwksProductIdentityVerifier {
    config: ProductJwtJwksFileVerifierConfig,
    state: Mutex<RefreshingJwtJwksState>,
}

#[derive(Debug)]
struct RefreshingJwtJwksState {
    verifier: JwtJwksProductIdentityVerifier,
    loaded_at_unix_secs: u64,
    last_checked_at_unix_secs: u64,
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
    pub fn new(operator_id: impl Into<String>, role: RbacRole) -> Self {
        Self {
            operator_id: operator_id.into(),
            role,
            issuer: None,
            subject: None,
            session_id: None,
            expires_at_unix_secs: None,
        }
    }

    pub fn with_issuer(mut self, issuer: impl Into<String>) -> Self {
        self.issuer = Some(issuer.into());
        self
    }

    pub fn with_subject(mut self, subject: impl Into<String>) -> Self {
        self.subject = Some(subject.into());
        self
    }

    pub fn with_session_id(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    pub fn with_expires_at_unix_secs(mut self, expires_at_unix_secs: u64) -> Self {
        self.expires_at_unix_secs = Some(expires_at_unix_secs);
        self
    }

    fn validate(&self) -> UnderlayResult<()> {
        validate_non_empty("product authenticated principal operator_id", &self.operator_id)
    }

    fn is_expired_at(&self, now_unix_secs: u64) -> bool {
        self.expires_at_unix_secs
            .map(|expires_at| expires_at <= now_unix_secs)
            .unwrap_or(false)
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

impl JwtJwksProductIdentityVerifier {
    pub fn new(config: ProductJwtJwksVerifierConfig) -> UnderlayResult<Self> {
        validate_jwt_config(&config)?;
        Ok(Self { config })
    }
}

impl RefreshingJwtJwksProductIdentityVerifier {
    pub fn new(config: ProductJwtJwksFileVerifierConfig) -> UnderlayResult<Self> {
        validate_jwt_file_config(&config)?;
        let verifier = JwtJwksProductIdentityVerifier::new(config.load_inline_config()?)?;
        let now = now_unix_secs();
        Ok(Self {
            config,
            state: Mutex::new(RefreshingJwtJwksState {
                verifier,
                loaded_at_unix_secs: now,
                last_checked_at_unix_secs: now,
            }),
        })
    }

    fn refresh_if_due(
        &self,
        state: &mut RefreshingJwtJwksState,
        now_unix_secs: u64,
    ) -> UnderlayResult<()> {
        let refresh_due = now_unix_secs
            >= state
                .last_checked_at_unix_secs
                .saturating_add(self.config.refresh_interval_secs);
        if !refresh_due {
            return Ok(());
        }
        state.last_checked_at_unix_secs = now_unix_secs;
        match self
            .config
            .load_inline_config()
            .and_then(JwtJwksProductIdentityVerifier::new)
        {
            Ok(verifier) => {
                state.verifier = verifier;
                state.loaded_at_unix_secs = now_unix_secs;
                Ok(())
            }
            Err(error) => {
                if now_unix_secs
                    > state
                        .loaded_at_unix_secs
                        .saturating_add(self.config.max_stale_secs)
                {
                    Err(UnderlayError::AuthenticationFailed(format!(
                        "product JWKS refresh failed and cached keys are stale: {error}"
                    )))
                } else {
                    Ok(())
                }
            }
        }
    }
}

impl ProductIdentityVerifier for JwtJwksProductIdentityVerifier {
    fn verify_bearer_token(
        &self,
        token: &str,
        now_unix_secs: u64,
    ) -> UnderlayResult<ProductAuthenticatedPrincipal> {
        validate_non_empty("product bearer token", token).map_err(auth_error)?;
        let header = decode_header(token).map_err(jwt_auth_error)?;
        let Some(kid) = header.kid.as_deref().filter(|kid| !kid.trim().is_empty()) else {
            return Err(UnderlayError::AuthenticationFailed(
                "product JWT header must include kid".into(),
            ));
        };
        let algorithm = ProductJwtAlgorithm::from_algorithm(header.alg).ok_or_else(|| {
            UnderlayError::AuthenticationFailed(format!(
                "product JWT algorithm {:?} is not supported by product identity verifier",
                header.alg
            ))
        })?;
        if !self.config.allowed_algorithms.contains(&algorithm) {
            return Err(UnderlayError::AuthenticationFailed(format!(
                "product JWT algorithm {:?} is not allowed",
                header.alg
            )));
        }
        let jwk = self.config.jwks.find(kid).ok_or_else(|| {
            UnderlayError::AuthenticationFailed(format!(
                "product JWT kid {kid} is not present in configured JWKS"
            ))
        })?;
        let key = DecodingKey::from_jwk(jwk).map_err(jwt_auth_error)?;
        let mut validation = Validation::new(header.alg);
        validation.algorithms = self
            .config
            .allowed_algorithms
            .iter()
            .map(|algorithm| algorithm.to_algorithm())
            .collect();
        validation.leeway = self.config.leeway_secs;
        validation.validate_nbf = true;
        validation.set_issuer(&[self.config.issuer.as_str()]);
        let audiences = self
            .config
            .audiences
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        validation.set_audience(&audiences);
        validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);

        let decoded = decode::<ProductJwtClaims>(token, &key, &validation)
            .map_err(jwt_auth_error)?;
        claims_to_principal(&self.config, decoded.claims, now_unix_secs)
    }
}

impl ProductIdentityVerifier for RefreshingJwtJwksProductIdentityVerifier {
    fn verify_bearer_token(
        &self,
        token: &str,
        now_unix_secs: u64,
    ) -> UnderlayResult<ProductAuthenticatedPrincipal> {
        let mut state = self.state.lock().map_err(|_| {
            UnderlayError::AuthenticationFailed("product JWKS refresh state mutex poisoned".into())
        })?;
        self.refresh_if_due(&mut state, now_unix_secs)?;
        state.verifier.verify_bearer_token(token, now_unix_secs)
    }
}

impl ProductIdentityVerifier for StaticProductIdentityVerifier {
    fn verify_bearer_token(
        &self,
        token: &str,
        now_unix_secs: u64,
    ) -> UnderlayResult<ProductAuthenticatedPrincipal> {
        validate_non_empty("product bearer token", token).map_err(auth_error)?;
        let principal = self
            .principals_by_token
            .get(token)
            .cloned()
            .ok_or_else(|| {
                UnderlayError::AuthenticationFailed("product bearer token is not trusted".into())
            })?;
        principal.validate().map_err(auth_error)?;
        if principal.is_expired_at(now_unix_secs) {
            return Err(UnderlayError::AuthenticationFailed(
                "product bearer token is expired".into(),
            ));
        }
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
            .verify_bearer_token(&token, now_unix_secs())?;
        Ok(ProductSession {
            operator_id: principal.operator_id,
            role: principal.role,
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

fn validate_jwt_config(config: &ProductJwtJwksVerifierConfig) -> UnderlayResult<()> {
    validate_non_empty("product JWT issuer", &config.issuer)?;
    if config.audiences.is_empty() {
        return Err(UnderlayError::InvalidIntent(
            "product JWT audiences must not be empty".into(),
        ));
    }
    for audience in &config.audiences {
        validate_non_empty("product JWT audience", audience)?;
    }
    if config.allowed_algorithms.is_empty() {
        return Err(UnderlayError::InvalidIntent(
            "product JWT allowed_algorithms must not be empty".into(),
        ));
    }
    validate_non_empty("product JWT role_claim", &config.role_claim)?;
    if let Some(operator_id_claim) = &config.operator_id_claim {
        validate_non_empty("product JWT operator_id_claim", operator_id_claim)?;
    }
    if let Some(session_id_claim) = &config.session_id_claim {
        validate_non_empty("product JWT session_id_claim", session_id_claim)?;
    }
    if config.role_mappings.is_empty() {
        return Err(UnderlayError::InvalidIntent(
            "product JWT role_mappings must not be empty".into(),
        ));
    }
    if config.jwks.keys.is_empty() {
        return Err(UnderlayError::InvalidIntent(
            "product JWT JWKS must contain at least one key".into(),
        ));
    }
    for jwk in &config.jwks.keys {
        let key_id = jwk.common.key_id.as_deref().unwrap_or_default();
        validate_non_empty("product JWT JWKS key kid", key_id)?;
        DecodingKey::from_jwk(jwk).map_err(|err| {
            UnderlayError::InvalidIntent(format!(
                "product JWT JWKS key {key_id} is not usable: {err}"
            ))
        })?;
    }
    Ok(())
}

fn validate_jwt_file_config(config: &ProductJwtJwksFileVerifierConfig) -> UnderlayResult<()> {
    validate_non_empty("product JWT issuer", &config.issuer)?;
    if config.audiences.is_empty() {
        return Err(UnderlayError::InvalidIntent(
            "product JWT audiences must not be empty".into(),
        ));
    }
    for audience in &config.audiences {
        validate_non_empty("product JWT audience", audience)?;
    }
    if config.allowed_algorithms.is_empty() {
        return Err(UnderlayError::InvalidIntent(
            "product JWT allowed_algorithms must not be empty".into(),
        ));
    }
    validate_non_empty("product JWT role_claim", &config.role_claim)?;
    if let Some(operator_id_claim) = &config.operator_id_claim {
        validate_non_empty("product JWT operator_id_claim", operator_id_claim)?;
    }
    if let Some(session_id_claim) = &config.session_id_claim {
        validate_non_empty("product JWT session_id_claim", session_id_claim)?;
    }
    if config.role_mappings.is_empty() {
        return Err(UnderlayError::InvalidIntent(
            "product JWT role_mappings must not be empty".into(),
        ));
    }
    if config.jwks_path.as_os_str().is_empty() {
        return Err(UnderlayError::InvalidIntent(
            "product JWT jwks_path must not be empty".into(),
        ));
    }
    if config.refresh_interval_secs == 0 {
        return Err(UnderlayError::InvalidIntent(
            "product JWT refresh_interval_secs must be greater than zero".into(),
        ));
    }
    if config.max_stale_secs < config.refresh_interval_secs {
        return Err(UnderlayError::InvalidIntent(
            "product JWT max_stale_secs must be greater than or equal to refresh_interval_secs"
                .into(),
        ));
    }
    Ok(())
}

impl ProductJwtJwksFileVerifierConfig {
    pub fn validate_static(&self) -> UnderlayResult<()> {
        validate_jwt_file_config(self)
    }

    fn load_inline_config(&self) -> UnderlayResult<ProductJwtJwksVerifierConfig> {
        let payload = fs::read_to_string(&self.jwks_path).map_err(|err| {
            UnderlayError::InvalidIntent(format!(
                "read product JWT JWKS {:?}: {err}",
                self.jwks_path
            ))
        })?;
        let jwks = serde_json::from_str::<JwkSet>(&payload).map_err(|err| {
            UnderlayError::InvalidIntent(format!(
                "parse product JWT JWKS {:?}: {err}",
                self.jwks_path
            ))
        })?;
        Ok(ProductJwtJwksVerifierConfig {
            issuer: self.issuer.clone(),
            audiences: self.audiences.clone(),
            allowed_algorithms: self.allowed_algorithms.clone(),
            role_claim: self.role_claim.clone(),
            operator_id_claim: self.operator_id_claim.clone(),
            session_id_claim: self.session_id_claim.clone(),
            leeway_secs: self.leeway_secs,
            role_mappings: self.role_mappings.clone(),
            jwks,
        })
    }
}

fn claims_to_principal(
    config: &ProductJwtJwksVerifierConfig,
    claims: ProductJwtClaims,
    now_unix_secs: u64,
) -> UnderlayResult<ProductAuthenticatedPrincipal> {
    if claims.exp.saturating_add(config.leeway_secs) <= now_unix_secs {
        return Err(UnderlayError::AuthenticationFailed(
            "product JWT is expired".into(),
        ));
    }
    if claims
        .nbf
        .map(|not_before| not_before > now_unix_secs + config.leeway_secs)
        .unwrap_or(false)
    {
        return Err(UnderlayError::AuthenticationFailed(
            "product JWT is not valid yet".into(),
        ));
    }
    let operator_id = config
        .operator_id_claim
        .as_deref()
        .and_then(|claim| string_claim(&claims, claim))
        .unwrap_or_else(|| claims.sub.clone());
    validate_non_empty("product JWT operator id", &operator_id).map_err(auth_error)?;
    let role = mapped_role(config, &claims)?;
    let mut principal = ProductAuthenticatedPrincipal::new(operator_id, role)
        .with_issuer(claims.iss.clone())
        .with_subject(claims.sub.clone())
        .with_expires_at_unix_secs(claims.exp);
    if let Some(session_id_claim) = &config.session_id_claim {
        if let Some(session_id) = string_claim(&claims, session_id_claim) {
            principal = principal.with_session_id(session_id);
        }
    }
    principal.validate().map_err(auth_error)?;
    Ok(principal)
}

fn mapped_role(
    config: &ProductJwtJwksVerifierConfig,
    claims: &ProductJwtClaims,
) -> UnderlayResult<RbacRole> {
    let Some(value) = claims.extra.get(&config.role_claim) else {
        return Err(UnderlayError::AuthenticationFailed(format!(
            "product JWT is missing role claim {}",
            config.role_claim
        )));
    };
    let mut mapped = Vec::new();
    match value {
        Value::String(role) => {
            if let Some(mapped_role) = config.role_mappings.get(role) {
                mapped.push(mapped_role.clone());
            }
        }
        Value::Array(values) => {
            for value in values {
                if let Some(role) = value.as_str() {
                    if let Some(mapped_role) = config.role_mappings.get(role) {
                        mapped.push(mapped_role.clone());
                    }
                }
            }
            mapped.sort();
            mapped.dedup();
        }
        _ => {}
    }
    match mapped.as_slice() {
        [role] => Ok(role.clone()),
        [] => Err(UnderlayError::AuthenticationFailed(format!(
            "product JWT role claim {} is not mapped to an underlay role",
            config.role_claim
        ))),
        _ => Err(UnderlayError::AuthenticationFailed(format!(
            "product JWT role claim {} maps to multiple underlay roles",
            config.role_claim
        ))),
    }
}

fn string_claim(claims: &ProductJwtClaims, name: &str) -> Option<String> {
    match name {
        "iss" => Some(claims.iss.clone()),
        "sub" => Some(claims.sub.clone()),
        other => claims
            .extra
            .get(other)
            .and_then(Value::as_str)
            .map(ToString::to_string),
    }
}

impl ProductJwtAlgorithm {
    fn to_algorithm(self) -> Algorithm {
        match self {
            Self::HS256 => Algorithm::HS256,
            Self::HS384 => Algorithm::HS384,
            Self::HS512 => Algorithm::HS512,
            Self::RS256 => Algorithm::RS256,
            Self::RS384 => Algorithm::RS384,
            Self::RS512 => Algorithm::RS512,
            Self::PS256 => Algorithm::PS256,
            Self::PS384 => Algorithm::PS384,
            Self::PS512 => Algorithm::PS512,
            Self::ES256 => Algorithm::ES256,
            Self::ES384 => Algorithm::ES384,
            Self::EdDSA => Algorithm::EdDSA,
        }
    }

    fn from_algorithm(algorithm: Algorithm) -> Option<Self> {
        match algorithm {
            Algorithm::HS256 => Some(Self::HS256),
            Algorithm::HS384 => Some(Self::HS384),
            Algorithm::HS512 => Some(Self::HS512),
            Algorithm::RS256 => Some(Self::RS256),
            Algorithm::RS384 => Some(Self::RS384),
            Algorithm::RS512 => Some(Self::RS512),
            Algorithm::PS256 => Some(Self::PS256),
            Algorithm::PS384 => Some(Self::PS384),
            Algorithm::PS512 => Some(Self::PS512),
            Algorithm::ES256 => Some(Self::ES256),
            Algorithm::ES384 => Some(Self::ES384),
            Algorithm::EdDSA => Some(Self::EdDSA),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct ProductJwtClaims {
    iss: String,
    sub: String,
    exp: u64,
    #[serde(default)]
    nbf: Option<u64>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

fn default_product_jwt_algorithms() -> Vec<ProductJwtAlgorithm> {
    vec![ProductJwtAlgorithm::RS256]
}

fn default_product_jwt_role_claim() -> String {
    "aria_role".into()
}

fn default_product_jwt_leeway_secs() -> u64 {
    60
}

fn default_product_jwks_refresh_interval_secs() -> u64 {
    300
}

fn default_product_jwks_max_stale_secs() -> u64 {
    3600
}

fn default_product_jwt_role_mappings() -> BTreeMap<String, RbacRole> {
    BTreeMap::from([
        ("Viewer".into(), RbacRole::Viewer),
        ("viewer".into(), RbacRole::Viewer),
        ("Operator".into(), RbacRole::Operator),
        ("operator".into(), RbacRole::Operator),
        ("BreakGlassOperator".into(), RbacRole::BreakGlassOperator),
        ("break-glass-operator".into(), RbacRole::BreakGlassOperator),
        ("break_glass_operator".into(), RbacRole::BreakGlassOperator),
        ("Admin".into(), RbacRole::Admin),
        ("admin".into(), RbacRole::Admin),
        ("Auditor".into(), RbacRole::Auditor),
        ("auditor".into(), RbacRole::Auditor),
    ])
}

fn jwt_auth_error(error: jsonwebtoken::errors::Error) -> UnderlayError {
    UnderlayError::AuthenticationFailed(format!("product JWT verification failed: {error}"))
}

fn auth_error(error: UnderlayError) -> UnderlayError {
    match error {
        UnderlayError::AuthenticationFailed(_) => error,
        other => UnderlayError::AuthenticationFailed(other.to_string()),
    }
}
