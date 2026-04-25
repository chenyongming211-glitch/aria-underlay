use aria_underlay::tx::{choose_strategy, CapabilityFlags, TransactionMode, TransactionStrategy};

#[test]
fn confirmed_commit_strategy_wins_when_supported() {
    let strategy = choose_strategy(
        CapabilityFlags {
            supports_candidate: true,
            supports_validate: true,
            supports_confirmed_commit: true,
            supports_rollback_on_error: false,
            supports_writable_running: false,
            supports_cli_fallback: false,
        },
        TransactionMode::StrictConfirmedCommit,
    );

    assert_eq!(strategy, TransactionStrategy::ConfirmedCommit2Pc);
}

