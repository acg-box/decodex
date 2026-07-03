use crate::orchestrator::tests::operator::status::running_lanes::{
	self, ChildAgentActivitySummary, OperatorStatusSnapshot, StateStore, orchestrator, state,
};

pub(in crate::orchestrator::tests::operator::status::running_lanes) fn assert_terminal_pending_status_projection(
	snapshot: &OperatorStatusSnapshot,
) {
	let project = snapshot.projects.first().expect("project summary should exist");
	let run = snapshot
		.recent_runs
		.iter()
		.find(|run| run.run_id == "run-1")
		.expect("terminal-pending run should remain inspectable in recent runs");

	assert!(
		snapshot.current_lanes.is_empty(),
		"terminal-finalized runs must not keep presenting as active execution"
	);
	assert_eq!(project.current_lane_count, 0);
	assert_eq!(project.running_lane_count, 0);
	assert_eq!(run.status, "review_handoff_pending");
	assert_eq!(run.attempt_status, "running");
	assert_eq!(run.phase, "terminal_pending");
	assert_eq!(run.wait_reason.as_deref(), Some("review_handoff_writeback"));
	assert_eq!(run.current_operation, state::RUN_OPERATION_REVIEW_WRITEBACK);
	assert!(!run.run_lease);
	assert_eq!(run.queue_lease_state, "not_held");
	assert_eq!(run.execution_liveness, "not_running");
	assert!(!run.suspected_stall);
	assert_eq!(run.last_event_type.as_deref(), Some("skills/changed"));
	assert_eq!(
		run.loop_status.as_ref().map(|status| status.summary.as_str()),
		Some("terminal lifecycle: review_handoff_pending")
	);
}

pub(in crate::orchestrator::tests::operator::status::running_lanes) fn assert_terminal_pending_lane_inspect(
	state_store: &StateStore,
) {
	let response = String::from_utf8(orchestrator::build_operator_lane_inspect_http_response(
		state_store,
		format!(
			"GET {}?projectId=pubfi&issue=PUB-101&runId=run-1 HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
			orchestrator::OPERATOR_LANE_INSPECT_ENDPOINT_PATH
		)
		.as_bytes(),
	))
	.expect("lane inspect response should be utf-8");
	let body = operator_status_response_body(&response, "lane inspect");

	assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
	assert!(body.contains(r#""matchedRunCount":1"#));
	assert!(body.contains(r#""status":"review_handoff_pending""#));
	assert!(body.contains(r#""phase":"terminal_pending""#));
	assert!(body.contains(r#""waitReason":"review_handoff_writeback""#));
	assert!(body.contains(r#""currentOperation":"review_writeback""#));
	assert!(body.contains(r#""runLease":false"#));
	assert!(body.contains(r#""executionLiveness":"not_running""#));
	assert!(body.contains(r#""softInterruptAvailable":false"#));
	assert!(body.contains(r#""hardInterruptAvailable":false"#));
}

pub(in crate::orchestrator::tests::operator::status::running_lanes) fn assert_terminal_pending_interrupt_rejects_force(
	state_store: &StateStore,
) {
	let body = br#"{"projectId":"pubfi","issue":"PUB-101","runId":"run-1","force":true}"#;
	let response = String::from_utf8(orchestrator::build_operator_lane_interrupt_http_response(
		state_store,
		format!(
			"POST {} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
			orchestrator::OPERATOR_LANE_INTERRUPT_ENDPOINT_PATH,
			body.len(),
			String::from_utf8_lossy(body)
		)
		.as_bytes(),
	))
	.expect("lane interrupt response should be utf-8");
	let body = operator_status_response_body(&response, "lane interrupt");

	assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
	assert!(body.contains(r#""classification":"soft_interrupt_unavailable""#));
	assert!(body.contains(r#""errorClass":"lane_not_active""#));
	assert!(body.contains(r#""hardInterrupt":null"#));
}

pub(super) fn sample_lifecycle_activity(
	wall_seconds: i64,
	event_count: i64,
	tool_call_count: i64,
	input_tokens: i64,
	output_tokens: i64,
) -> ChildAgentActivitySummary {
	ChildAgentActivitySummary {
		buckets: vec![state::ChildAgentActivityBucket {
			name: String::from("Model"),
			wall_seconds,
			event_count,
			tool_call_count,
			input_tokens,
			output_tokens,
			..state::ChildAgentActivityBucket::default()
		}],
		wall_seconds,
		event_count,
		tool_call_count,
		input_tokens_cumulative: input_tokens,
		output_tokens_cumulative: output_tokens,
		..ChildAgentActivitySummary::default()
	}
}

fn operator_status_response_body<'a>(response: &'a str, context: &str) -> &'a str {
	response
		.split_once("\r\n\r\n")
		.map(|(_, body)| body)
		.unwrap_or_else(|| running_lanes::panic!("{context} response should include body"))
}
