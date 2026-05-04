use std::collections::BTreeMap;
use std::sync::Arc;

use aria_underlay::api::operations::{
    ListOperationSummariesRequest, ListOperationSummariesResponse,
};
use aria_underlay::api::product_api::{
    ProductApiRequest, ProductApiRequestMetadata, ProductApiResponse, ProductOpsApi,
    ProductSessionExtractor,
};
use aria_underlay::api::product_http::{
    ProductHttpErrorResponse, ProductHttpMethod, ProductHttpRequest, ProductHttpRouter,
    OPERATION_SUMMARIES_QUERY_PATH,
};
use aria_underlay::api::product_identity::{
    BearerTokenProductSessionExtractor, ProductAuthenticatedPrincipal,
    StaticProductIdentityVerifier,
};
use aria_underlay::authz::RbacRole;
use aria_underlay::telemetry::{
    InMemoryOperationSummaryStore, InMemoryProductAuditStore, UnderlayEvent,
};
use aria_underlay::tx::recovery::RecoveryReport;
use aria_underlay::UnderlayError;

#[test]
fn bearer_token_session_lists_operation_summaries_without_mock_role_headers() {
    let summary_store = seeded_summary_store();
    let api = ProductOpsApi::new(
        Arc::new(bearer_extractor(
            StaticProductIdentityVerifier::new().with_token(
                "viewer-token",
                ProductAuthenticatedPrincipal::new("viewer-a", RbacRole::Viewer)
                    .with_issuer("internal-product-config")
                    .with_subject("subject-viewer-a")
                    .with_session_id("session-viewer-a"),
            ),
        )),
        summary_store,
        Arc::new(InMemoryProductAuditStore::default()),
    );

    let response = api
        .list_operation_summaries(ProductApiRequest {
            request_id: "req-bearer-list".into(),
            trace_id: Some("trace-bearer-list".into()),
            headers: authorization_headers("Bearer viewer-token"),
            body: ListOperationSummariesRequest {
                attention_required_only: true,
                limit: Some(10),
                ..Default::default()
            },
        })
        .expect("valid bearer token should create a product session");

    assert_eq!(response.request_id, "req-bearer-list");
    assert_eq!(response.trace_id, "trace-bearer-list");
    assert_eq!(response.operator_id, "viewer-a");
    assert_eq!(response.role, RbacRole::Viewer);
    assert_eq!(response.body.overview.matched_records, 1);
}

#[test]
fn bearer_token_session_rejects_missing_authorization_header() {
    let extractor = bearer_extractor(StaticProductIdentityVerifier::new());

    let err = extractor
        .extract(&ProductApiRequestMetadata {
            request_id: "req-missing-auth".into(),
            trace_id: None,
            headers: BTreeMap::new(),
        })
        .expect_err("missing authorization should fail closed");

    assert!(matches!(err, UnderlayError::AuthenticationFailed(_)));
}

#[test]
fn bearer_token_session_rejects_unknown_token() {
    let extractor = bearer_extractor(StaticProductIdentityVerifier::new().with_token(
        "known-token",
        ProductAuthenticatedPrincipal::new("viewer-a", RbacRole::Viewer),
    ));

    let err = extractor
        .extract(&ProductApiRequestMetadata {
            request_id: "req-unknown-token".into(),
            trace_id: None,
            headers: authorization_headers("Bearer unknown-token"),
        })
        .expect_err("unknown token should fail closed");

    assert!(matches!(err, UnderlayError::AuthenticationFailed(_)));
}

#[test]
fn bearer_token_session_rejects_expired_token() {
    let extractor = bearer_extractor(StaticProductIdentityVerifier::new().with_token(
        "expired-token",
        ProductAuthenticatedPrincipal::new("viewer-a", RbacRole::Viewer)
            .with_expires_at_unix_secs(1),
    ));

    let err = extractor
        .extract(&ProductApiRequestMetadata {
            request_id: "req-expired-token".into(),
            trace_id: None,
            headers: authorization_headers("Bearer expired-token"),
        })
        .expect_err("expired token should fail closed");

    assert!(matches!(err, UnderlayError::AuthenticationFailed(_)));
}

