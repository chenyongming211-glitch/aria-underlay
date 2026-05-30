use aria_underlay::adapter_client::{AdapterClientPool, TlsConfig};
use aria_underlay::UnderlayError;

#[tokio::test]
async fn adapter_client_pool_reuses_channel_for_same_endpoint() {
    let pool = AdapterClientPool::default();

    let _first = pool
        .client("http://127.0.0.1:50051")
        .expect("first client should be created");
    let _second = pool
        .client("http://127.0.0.1:50051")
        .expect("second client should reuse cached endpoint");

    assert_eq!(pool.cached_endpoint_count(), 1);
    assert!(pool.contains_endpoint("http://127.0.0.1:50051"));
}

#[tokio::test]
async fn adapter_client_pool_keeps_endpoints_separate() {
    let pool = AdapterClientPool::default();

    let _first = pool
        .client("http://127.0.0.1:50051")
        .expect("first client should be created");
    let _second = pool
        .client("http://127.0.0.1:50052")
        .expect("second endpoint should be cached separately");

    assert_eq!(pool.cached_endpoint_count(), 2);
    assert!(pool.contains_endpoint("http://127.0.0.1:50051"));
    assert!(pool.contains_endpoint("http://127.0.0.1:50052"));
}

#[tokio::test]
async fn adapter_client_pool_rejects_invalid_endpoint_without_caching() {
    let pool = AdapterClientPool::default();

    let err = pool
        .client("not a uri")
        .expect_err("invalid endpoint should fail before caching");

    assert!(matches!(err, UnderlayError::AdapterTransport(_)));
    assert_eq!(pool.cached_endpoint_count(), 0);
}

#[tokio::test]
async fn adapter_client_pool_can_invalidate_endpoint() {
    let pool = AdapterClientPool::default();

    let _client = pool
        .client("http://127.0.0.1:50051")
        .expect("client should be created");
    assert!(pool.contains_endpoint("http://127.0.0.1:50051"));

    pool.invalidate("http://127.0.0.1:50051");

    assert_eq!(pool.cached_endpoint_count(), 0);
    assert!(!pool.contains_endpoint("http://127.0.0.1:50051"));
}

#[tokio::test]
async fn adapter_client_pool_default_has_no_tls() {
    let pool = AdapterClientPool::default();
    assert!(!pool.has_tls());
}

#[tokio::test]
async fn adapter_client_pool_without_tls_rejects_https_endpoint() {
    let pool = AdapterClientPool::default();

    let err = pool
        .client("https://127.0.0.1:50051")
        .expect_err("https endpoint requires tls config");

    assert!(matches!(
        err,
        UnderlayError::AdapterTransport(message) if message.contains("TLS config")
    ));
    assert_eq!(pool.cached_endpoint_count(), 0);
}

#[tokio::test]
async fn adapter_client_pool_with_tls_reports_tls_enabled() {
    let pool = AdapterClientPool::with_tls(TlsConfig {
        client_cert_pem: TEST_CERT.to_string(),
        client_key_pem: TEST_KEY.to_string(),
        ca_cert_pem: None,
    });
    assert!(pool.has_tls());
}

#[tokio::test]
async fn adapter_client_pool_with_tls_creates_client_for_https_endpoint() {
    let pool = AdapterClientPool::with_tls(TlsConfig {
        client_cert_pem: TEST_CERT.to_string(),
        client_key_pem: TEST_KEY.to_string(),
        ca_cert_pem: Some(TEST_CA_CERT.to_string()),
    });

    let _client = pool
        .client("https://127.0.0.1:50051")
        .expect("tls pool should create client for https endpoint");

    assert_eq!(pool.cached_endpoint_count(), 1);
    assert!(pool.contains_endpoint("https://127.0.0.1:50051"));
}

#[tokio::test]
async fn adapter_client_pool_with_tls_still_supports_http_endpoints() {
    let pool = AdapterClientPool::with_tls(TlsConfig {
        client_cert_pem: TEST_CERT.to_string(),
        client_key_pem: TEST_KEY.to_string(),
        ca_cert_pem: None,
    });

    let _client = pool
        .client("http://127.0.0.1:50051")
        .expect("tls pool should still allow http endpoints");

    assert_eq!(pool.cached_endpoint_count(), 1);
}

#[tokio::test]
async fn adapter_client_pool_with_tls_reuses_cached_secure_channel() {
    let pool = AdapterClientPool::with_tls(TlsConfig {
        client_cert_pem: TEST_CERT.to_string(),
        client_key_pem: TEST_KEY.to_string(),
        ca_cert_pem: None,
    });

    let _first = pool
        .client("https://127.0.0.1:50051")
        .expect("first secure client");
    let _second = pool
        .client("https://127.0.0.1:50051")
        .expect("second secure client reuses cache");

    assert_eq!(pool.cached_endpoint_count(), 1);
}

#[tokio::test]
async fn adapter_client_pool_with_tls_can_invalidate_secure_endpoint() {
    let pool = AdapterClientPool::with_tls(TlsConfig {
        client_cert_pem: TEST_CERT.to_string(),
        client_key_pem: TEST_KEY.to_string(),
        ca_cert_pem: None,
    });

    let _client = pool
        .client("https://127.0.0.1:50051")
        .expect("secure client");
    assert!(pool.contains_endpoint("https://127.0.0.1:50051"));

    pool.invalidate("https://127.0.0.1:50051");

    assert_eq!(pool.cached_endpoint_count(), 0);
    assert!(!pool.contains_endpoint("https://127.0.0.1:50051"));
}

const TEST_CERT: &str = include_str!("../adapter-python/tests/fixtures/tls/client.crt");
const TEST_KEY: &str = include_str!("../adapter-python/tests/fixtures/tls/client.key");
const TEST_CA_CERT: &str = include_str!("../adapter-python/tests/fixtures/tls/ca.crt");
