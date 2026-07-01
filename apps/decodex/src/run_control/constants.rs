use std::time::Duration;

pub(super) const RUN_CONTROL_DIR: &str = ".decodex-run-control";
pub(super) const REQUEST_SUFFIX: &str = ".request.json";
pub(super) const RESPONSE_SUFFIX: &str = ".response.json";
pub(super) const STEER_REQUEST_SUFFIX: &str = ".steer-request.json";
pub(super) const STEER_RESPONSE_SUFFIX: &str = ".steer-response.json";
pub(super) const SCHEMA_INTERRUPT_REQUEST: &str = "decodex/run-control/interrupt-request/1";
pub(super) const SCHEMA_INTERRUPT_RESPONSE: &str = "decodex/run-control/interrupt-response/1";
pub(super) const SCHEMA_STEER_REQUEST: &str = "decodex/run-control/steer-request/1";
pub(super) const SCHEMA_STEER_RESPONSE: &str = "decodex/run-control/steer-response/1";
pub(super) const POLL_INTERVAL: Duration = Duration::from_millis(100);
