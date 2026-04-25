use std::time::Duration;

pub fn exponential_delay(initial: Duration, attempt: u32, max: Duration) -> Duration {
    let multiplier = 2_u32.saturating_pow(attempt);
    let delay = initial.saturating_mul(multiplier);
    delay.min(max)
}

