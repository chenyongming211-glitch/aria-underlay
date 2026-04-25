use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransactionStrategy {
    ConfirmedCommit,
    CandidateCommit,
    RunningRollbackOnError,
    BestEffortCli,
    Unsupported,
}

impl TransactionStrategy {
    pub fn is_supported(self) -> bool {
        !matches!(self, Self::Unsupported)
    }

    pub fn is_degraded(self) -> bool {
        matches!(self, Self::RunningRollbackOnError | Self::BestEffortCli)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransactionMode {
    StrictConfirmedCommit,
    AllowCandidateCommit,
    AllowRunningFallback,
    AllowBestEffortCli,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityFlags {
    pub supports_candidate: bool,
    pub supports_validate: bool,
    pub supports_confirmed_commit: bool,
    pub supports_rollback_on_error: bool,
    pub supports_writable_running: bool,
    pub supports_cli_fallback: bool,
}

pub fn choose_strategy(flags: CapabilityFlags, mode: TransactionMode) -> TransactionStrategy {
    if flags.supports_candidate && flags.supports_validate && flags.supports_confirmed_commit {
        return TransactionStrategy::ConfirmedCommit;
    }

    if flags.supports_candidate
        && flags.supports_validate
        && matches!(
            mode,
            TransactionMode::AllowCandidateCommit
                | TransactionMode::AllowRunningFallback
                | TransactionMode::AllowBestEffortCli
        )
    {
        return TransactionStrategy::CandidateCommit;
    }

    if flags.supports_writable_running
        && flags.supports_rollback_on_error
        && matches!(
            mode,
            TransactionMode::AllowRunningFallback | TransactionMode::AllowBestEffortCli
        )
    {
        return TransactionStrategy::RunningRollbackOnError;
    }

    if flags.supports_cli_fallback && matches!(mode, TransactionMode::AllowBestEffortCli) {
        return TransactionStrategy::BestEffortCli;
    }

    TransactionStrategy::Unsupported
}
