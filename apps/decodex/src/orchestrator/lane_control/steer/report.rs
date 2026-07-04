use std::path::Path;

use crate::{
	orchestrator::{LaneSteerReport, OperatorRunStatus},
	run_control::{
		LaneControlSteerRequest, LaneControlSteerResponse, LaneControlSteerResponseStatus,
	},
	state::{
		RUN_CONTROL_ACTION_ACCEPTED, RUN_CONTROL_ACTION_COMPLETED, RUN_CONTROL_ACTION_FAILED,
		RunControlActionReceipt,
	},
};

pub(super) fn lane_steer_report_from_rejected_receipt(
	issue: &str,
	run: &OperatorRunStatus,
	receipt: &RunControlActionReceipt,
	expected_turn_id: &str,
	message_byte_count: usize,
	message_line_count: usize,
) -> LaneSteerReport {
	LaneSteerReport {
		project_id: receipt.project_id().to_owned(),
		issue_id: receipt.issue_id().to_owned(),
		issue_identifier: run.issue_identifier.clone().or_else(|| Some(issue.to_owned())),
		run_id: receipt.run_id().to_owned(),
		attempt_number: receipt.attempt_number(),
		thread_id: receipt.current_thread_id().map(str::to_owned),
		expected_turn_id: expected_turn_id.to_owned(),
		current_turn_id: receipt.current_turn_id().map(str::to_owned),
		response_turn_id: None,
		audit_record_id: receipt.audit_record_id(),
		request_id: String::new(),
		request_path: None,
		outcome: receipt.outcome().to_owned(),
		reason: receipt.reason().to_owned(),
		failure_class: lane_steer_failure_class_for_reason(receipt.reason()).map(str::to_owned),
		delivery_status: String::from("rejected"),
		message_byte_count,
		message_line_count,
	}
}

pub(super) fn lane_steer_report_from_response(
	issue: &str,
	run: &OperatorRunStatus,
	receipt: &RunControlActionReceipt,
	request: &LaneControlSteerRequest,
	request_path: &Path,
	response: LaneControlSteerResponse,
) -> LaneSteerReport {
	let outcome = match &response.status {
		LaneControlSteerResponseStatus::Delivered => RUN_CONTROL_ACTION_COMPLETED,
		LaneControlSteerResponseStatus::Failed | LaneControlSteerResponseStatus::Rejected =>
			RUN_CONTROL_ACTION_FAILED,
	};

	LaneSteerReport {
		project_id: receipt.project_id().to_owned(),
		issue_id: receipt.issue_id().to_owned(),
		issue_identifier: run.issue_identifier.clone().or_else(|| Some(issue.to_owned())),
		run_id: receipt.run_id().to_owned(),
		attempt_number: receipt.attempt_number(),
		thread_id: Some(response.thread_id.clone()),
		expected_turn_id: response.expected_turn_id.clone(),
		current_turn_id: response.current_turn_id.clone(),
		response_turn_id: response.response_turn_id.clone(),
		audit_record_id: receipt.audit_record_id(),
		request_id: response.request_id.clone(),
		request_path: Some(request_path.display().to_string()),
		outcome: outcome.to_owned(),
		reason: response.classification.clone(),
		failure_class: response.error_class.clone(),
		delivery_status: String::from("resolved"),
		message_byte_count: request.message_byte_count,
		message_line_count: request.message_line_count,
	}
}

pub(super) fn lane_steer_report_pending(
	issue: &str,
	run: &OperatorRunStatus,
	receipt: &RunControlActionReceipt,
	request: &LaneControlSteerRequest,
	request_path: &Path,
) -> LaneSteerReport {
	LaneSteerReport {
		project_id: receipt.project_id().to_owned(),
		issue_id: receipt.issue_id().to_owned(),
		issue_identifier: run.issue_identifier.clone().or_else(|| Some(issue.to_owned())),
		run_id: receipt.run_id().to_owned(),
		attempt_number: receipt.attempt_number(),
		thread_id: Some(request.thread_id.clone()),
		expected_turn_id: request.expected_turn_id.clone(),
		current_turn_id: run.turn_id.clone(),
		response_turn_id: None,
		audit_record_id: receipt.audit_record_id(),
		request_id: request.request_id.clone(),
		request_path: Some(request_path.display().to_string()),
		outcome: RUN_CONTROL_ACTION_ACCEPTED.to_owned(),
		reason: String::from("queued_wait_timeout"),
		failure_class: None,
		delivery_status: String::from("queued"),
		message_byte_count: request.message_byte_count,
		message_line_count: request.message_line_count,
	}
}

pub(super) fn lane_steer_message_line_count(message: &str) -> usize {
	message.lines().count().max(usize::from(!message.is_empty()))
}

fn lane_steer_failure_class_for_reason(reason: &str) -> Option<&'static str> {
	match reason {
		"turn_mismatch" | "stale_expected_turn_id" => Some("stale_expected_turn_id"),
		"active_turn_not_steerable" => Some("active_turn_not_steerable"),
		"app_server_turn_steer_timed_out" => Some("app_server_turn_steer_timed_out"),
		"app_server_turn_steer_unsupported" => Some("app_server_turn_steer_unsupported"),
		"app_server_turn_steer_failed" => Some("app_server_turn_steer_failed"),
		"run_lease_control_channel_resolved" | "queued_wait_timeout" => None,
		_ => Some("run_control_action_failed"),
	}
}
