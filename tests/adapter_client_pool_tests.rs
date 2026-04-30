use aria_underlay::adapter_client::AdapterClientPool;
use aria_underlay::UnderlayError;

#[test]
fn adapter_client_pool_reuses_channel_for_same_endpoint() {
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

#[test]
fn adapter_client_pool_keeps_endpoints_separate() {
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

#[test]
fn adapter_client_pool_rejects_invalid_endpoint_without_caching() {
    let pool = AdapterClientPool::default();

    let err = pool
        .client("not a uri")
        .expect_err("invalid endpoint should fail before caching");

    assert!(matches!(err, UnderlayError::AdapterTransport(_)));
    assert_eq!(pool.cached_endpoint_count(), 0);
}

#[test]
fn adapter_client_pool_can_invalidate_endpoint() {
    let pool = AdapterClientPool::default();

    let _client = pool
        .client("http://127.0.0.1:50051")
        .expect("client should be created");
    assert!(pool.contains_endpoint("http://127.0.0.1:50051"));

    pool.invalidate("http://127.0.0.1:50051");

    assert_eq!(pool.cached_endpoint_count(), 0);
    assert!(!pool.contains_endpoint("http://127.0.0.1:50051"));
}
