use crate::intent::SwitchPairIntent;
use crate::{UnderlayError, UnderlayResult};

pub fn validate_switch_pair_intent(intent: &SwitchPairIntent) -> UnderlayResult<()> {
    if intent.switches.is_empty() {
        return Err(UnderlayError::InvalidIntent("intent has no switches".into()));
    }
    Ok(())
}

