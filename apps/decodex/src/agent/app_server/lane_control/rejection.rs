use crate::agent::app_server::{
	AppServerRunRequest, LaneControlInterruptRequest, LaneControlSteerRequest,
};

pub(in crate::agent::app_server::lane_control) fn lane_interrupt_request_rejection(
	run_request: &AppServerRunRequest<'_>,
	request: &LaneControlInterruptRequest,
	target_thread_id: &str,
	target_turn_id: &str,
) -> Option<(&'static str, String)> {
	if request.project_id != run_request.project_id {
		return Some((
			"project_mismatch",
			format!(
				"Control request targeted project `{}`, but this run belongs to `{}`.",
				request.project_id, run_request.project_id
			),
		));
	}
	if request.issue_id != run_request.issue_id {
		return Some((
			"issue_mismatch",
			format!(
				"Control request targeted issue `{}`, but this run belongs to `{}`.",
				request.issue_id, run_request.issue_id
			),
		));
	}
	if request.run_id != run_request.run_id {
		return Some((
			"run_mismatch",
			format!(
				"Control request targeted run `{}`, but this run is `{}`.",
				request.run_id, run_request.run_id
			),
		));
	}
	if request.attempt_number != run_request.attempt_number {
		return Some((
			"attempt_mismatch",
			format!(
				"Control request targeted attempt `{}`, but this run is attempt `{}`.",
				request.attempt_number, run_request.attempt_number
			),
		));
	}
	if request.thread_id != target_thread_id {
		return Some((
			"thread_mismatch",
			format!(
				"Control request targeted thread `{}`, but the active thread is `{target_thread_id}`.",
				request.thread_id
			),
		));
	}
	if request.turn_id != target_turn_id {
		return Some((
			"turn_mismatch",
			format!(
				"Control request targeted turn `{}`, but the active turn is `{target_turn_id}`.",
				request.turn_id
			),
		));
	}

	None
}

pub(in crate::agent::app_server::lane_control) fn lane_steer_request_rejection(
	run_request: &AppServerRunRequest<'_>,
	request: &LaneControlSteerRequest,
	target_thread_id: &str,
	target_turn_id: &str,
) -> Option<(&'static str, String)> {
	if request.project_id != run_request.project_id {
		return Some((
			"project_mismatch",
			format!(
				"Control request targeted project `{}`, but this run belongs to `{}`.",
				request.project_id, run_request.project_id
			),
		));
	}
	if request.issue_id != run_request.issue_id {
		return Some((
			"issue_mismatch",
			format!(
				"Control request targeted issue `{}`, but this run belongs to `{}`.",
				request.issue_id, run_request.issue_id
			),
		));
	}
	if request.run_id != run_request.run_id {
		return Some((
			"run_mismatch",
			format!(
				"Control request targeted run `{}`, but this run is `{}`.",
				request.run_id, run_request.run_id
			),
		));
	}
	if request.attempt_number != run_request.attempt_number {
		return Some((
			"attempt_mismatch",
			format!(
				"Control request targeted attempt `{}`, but this run is attempt `{}`.",
				request.attempt_number, run_request.attempt_number
			),
		));
	}
	if request.thread_id != target_thread_id {
		return Some((
			"thread_mismatch",
			format!(
				"Control request targeted thread `{}`, but the active thread is `{target_thread_id}`.",
				request.thread_id
			),
		));
	}
	if request.expected_turn_id != target_turn_id {
		return Some((
			"stale_expected_turn_id",
			format!(
				"Control request expected turn `{}`, but the active turn is `{target_turn_id}`.",
				request.expected_turn_id
			),
		));
	}

	None
}
