use serde::Serialize;

use crate::{
	orchestrator::{self, OperatorRunStatus},
	run_control::{LaneControlInterruptResponse, LaneControlResponseStatus},
	state::RunControlActionReceipt,
};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LaneInspectReport {
	pub(super) project_id: String,
	pub(super) issue: String,
	pub(super) run_id: Option<String>,
	pub(super) matched_run_count: usize,
	pub(super) runs: Vec<LaneRunInspect>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LaneInterruptReport {
	pub(super) project_id: String,
	pub(super) issue: String,
	pub(super) issue_id: String,
	pub(super) issue_identifier: Option<String>,
	pub(super) run_id: String,
	pub(super) attempt_number: i64,
	pub(super) force: bool,
	pub(super) classification: String,
	pub(super) soft_interrupt: LaneSoftInterruptReport,
	pub(super) hard_interrupt: Option<LaneHardInterruptReport>,
	pub(super) next_action: String,
}
impl LaneInterruptReport {
	pub(in crate::orchestrator) fn http_status_line(&self) -> &'static str {
		if self.soft_interrupt.status == "pending" && self.hard_interrupt.is_none() {
			"202 Accepted"
		} else {
			"200 OK"
		}
	}
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct LaneRunInspect {
	pub(super) project_id: String,
	pub(super) issue_id: String,
	pub(super) issue_identifier: Option<String>,
	pub(super) run_id: String,
	pub(super) attempt_number: i64,
	pub(super) status: String,
	pub(super) attempt_status: String,
	pub(super) phase: String,
	pub(super) wait_reason: Option<String>,
	pub(super) current_operation: String,
	pub(super) run_lease: bool,
	pub(super) execution_liveness: String,
	pub(super) ownership_state: String,
	pub(super) liveness_state: String,
	pub(super) policy_state: String,
	pub(super) terminalization_state: String,
	pub(super) lane_control_next_action: String,
	pub(super) lane_control_conditions: Vec<String>,
	pub(super) thread_id: Option<String>,
	pub(super) turn_id: Option<String>,
	pub(super) thread_status: Option<String>,
	pub(super) process_id: Option<u32>,
	pub(super) process_alive: Option<bool>,
	pub(super) process_liveness_reason: Option<String>,
	pub(super) last_event_type: Option<String>,
	pub(super) last_event_at: Option<String>,
	pub(super) event_count: i64,
	pub(super) worktree_path: Option<String>,
	pub(super) soft_interrupt_available: bool,
	pub(super) hard_interrupt_available: bool,
	pub(super) hard_interrupt_requires_force: bool,
}
impl LaneRunInspect {
	pub(super) fn from_operator_run(run: &OperatorRunStatus) -> Self {
		Self {
			project_id: run.project_id.clone(),
			issue_id: run.issue_id.clone(),
			issue_identifier: run.issue_identifier.clone(),
			run_id: run.run_id.clone(),
			attempt_number: run.attempt_number,
			status: run.status.clone(),
			attempt_status: run.attempt_status.clone(),
			phase: run.phase.clone(),
			wait_reason: run.wait_reason.clone(),
			current_operation: run.current_operation.clone(),
			run_lease: run.run_lease,
			execution_liveness: run.execution_liveness.clone(),
			ownership_state: run.ownership_state.clone(),
			liveness_state: run.liveness_state.clone(),
			policy_state: run.policy_state.clone(),
			terminalization_state: run.terminalization_state.clone(),
			lane_control_next_action: run.lane_control_next_action.clone(),
			lane_control_conditions: run.lane_control_conditions.clone(),
			thread_id: run.thread_id.clone(),
			turn_id: run.turn_id.clone(),
			thread_status: run.thread_status.clone(),
			process_id: run.process_id,
			process_alive: run.process_alive,
			process_liveness_reason: run.process_liveness_reason.clone(),
			last_event_type: run.last_event_type.clone(),
			last_event_at: run.last_event_at.clone(),
			event_count: run.event_count,
			worktree_path: run.worktree_path.clone(),
			soft_interrupt_available: soft_interrupt_available_for_run(run),
			hard_interrupt_available: hard_interrupt_available_for_run(run),
			hard_interrupt_requires_force: true,
		}
	}
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct LaneSoftInterruptReport {
	pub(super) attempted: bool,
	pub(super) available: bool,
	pub(super) status: String,
	pub(super) classification: String,
	pub(super) method: String,
	pub(super) request_id: Option<String>,
	pub(super) message: String,
	pub(super) error_class: Option<String>,
	pub(super) protocol_summary: Option<String>,
	pub(super) response: Option<LaneControlInterruptResponse>,
}
impl LaneSoftInterruptReport {
	pub(super) fn unavailable(error_class: &str, message: &str) -> Self {
		Self {
			attempted: false,
			available: false,
			status: String::from("unavailable"),
			classification: String::from("soft_interrupt_unavailable"),
			method: String::from("turn/interrupt"),
			request_id: None,
			message: message.to_owned(),
			error_class: Some(error_class.to_owned()),
			protocol_summary: None,
			response: None,
		}
	}

	pub(super) fn from_response(response: LaneControlInterruptResponse) -> Self {
		let status = match &response.status {
			LaneControlResponseStatus::SoftDelivered => "delivered",
			LaneControlResponseStatus::SoftFailed => "failed",
			LaneControlResponseStatus::Rejected => "rejected",
		};

		Self {
			attempted: true,
			available: true,
			status: String::from(status),
			classification: response.classification.clone(),
			method: response.method.clone(),
			request_id: Some(response.request_id.clone()),
			message: response.message.clone(),
			error_class: response.error_class.clone(),
			protocol_summary: response.protocol_summary.clone(),
			response: Some(response),
		}
	}

	pub(super) fn from_control_rejection(receipt: &RunControlActionReceipt) -> Self {
		Self {
			attempted: false,
			available: false,
			status: String::from("rejected"),
			classification: String::from("control_request_rejected"),
			method: String::from("turn/interrupt"),
			request_id: None,
			message: format!(
				"Run-control resolver rejected the interrupt request: {}.",
				receipt.reason()
			),
			error_class: Some(receipt.reason().to_owned()),
			protocol_summary: None,
			response: None,
		}
	}
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct LaneHardInterruptReport {
	pub(super) attempted: bool,
	pub(super) status: String,
	pub(super) classification: String,
	pub(super) signals: Vec<String>,
	pub(super) process_id: Option<u32>,
	pub(super) process_alive_after: Option<bool>,
	pub(super) message: String,
	pub(super) error_class: Option<String>,
}
impl LaneHardInterruptReport {
	pub(super) fn unavailable(error_class: &str, message: &str) -> Self {
		Self {
			attempted: false,
			status: String::from("unavailable"),
			classification: String::from("hard_interrupt_fallback"),
			signals: Vec::new(),
			process_id: None,
			process_alive_after: None,
			message: message.to_owned(),
			error_class: Some(error_class.to_owned()),
		}
	}
}

fn soft_interrupt_available_for_run(run: &OperatorRunStatus) -> bool {
	orchestrator::operator_run_counts_as_current_lane(run)
		&& run.worktree_path.is_some()
		&& run.thread_id.is_some()
		&& run.turn_id.is_some()
		&& run.control_capability.as_ref().is_some_and(|capability| capability.status == "active")
}

fn hard_interrupt_available_for_run(run: &OperatorRunStatus) -> bool {
	run.phase != "terminal_pending" && run.process_id.is_some() && run.process_alive != Some(false)
}
