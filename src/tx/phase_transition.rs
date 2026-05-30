use crate::error::UnderlayError;
use crate::tx::journal::TxPhase;

/// 验证 `from` → `to` 是否为合法的 phase 转换。
///
/// 非法转换返回 `UnderlayError::InvalidPhaseTransition`。
pub fn validate_transition(from: &TxPhase, to: &TxPhase) -> Result<(), UnderlayError> {
    if is_allowed(from, to) {
        Ok(())
    } else {
        Err(UnderlayError::InvalidPhaseTransition {
            from: format!("{from:?}"),
            to: format!("{to:?}"),
        })
    }
}

fn is_allowed(from: &TxPhase, to: &TxPhase) -> bool {
    if from == to {
        return true;
    }

    use TxPhase::*;
    matches!(
        (from, to),
        // Happy path: forward progression
        (Started, Preparing)
            | (Preparing, Prepared)
            | (Prepared, Committing)
            | (Committing, Verifying)
            | (Verifying, FinalConfirming)
            | (FinalConfirming, Committed)
            // Happy path -> terminal failure
            | (Preparing | Prepared | Committing | Verifying | FinalConfirming, Failed)
            // Happy path -> rollback
            | (Preparing | Prepared | Committing | Verifying | FinalConfirming, RollingBack)
            // Recovery entry: any stuck phase can enter Recovering
            | (
                Started
                    | Preparing
                    | Prepared
                    | Committing
                    | Verifying
                    | FinalConfirming
                    | Recovering,
                Recovering
            )
            // Recovery outcome
            | (Recovering, Committed | RolledBack | InDoubt)
            // Rollback outcome
            | (RollingBack, RolledBack | InDoubt)
            // Committed -> InDoubt: shadow state write failure after adapter commit
            | (Committed, InDoubt)
            // Force resolve: only from InDoubt
            | (InDoubt, ForceResolved)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use TxPhase::*;

    // --- Valid transitions ---

    #[test]
    fn happy_path_forward() {
        let transitions = [
            (Started, Preparing),
            (Preparing, Prepared),
            (Prepared, Committing),
            (Committing, Verifying),
            (Verifying, FinalConfirming),
            (FinalConfirming, Committed),
        ];
        for (from, to) in transitions {
            assert!(
                validate_transition(&from, &to).is_ok(),
                "{:?} -> {:?} should be valid",
                from,
                to
            );
        }
    }

    #[test]
    fn happy_path_to_failed() {
        let phases = [Preparing, Prepared, Committing, Verifying, FinalConfirming];
        for from in phases {
            assert!(
                validate_transition(&from, &Failed).is_ok(),
                "{:?} -> Failed should be valid",
                from
            );
        }
    }

    #[test]
    fn happy_path_to_rolling_back() {
        let phases = [Preparing, Prepared, Committing, Verifying, FinalConfirming];
        for from in phases {
            assert!(
                validate_transition(&from, &RollingBack).is_ok(),
                "{:?} -> RollingBack should be valid",
                from
            );
        }
    }

    #[test]
    fn recovery_entry() {
        let phases = [
            Started,
            Preparing,
            Prepared,
            Committing,
            Verifying,
            FinalConfirming,
            Recovering,
        ];
        for from in phases {
            assert!(
                validate_transition(&from, &Recovering).is_ok(),
                "{:?} -> Recovering should be valid",
                from
            );
        }
    }

    #[test]
    fn recovery_outcome() {
        let outcomes = [Committed, RolledBack, InDoubt];
        for to in outcomes {
            assert!(
                validate_transition(&Recovering, &to).is_ok(),
                "Recovering -> {:?} should be valid",
                to
            );
        }
    }

    #[test]
    fn rollback_outcome() {
        let outcomes = [RolledBack, InDoubt];
        for to in outcomes {
            assert!(
                validate_transition(&RollingBack, &to).is_ok(),
                "RollingBack -> {:?} should be valid",
                to
            );
        }
    }

    #[test]
    fn committed_to_in_doubt() {
        assert!(validate_transition(&Committed, &InDoubt).is_ok());
    }

    #[test]
    fn in_doubt_to_force_resolved() {
        assert!(validate_transition(&InDoubt, &ForceResolved).is_ok());
    }

    #[test]
    fn self_transitions_are_allowed() {
        let all_phases = [
            Started,
            Preparing,
            Prepared,
            Committing,
            Verifying,
            FinalConfirming,
            Recovering,
            Committed,
            RollingBack,
            RolledBack,
            Failed,
            InDoubt,
            ForceResolved,
        ];
        for phase in all_phases {
            assert!(
                validate_transition(&phase, &phase).is_ok(),
                "{:?} -> {:?} (self) should be valid",
                phase,
                phase
            );
        }
    }

    // --- Invalid transitions ---

    #[test]
    fn terminal_states_have_no_outgoing() {
        let terminals = [RolledBack, Failed, ForceResolved];
        let all_phases = [
            Started,
            Preparing,
            Prepared,
            Committing,
            Verifying,
            FinalConfirming,
            Recovering,
            Committed,
            RollingBack,
            InDoubt,
        ];
        for terminal in &terminals {
            for target in &all_phases {
                if terminal == target {
                    continue; // self-transition is allowed
                }
                assert!(
                    validate_transition(terminal, target).is_err(),
                    "{:?} -> {:?} should be invalid (terminal source)",
                    terminal,
                    target
                );
            }
        }
    }

    #[test]
    fn committed_only_to_in_doubt() {
        let invalid_targets = [
            Started,
            Preparing,
            Prepared,
            Committing,
            Verifying,
            FinalConfirming,
            Recovering,
            RollingBack,
            RolledBack,
            Failed,
            ForceResolved,
        ];
        for to in invalid_targets {
            assert!(
                validate_transition(&Committed, &to).is_err(),
                "Committed -> {:?} should be invalid",
                to
            );
        }
    }

    #[test]
    fn in_doubt_only_to_force_resolved() {
        let invalid_targets = [
            Started,
            Preparing,
            Prepared,
            Committing,
            Verifying,
            FinalConfirming,
            Recovering,
            Committed,
            RollingBack,
            RolledBack,
            Failed,
        ];
        for to in invalid_targets {
            assert!(
                validate_transition(&InDoubt, &to).is_err(),
                "InDoubt -> {:?} should be invalid",
                to
            );
        }
    }

    #[test]
    fn started_only_to_preparing_or_recovering() {
        let invalid_targets = [
            Prepared,
            Committing,
            Verifying,
            FinalConfirming,
            Committed,
            RollingBack,
            RolledBack,
            Failed,
            InDoubt,
            ForceResolved,
        ];
        for to in invalid_targets {
            assert!(
                validate_transition(&Started, &to).is_err(),
                "Started -> {:?} should be invalid",
                to
            );
        }
        assert!(validate_transition(&Started, &Preparing).is_ok());
        assert!(validate_transition(&Started, &Recovering).is_ok());
    }

    #[test]
    fn skipping_phases_is_invalid() {
        assert!(validate_transition(&Started, &Prepared).is_err());
        assert!(validate_transition(&Started, &Committed).is_err());
        assert!(validate_transition(&Preparing, &Committing).is_err());
        assert!(validate_transition(&Prepared, &Verifying).is_err());
        assert!(validate_transition(&Committing, &FinalConfirming).is_err());
    }

    #[test]
    fn backward_transitions_are_invalid() {
        assert!(validate_transition(&Prepared, &Preparing).is_err());
        assert!(validate_transition(&Committing, &Prepared).is_err());
        assert!(validate_transition(&Verifying, &Committing).is_err());
        assert!(validate_transition(&FinalConfirming, &Verifying).is_err());
    }

    #[test]
    fn rolling_back_to_recovering_is_invalid() {
        assert!(validate_transition(&RollingBack, &Recovering).is_err());
    }

    #[test]
    fn error_message_contains_phases() {
        let err = validate_transition(&Started, &Committed).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("Started"), "error should mention source phase");
        assert!(msg.contains("Committed"), "error should mention target phase");
    }
}
