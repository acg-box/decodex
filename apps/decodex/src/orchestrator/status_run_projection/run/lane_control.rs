mod input;

use crate::orchestrator::{OperatorLaneControlProjection, OperatorRunStatus, kernel::lane_control};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct OperatorRunLaneControlReadback {
	pub(crate) has_live_execution: bool,
	pub(crate) has_authoritative_live_owner: bool,
	pub(crate) counts_as_current_lane: bool,
	pub(crate) counts_as_running: bool,
	pub(crate) counts_as_attention: bool,
}

struct OperatorRunLaneControlState {
	projection: OperatorLaneControlProjection,
	has_fresh_execution: bool,
	readback: OperatorRunLaneControlReadback,
	counts_as_attention: bool,
	counts_as_running: bool,
}

pub(super) fn hydrate_operator_run_derived_status(
	mut status: OperatorRunStatus,
) -> OperatorRunStatus {
	let lane_control_state = operator_lane_control_state(&status);

	status.has_fresh_execution = lane_control_state.has_fresh_execution;
	status.ownership_state = lane_control_state.projection.ownership_state;
	status.liveness_state = lane_control_state.projection.liveness_state;
	status.policy_state = lane_control_state.projection.policy_state;
	status.terminalization_state = lane_control_state.projection.terminalization_state;
	status.lane_control_next_action = lane_control_state.projection.next_action;
	status.lane_control_conditions = lane_control_state.projection.conditions;
	status.needs_attention = lane_control_state.counts_as_attention;
	status.counts_as_running = lane_control_state.counts_as_running;

	status
}

pub(crate) fn operator_run_lane_control_readback(
	run: &OperatorRunStatus,
) -> OperatorRunLaneControlReadback {
	operator_lane_control_state(run).readback
}

fn operator_lane_control_state(run: &OperatorRunStatus) -> OperatorRunLaneControlState {
	let projection =
		lane_control::project_lane_control(&input::operator_lane_control_kernel_input(run));

	OperatorRunLaneControlState {
		projection: OperatorLaneControlProjection {
			ownership_state: projection.axes.ownership.as_str().to_owned(),
			liveness_state: projection.axes.liveness.as_str().to_owned(),
			policy_state: projection.axes.policy.as_str().to_owned(),
			terminalization_state: projection.axes.terminalization.as_str().to_owned(),
			next_action: projection.next_action,
			conditions: projection.conditions.into_iter().map(str::to_owned).collect(),
		},
		has_fresh_execution: projection.has_fresh_execution,
		readback: OperatorRunLaneControlReadback {
			has_live_execution: projection.has_live_execution,
			has_authoritative_live_owner: projection.has_authoritative_live_owner,
			counts_as_current_lane: projection.counts_as_current_lane,
			counts_as_running: projection.counts_as_running,
			counts_as_attention: projection.counts_as_attention,
		},
		counts_as_attention: projection.counts_as_attention,
		counts_as_running: projection.counts_as_running,
	}
}
