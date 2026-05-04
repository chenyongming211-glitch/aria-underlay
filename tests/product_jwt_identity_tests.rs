use std::collections::BTreeMap;

use aria_underlay::api::product_identity::{
    JwtJwksProductIdentityVerifier, ProductIdentityVerifier, ProductJwtAlgorithm,
    ProductJwtJwksFileVerifierConfig, ProductJwtJwksVerifierConfig,
    RefreshingJwtJwksProductIdentityVerifier,
};
use aria_underlay::authz::RbacRole;
use aria_underlay::UnderlayError;
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::Serialize;
use serde_json::json;

const TEST_SECRET: &[u8] = b"abcdefghijklmnopqrstuvwxyz012345";
const TEST_SECRET_JWK_VALUE: &str = "YWJjZGVmZ2hpamtsbW5vcHFyc3R1dnd4eXowMTIzNDU";
const ROTATED_SECRET: &[u8] = b"12345678901234567890123456789012";
const ROTATED_SECRET_JWK_VALUE: &str = "MTIzNDU2Nzg5MDEyMzQ1Njc4OTAxMjM0NTY3ODkwMTI";

#[test]
fn jwt_jwks_verifier_accepts_valid_signed_token() {
    let verifier = jwt_verifier();
    let token = signed_token("local-hs", "https://idp.example", "aria-product-api", "Viewer");

    let principal = verifier
        .verify_bearer_token(&token, 1_800_000_000)
        .expect("valid JWT should authenticate");

    assert_eq!(principal.operator_id, "viewer-a");
    assert_eq!(principal.role, RbacRole::Viewer);
    assert_eq!(principal.issuer.as_deref(), Some("https://idp.example"));
    assert_eq!(principal.subject.as_deref(), Some("subject-viewer-a"));
    assert_eq!(principal.session_id.as_deref(), Some("session-viewer-a"));
    assert_eq!(principal.expires_at_unix_secs, Some(4_102_444_800));
}

#[test]
fn jwt_jwks_verifier_rejects_wrong_audience() {
    let verifier = jwt_verifier();
    let token = signed_token("local-hs", "https://idp.example", "wrong-audience", "Viewer");

    let err = verifier
        .verify_bearer_token(&token, 1_800_000_000)
        .expect_err("wrong audience should fail closed");

    assert!(matches!(err, UnderlayError::AuthenticationFailed(_)));
}

#[test]
fn jwt_jwks_verifier_rejects_unknown_kid() {
    let verifier = jwt_verifier();
    let token = signed_token("unknown-kid", "https://idp.example", "aria-product-api", "Viewer");

    let err = verifier
        .verify_bearer_token(&token, 1_800_000_000)
        .expect_err("unknown kid should fail closed");

    assert!(matches!(err, UnderlayError::AuthenticationFailed(_)));
}

#[test]
fn jwt_jwks_verifier_rejects_unmapped_role() {
    let verifier = jwt_verifier();
    let token = signed_token("local-hs", "https://idp.example", "aria-product-api", "Root");

    let err = verifier
        .verify_bearer_token(&token, 1_800_000_000)
        .expect_err("unmapped role should fail closed");

    assert!(matches!(err, UnderlayError::AuthenticationFailed(_)));
}

#[test]
fn jwt_jwks_config_rejects_empty_jwks() {
    let config = ProductJwtJwksVerifierConfig {
        issuer: "https://idp.example".into(),
        audiences: vec!["aria-product-api".into()],
        allowed_algorithms: vec![ProductJwtAlgorithm::HS256],
        role_claim: "aria_role".into(),
        operator_id_claim: Some("preferred_username".into()),
        session_id_claim: Some("sid".into()),
        leeway_secs: 30,
        role_mappings: role_mappings(),
        jwks: serde_json::from_value(json!({ "keys": [] })).expect("jwks should parse"),
    };

    let err = JwtJwksProductIdentityVerifier::new(config)
        .expect_err("empty jwks should fail configuration validation");

    assert!(matches!(err, UnderlayError::InvalidIntent(_)));
}

#[test]
fn refreshing_jwks_verifier_accepts_rotated_key_without_restart() {
    let temp = temp_test_dir("jwks-refresh-rotation");
    let jwks_path = temp.join("jwks.json");
    std::fs::create_dir_all(&temp).expect("temp dir should be created");
    write_jwks(&jwks_path, "local-hs", TEST_SECRET_JWK_VALUE);
    let verifier = RefreshingJwtJwksProductIdentityVerifier::new(jwt_file_config(&jwks_path))
        .expect("refreshing verifier should load initial JWKS");

    let initial_token = signed_token_with_secret(
        "local-hs",
        TEST_SECRET,
        "https://idp.example",
        "aria-product-api",
        "Viewer",
    );
    let principal = verifier
        .verify_bearer_token(&initial_token, 1_800_000_000)
        .expect("initial key should authenticate");
    assert_eq!(principal.operator_id, "viewer-a");

    write_jwks(&jwks_path, "rotated-hs", ROTATED_SECRET_JWK_VALUE);
    let rotated_token = signed_token_with_secret(
        "rotated-hs",
        ROTATED_SECRET,
        "https://idp.example",
        "aria-product-api",
        "Viewer",
    );
    let principal = verifier
        .verify_bearer_token(&rotated_token, 1_800_000_002)
        .expect("rotated key should authenticate after refresh interval");
    assert_eq!(principal.operator_id, "viewer-a");

    std::fs::remove_dir_all(temp).ok();
}

