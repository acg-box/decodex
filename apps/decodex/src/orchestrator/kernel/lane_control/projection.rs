mod action;
mod conditions;
mod liveness;
mod ownership;
mod policy;
mod terminalization;

use crate::orchestrator::kernel::{
	lane_control::model::{LaneControlKernelInput, LaneControlKernelProjection},
	state::{LaneStateAxes, OwnershipState},
};

pub(crate) fn project_lane_control(
	input: &LaneControlKernelInput<'_>,
) -> LaneControlKernelProjection {
	let has_fresh_execution = input.status_starting_or_running
		&& (input.process_alive == Some(true) || input.protocol_recent);
	let has_live_execution = input.status_starting_or_running
		&& (input.execution_liveness_observed
			|| input.process_alive == Some(true)
			|| input.thread_active
			|| input.protocol_recent);
	let has_authoritative_live_owner =
		input.run_lease || input.process_alive == Some(true) || input.thread_active;
	let needs_attention_signal = input.status_needs_attention_or_terminal_failure
		|| input.phase_needs_attention
		|| input.suspected_stall
		|| input.phase_stalled
		|| input.thread_terminal_failure
		|| input.process_alive == Some(false) && input.status_starting_or_running
		|| input.stale_execution_without_known_process;
	let liveness = liveness::lane_control_liveness(input);
	let terminalization = terminalization::lane_control_terminalization(input, liveness);
	let ownership = ownership::lane_control_ownership(
		input,
		liveness,
		input.policy,
		terminalization,
		needs_attention_signal,
	);
	let next_action =
		action::lane_control_next_action(input, ownership, liveness, input.policy, terminalization);
	let conditions = conditions::lane_control_conditions(input, ownership, liveness, input.policy);
	let counts_as_attention = needs_attention_signal
		|| ownership == OwnershipState::RetainedAttention
		|| policy::policy_requires_attention(input.policy);
	let counts_as_running = ownership == OwnershipState::LeasedRun
		&& input.status_starting_or_running
		&& input.phase_executing
		&& input.process_alive != Some(false)
		&& !needs_attention_signal;
	let counts_as_current_lane = (input.run_lease || has_live_execution) && !input.phase_terminal;

	LaneControlKernelProjection {
		axes: LaneStateAxes::new(ownership, liveness, input.policy, terminalization),
		next_action,
		conditions,
		has_fresh_execution,
		has_live_execution,
		has_authoritative_live_owner,
		needs_attention_signal,
		counts_as_attention,
		counts_as_running,
		counts_as_current_lane,
	}
}
