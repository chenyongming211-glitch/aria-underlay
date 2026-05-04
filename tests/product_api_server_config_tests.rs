use aria_underlay::api::product_server_config::{
    ProductApiDeploymentMode, ProductApiServerConfig,
};
use aria_underlay::api::product_identity::ProductAuthenticatedPrincipal;
use aria_underlay::authz::RbacRole;
use aria_underlay::UnderlayError;

#[test]
fn product_api_server_config_rejects_wildcard_bind_in_local_mode() {
    let config = ProductApiServerConfig {
        deployment_mode: ProductApiDeploymentMode::Local,
        bind_addr: "0.0.0.0:8088".parse().expect("addr should parse"),
        ..local_static_config()
    };

    let err = config
        .validate()
        .expect_err("local mode must bind only to loopback");

    assert!(matches!(err, UnderlayError::InvalidIntent(_)));
}

#[test]
fn product_api_production_sample_parses_and_validates_packaging_boundary() {
    let config = ProductApiServerConfig::from_path("docs/examples/product-api.production.json")
        .expect("production sample should parse");

    config
        .validate()
        .expect("production sample should satisfy packaging guardrails");
    assert_eq!(
        config.deployment_mode,
        ProductApiDeploymentMode::ProductionIngress
    );
    assert!(!config.static_tokens.is_empty());
}

#[test]
fn product_api_server_config_rejects_jwt_jwks_fields() {
    let json = r#"{
      "deployment_mode": "production_ingress",
      "bind_addr": "127.0.0.1:8088",
      "max_body_bytes": 1048576,
      "operation_summary_path": "/var/lib/aria-underlay/ops/operation-summaries.jsonl",
      "product_audit_path": "/var/lib/aria-underlay/ops/product-audit.jsonl",
      "static_tokens": {},
      "jwt_jwks": {"issuer": "https://internal.example.invalid"}
    }"#;

    serde_json::from_str::<ProductApiServerConfig>(json)
        .expect_err("product API config must not accept JWT/JWKS fields");
}

fn local_static_config() -> ProductApiServerConfig {
    ProductApiServerConfig {
        deployment_mode: ProductApiDeploymentMode::Local,
        bind_addr: "127.0.0.1:8088".parse().expect("addr should parse"),
        max_body_bytes: 1024 * 1024,
        operation_summary_path: "var/aria-underlay/ops/operation-summaries.jsonl".into(),
        product_audit_path: "var/aria-underlay/ops/product-audit.jsonl".into(),
        static_tokens: std::collections::BTreeMap::from([(
            "local-viewer-token".into(),
            ProductAuthenticatedPrincipal::new("local-viewer", RbacRole::Viewer),
        )]),
    }
}