#[test]
fn refreshing_jwks_verifier_fails_closed_when_trust_source_is_stale() {
    let temp = temp_test_dir("jwks-refresh-stale");
    let jwks_path = temp.join("jwks.json");
    std::fs::create_dir_all(&temp).expect("temp dir should be created");
    write_jwks(&jwks_path, "local-hs", TEST_SECRET_JWK_VALUE);
    let verifier = RefreshingJwtJwksProductIdentityVerifier::new(ProductJwtJwksFileVerifierConfig {
        max_stale_secs: 1,
        ..jwt_file_config(&jwks_path)
    })
    .expect("refreshing verifier should load initial JWKS");
    let token = signed_token_with_secret(
        "local-hs",
        TEST_SECRET,
        "https://idp.example",
        "aria-product-api",
        "Viewer",
    );
    verifier
        .verify_bearer_token(&token, 1_800_000_000)
        .expect("initial key should authenticate");

    std::fs::write(&jwks_path, br#"{"keys":[]}"#)
        .expect("invalid refreshed JWKS should be written");
    let err = verifier
        .verify_bearer_token(&token, 1_800_000_003)
        .expect_err("stale trust source should fail closed");
    assert!(matches!(err, UnderlayError::AuthenticationFailed(_)));

    std::fs::remove_dir_all(temp).ok();
}

fn jwt_verifier() -> JwtJwksProductIdentityVerifier {
    JwtJwksProductIdentityVerifier::new(ProductJwtJwksVerifierConfig {
        issuer: "https://idp.example".into(),
        audiences: vec!["aria-product-api".into()],
        allowed_algorithms: vec![ProductJwtAlgorithm::HS256],
        role_claim: "aria_role".into(),
        operator_id_claim: Some("preferred_username".into()),
        session_id_claim: Some("sid".into()),
        leeway_secs: 30,
        role_mappings: role_mappings(),
        jwks: serde_json::from_value(json!({
            "keys": [
                {
                    "kty": "oct",
                    "alg": "HS256",
                    "kid": "local-hs",
                    "k": TEST_SECRET_JWK_VALUE
                }
            ]
        }))
        .expect("jwks should parse"),
    })
    .expect("verifier config should validate")
}

fn signed_token(kid: &str, issuer: &str, audience: &str, role: &str) -> String {
    signed_token_with_secret(kid, TEST_SECRET, issuer, audience, role)
}

fn signed_token_with_secret(
    kid: &str,
    secret: &[u8],
    issuer: &str,
    audience: &str,
    role: &str,
) -> String {
    let mut header = Header::new(Algorithm::HS256);
    header.kid = Some(kid.into());
    encode(
        &header,
        &TestClaims {
            iss: issuer.into(),
            sub: "subject-viewer-a".into(),
            aud: audience.into(),
            exp: 4_102_444_800,
            preferred_username: "viewer-a".into(),
            aria_role: role.into(),
            sid: "session-viewer-a".into(),
        },
        &EncodingKey::from_secret(secret),
    )
    .expect("test token should encode")
}

fn jwt_file_config(path: &std::path::Path) -> ProductJwtJwksFileVerifierConfig {
    ProductJwtJwksFileVerifierConfig {
        issuer: "https://idp.example".into(),
        audiences: vec!["aria-product-api".into()],
        allowed_algorithms: vec![ProductJwtAlgorithm::HS256],
        role_claim: "aria_role".into(),
        operator_id_claim: Some("preferred_username".into()),
        session_id_claim: Some("sid".into()),
        leeway_secs: 30,
        role_mappings: role_mappings(),
        jwks_path: path.into(),
        refresh_interval_secs: 1,
        max_stale_secs: 60,
    }
}

fn write_jwks(path: &std::path::Path, kid: &str, secret_value: &str) {
    let jwks = json!({
        "keys": [
            {
                "kty": "oct",
                "alg": "HS256",
                "kid": kid,
                "k": secret_value
            }
        ]
    });
    std::fs::write(
        path,
        serde_json::to_vec_pretty(&jwks).expect("jwks should serialize"),
    )
    .expect("jwks should be written");
}

fn temp_test_dir(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "aria-underlay-product-jwt-{name}-{}",
        uuid::Uuid::new_v4()
    ))
}

fn role_mappings() -> BTreeMap<String, RbacRole> {
    BTreeMap::from([
        ("Viewer".into(), RbacRole::Viewer),
        ("Operator".into(), RbacRole::Operator),
        ("Admin".into(), RbacRole::Admin),
        ("Auditor".into(), RbacRole::Auditor),
    ])
}

#[derive(Debug, Serialize)]
struct TestClaims {
    iss: String,
    sub: String,
    aud: String,
    exp: u64,
    preferred_username: String,
    aria_role: String,
    sid: String,
}
