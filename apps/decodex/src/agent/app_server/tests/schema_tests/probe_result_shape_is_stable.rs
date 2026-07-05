use crate::agent::app_server::{AppServerCapabilityPreflightReport, AppServerRunResult};

#[test]
fn probe_result_shape_is_stable() {
	let result = AppServerRunResult {
		user_agent: String::from("ua"),
		capability_preflight: AppServerCapabilityPreflightReport::new(),
		thread_id: String::from("thread"),
		turn_id: String::from("turn"),
		turn_count: 1,
		event_count: 3,
		final_output: String::from("PROBE_OK"),
		continuation_pending: false,
		phase_goal_status: None,
	};

	assert_eq!(result.final_output, "PROBE_OK");
	assert_eq!(result.turn_count, 1);
}
