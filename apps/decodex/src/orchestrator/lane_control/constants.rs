use std::time::Duration;

pub(crate) const DEFAULT_STEER_RESULT_WAIT_TIMEOUT: Duration = Duration::from_secs(10);

#[cfg(not(test))]
pub(super) const LANE_INTERRUPT_RESPONSE_WAIT: Duration = Duration::from_secs(3);
#[cfg(test)]
pub(super) const LANE_INTERRUPT_RESPONSE_WAIT: Duration = Duration::from_millis(20);
pub(super) const LANE_HARD_INTERRUPT_TERM_WAIT: Duration = Duration::from_secs(2);