#[test]
fn product_http_maps_authentication_failure_to_401() {
    let router = product_http_router(StaticProductIdentityVerifier::new());

    let response = router.handle(ProductHttpRequest {
        method: ProductHttpMethod::Post,
        path: OPERATION_SUMMARIES_QUERY_PATH.into(),
        headers: request_headers("req-http-auth-failed", None),
        body: json_body(&ListOperationSummariesRequest::default()),
    });

    assert_eq!(response.status, 401);
    assert_eq!(
        response.headers.get("www-authenticate").map(String::as_str),
        Some("Bearer")
    );
    let body: ProductHttpErrorResponse = response_json(&response.body);
    assert_eq!(body.request_id.as_deref(), Some("req-http-auth-failed"));
    assert_eq!(body.error_code, "authentication_failed");
}

#[test]
fn product_http_accepts_bearer_session_without_mock_role_headers() {
    let router = product_http_router(StaticProductIdentityVerifier::new().with_token(
        "viewer-token",
        ProductAuthenticatedPrincipal::new("viewer-a", RbacRole::Viewer),
    ));
    let mut headers = request_headers("req-http-bearer", Some("trace-http-bearer"));
    headers.insert("authorization".into(), "Bearer viewer-token".into());

    let response = router.handle(ProductHttpRequest {
        method: ProductHttpMethod::Post,
        path: OPERATION_SUMMARIES_QUERY_PATH.into(),
        headers,
        body: json_body(&ListOperationSummariesRequest {
            attention_required_only: true,
            limit: Some(10),
            ..Default::default()
        }),
    });

    assert_eq!(response.status, 200);
    let body: ProductApiResponse<ListOperationSummariesResponse> = response_json(&response.body);
    assert_eq!(body.request_id, "req-http-bearer");
    assert_eq!(body.trace_id, "trace-http-bearer");
    assert_eq!(body.operator_id, "viewer-a");
    assert_eq!(body.role, RbacRole::Viewer);
    assert_eq!(body.body.overview.matched_records, 1);
}

fn bearer_extractor(
    verifier: StaticProductIdentityVerifier,
) -> BearerTokenProductSessionExtractor {
    BearerTokenProductSessionExtractor::new(Arc::new(verifier))
}

fn product_http_router(verifier: StaticProductIdentityVerifier) -> ProductHttpRouter {
    ProductHttpRouter::new(ProductOpsApi::new(
        Arc::new(bearer_extractor(verifier)),
        seeded_summary_store(),
        Arc::new(InMemoryProductAuditStore::default()),
    ))
}

fn seeded_summary_store() -> Arc<InMemoryOperationSummaryStore> {
    let summary_store = Arc::new(InMemoryOperationSummaryStore::default());
    summary_store
        .record_event(&UnderlayEvent::recovery_completed(
            "req-recovery",
            "trace-recovery",
            &RecoveryReport {
                recovered: 0,
                in_doubt: 1,
                pending: 0,
                tx_ids: vec!["tx-recovery".into()],
                decisions: Vec::new(),
            },
        ))
        .expect("summary event should be recorded");
    summary_store
}

fn authorization_headers(value: &str) -> BTreeMap<String, String> {
    let mut headers = BTreeMap::new();
    headers.insert("authorization".into(), value.into());
    headers
}

fn request_headers(request_id: &str, trace_id: Option<&str>) -> BTreeMap<String, String> {
    let mut headers = BTreeMap::new();
    headers.insert("x-aria-request-id".into(), request_id.into());
    if let Some(trace_id) = trace_id {
        headers.insert("x-aria-trace-id".into(), trace_id.into());
    }
    headers
}

fn json_body<T: serde::Serialize>(body: &T) -> Vec<u8> {
    serde_json::to_vec(body).expect("test body should serialize")
}

fn response_json<T: serde::de::DeserializeOwned>(body: &[u8]) -> T {
    serde_json::from_slice(body).expect("response should be valid JSON")
}
