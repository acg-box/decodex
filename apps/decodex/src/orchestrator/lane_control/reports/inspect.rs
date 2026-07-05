use serde::Serialize;

use crate::orchestrator::{self, OperatorRunStatus};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LaneInspectReport {
	pub(in crate::orchestrator::lane_control) project_id: String,
	pub(in crate::orchestrator::lane_control) issue: String,
	pub(in crate::orchestrator::lane_control) run_id: Option<String>,
	pub(in crate::orchestrator::lane_control) matched_run_count: usize,
	pub(in crate::orchestrator::lane_control) runs: Vec<LaneRunInspect>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LaneRunInspect {
	pub(in crate::orchestrator::lane_control) project_id: String,
	pub(in crate::orchestrator::lane_control) issue_id: String,
	pub(in crate::orchestrator::lane_control) issue_identifier: Option<String>,
	pub(in crate::orchestrator::lane_control) run_id: String,
	pub(in crate::orchestrator::lane_control) attempt_number: i64,
	pub(in crate::orchestrator::lane_control) status: String,
	pub(in crate::orchestrator::lane_control) attempt_status: String,
	pub(in crate::orchestrator::lane_control) phase: String,
	pub(in crate::orchestrator::lane_control) wait_reason: Option<String>,
	pub(in crate::orchestrator::lane_control) current_operation: String,
	pub(in crate::orchestrator::lane_control) run_lease: bool,
	pub(in crate::orchestrator::lane_control) execution_liveness: String,
	pub(in crate::orchestrator::lane_control) ownership_state: String,
	pub(in crate::orchestrator::lane_control) liveness_state: String,
	pub(in crate::orchestrator::lane_control) policy_state: String,
	pub(in crate::orchestrator::lane_control) terminalization_state: String,
	pub(in crate::orchestrator::lane_control) lane_control_next_action: String,
	pub(in crate::orchestrator::lane_control) lane_control_conditions: Vec<String>,
	pub(in crate::orchestrator::lane_control) thread_id: Option<String>,
	pub(in crate::orchestrator::lane_control) turn_id: Option<String>,
	pub(in crate::orchestrator::lane_control) thread_status: Option<String>,
	pub(in crate::orchestrator::lane_control) process_id: Option<u32>,
	pub(in crate::orchestrator::lane_control) process_alive: Option<bool>,
	pub(in crate::orchestrator::lane_control) process_liveness_reason: Option<String>,
	pub(in crate::orchestrator::lane_control) last_event_type: Option<String>,
	pub(in crate::orchestrator::lane_control) last_event_at: Option<String>,
	pub(in crate::orchestrator::lane_control) event_count: i64,
	pub(in crate::orchestrator::lane_control) worktree_path: Option<String>,
	pub(in crate::orchestrator::lane_control) soft_interrupt_available: bool,
	pub(in crate::orchestrator::lane_control) hard_interrupt_available: bool,
	pub(in crate::orchestrator::lane_control) hard_interrupt_requires_force: bool,
}
impl LaneRunInspect {
	pub(in crate::orchestrator::lane_control) fn from_operator_run(
		run: &OperatorRunStatus,
	) -> Self {
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
