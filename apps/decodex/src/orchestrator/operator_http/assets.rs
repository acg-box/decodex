//! Operator HTTP constants.

use std::time::Duration;

pub(crate) const DASHBOARD_MAX_WEBSOCKET_CLIENTS: usize = 64;

pub(in crate::orchestrator::operator_http) const OPERATOR_HTTP_READ_TIMEOUT: Duration =
	Duration::from_millis(250);
pub(in crate::orchestrator::operator_http) const RUN_ACTIVITY_FINGERPRINT_VOLATILE_FIELDS:
	&[&str] =
	&["idle_for_seconds", "protocol_idle_for_seconds", "current_elapsed_seconds", "wall_seconds"];
