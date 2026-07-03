use std::time::Duration;

pub(in crate::orchestrator::operator_http) const OPERATOR_HTTP_READ_TIMEOUT: Duration =
	Duration::from_millis(250);
pub(in crate::orchestrator::operator_http) const DASHBOARD_RUN_ACTIVITY_FINGERPRINT_VOLATILE_FIELDS: &[&str] =
	&["idle_for_seconds", "protocol_idle_for_seconds", "current_elapsed_seconds", "wall_seconds"];
