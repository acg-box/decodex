use std::time::{Duration, Instant};

use crate::state::ProtocolActivitySummary;

#[test]
fn remaining_idle_budget_resets_from_latest_activity() {
	let now = Instant::now();
	let timeout = Duration::from_secs(300);
	let last_activity_at = now.checked_sub(Duration::from_secs(12)).expect("instant math");
	let remaining =
		super::remaining_idle_budget(last_activity_at, now, timeout).expect("budget should remain");

	assert!(remaining <= timeout);
	assert!(remaining >= Duration::from_secs(287));
}

#[test]
fn remaining_idle_budget_expires_after_idle_timeout() {
	let now = Instant::now();
	let timeout = Duration::from_secs(300);
	let last_activity_at = now.checked_sub(Duration::from_secs(301)).expect("instant math");

	assert!(super::remaining_idle_budget(last_activity_at, now, timeout).is_none());
}

#[test]
fn protocol_activity_idle_timeout_extends_running_model_execution() {
	let protocol_activity = ProtocolActivitySummary {
		turn_status: Some(String::from("running")),
		waiting_reason: Some(String::from("model_execution")),
		..ProtocolActivitySummary::default()
	};

	assert_eq!(
		super::protocol_activity_idle_timeout(
			Some(&protocol_activity),
			super::RUN_LEASE_IDLE_TIMEOUT
		),
		super::MODEL_EXECUTION_IDLE_TIMEOUT
	);
}

#[test]
fn protocol_activity_idle_timeout_keeps_base_timeout_for_other_waits() {
	let protocol_activity = ProtocolActivitySummary {
		turn_status: Some(String::from("running")),
		waiting_reason: Some(String::from("tool_execution")),
		..ProtocolActivitySummary::default()
	};

	assert_eq!(
		super::protocol_activity_idle_timeout(
			Some(&protocol_activity),
			super::RUN_LEASE_IDLE_TIMEOUT
		),
		super::RUN_LEASE_IDLE_TIMEOUT
	);
}
