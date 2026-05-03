use std::sync::Arc;

use aria_underlay::api::operations::ListOperationSummariesRequest;
use aria_underlay::api::product_api::ProductOpsApi;
use aria_underlay::api::product_http::OPERATION_SUMMARIES_QUERY_PATH;
use aria_underlay::api::product_http_server::{
    ProductHttpListenerConfig, ProductHttpServer,
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
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;

#[tokio::test]
async fn product_http_server_serves_router_over_loopback_tcp() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("loopback listener should bind");
    let addr = listener.local_addr().expect("listener should expose addr");
    let server = ProductHttpServer::new(product_server_router(), ProductHttpListenerConfig {
        bind_addr: addr,
        max_body_bytes: 16 * 1024,
    })
    .expect("server config should validate");
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let server_task = tokio::spawn(async move {
        server
            .serve_listener_until_shutdown(listener, async {
                let _ = shutdown_rx.await;
            })
            .await
    });

    let body = serde_json::to_vec(&ListOperationSummariesRequest {
        attention_required_only: true,
        limit: Some(10),
        ..Default::default()
    })
    .expect("request body should serialize");
    let response = send_raw_http(
        addr,
        format!(
            "POST {OPERATION_SUMMARIES_QUERY_PATH} HTTP/1.1\r\n\
             Host: {addr}\r\n\
             x-aria-request-id: req-product-listener\r\n\
             x-aria-trace-id: trace-product-listener\r\n\
             Authorization: Bearer viewer-token\r\n\
             Content-Type: application/json\r\n\
             Content-Length: {}\r\n\
             \r\n\
             {}",
            body.len(),
            String::from_utf8(body).expect("body should be utf-8")
        ),
    )
    .await;

    shutdown_tx
        .send(())
        .expect("shutdown receiver should still be running");
    server_task
        .await
        .expect("server task should join")
        .expect("server should shut down cleanly");

    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(response.contains("content-type: application/json\r\n"));
    assert!(response.contains("connection: close\r\n"));
    assert!(response.contains("\"request_id\":\"req-product-listener\""));
    assert!(response.contains("\"operator_id\":\"viewer-a\""));
    assert!(response.contains("\"matched_records\":1"));
}

#[test]
fn product_http_server_rejects_oversized_body_before_router_dispatch() {
    let server = ProductHttpServer::new(product_server_router(), ProductHttpListenerConfig {
        bind_addr: "127.0.0.1:0".parse().expect("addr should parse"),
        max_body_bytes: 8,
    })
    .expect("server config should validate");

    let response = server.handle_http_bytes(
        format!(
            "POST {OPERATION_SUMMARIES_QUERY_PATH} HTTP/1.1\r\n\
             Host: localhost\r\n\
             x-aria-request-id: req-too-large\r\n\
             Authorization: Bearer viewer-token\r\n\
             Content-Length: 9\r\n\
             \r\n\
             123456789"
        )
        .as_bytes(),
    );

    let response = String::from_utf8(response).expect("response should be utf-8");
    assert!(response.starts_with("HTTP/1.1 413 Payload Too Large\r\n"));
    assert!(response.contains("\"error_code\":\"payload_too_large\""));
    assert!(response.contains("\"request_id\":\"req-too-large\""));
}

async fn send_raw_http(addr: std::net::SocketAddr, request: String) -> String {
    let mut stream = TcpStream::connect(addr)
        .await
        .expect("client should connect to loopback listener");
    stream
        .write_all(request.as_bytes())
        .await
        .expect("client should write request");
    stream.shutdown().await.expect("client write shutdown should work");

    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .await
        .expect("client should read response");
    String::from_utf8(response).expect("response should be utf-8")
}

fn product_server_router() -> aria_underlay::api::product_http::ProductHttpRouter {
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
    let verifier = StaticProductIdentityVerifier::new().with_token(
        "viewer-token",
        ProductAuthenticatedPrincipal::new("viewer-a", RbacRole::Viewer),
    );
    aria_underlay::api::product_http::ProductHttpRouter::new(ProductOpsApi::new(
        Arc::new(BearerTokenProductSessionExtractor::new(Arc::new(verifier))),
        summary_store,
        Arc::new(InMemoryProductAuditStore::default()),
    ))
}
