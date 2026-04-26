#[test]
fn proto_contract_module_is_available() {
    let _ = aria_underlay::proto::adapter::Vendor::Unknown;
}

#[test]
fn recover_request_carries_strategy_and_action() {
    let request = aria_underlay::proto::adapter::RecoverRequest {
        context: None,
        device: None,
        strategy: aria_underlay::proto::adapter::TransactionStrategy::ConfirmedCommit as i32,
        action: aria_underlay::proto::adapter::RecoveryAction::AdapterRecover as i32,
    };

    assert_eq!(
        request.strategy,
        aria_underlay::proto::adapter::TransactionStrategy::ConfirmedCommit as i32
    );
    assert_eq!(
        request.action,
        aria_underlay::proto::adapter::RecoveryAction::AdapterRecover as i32
    );
}
