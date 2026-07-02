use crate::orchestrator::kernel::state::{LaneStateAxes, PolicyState};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::orchestrator) struct LaneControlKernelInput<'a> {
	pub(in crate::orchestrator) run_lease: bool,
	pub(in crate::orchestrator) attempt_active: bool,
	pub(in crate::orchestrator) attempt_terminal: bool,
	pub(in crate::orchestrator) status_starting_or_running: bool,
	pub(in crate::orchestrator) status_needs_attention_or_terminal_failure: bool,
	pub(in crate::orchestrator) phase_executing: bool,
	pub(in crate::orchestrator) phase_needs_attention: bool,
	pub(in crate::orchestrator) phase_stalled: bool,
	pub(in crate::orchestrator) phase_terminal: bool,
	pub(in crate::orchestrator) cleanup_complete_signal: bool,
	pub(in crate::orchestrator) current_operation_ledger_outcome: bool,
	pub(in crate::orchestrator) wait_reason_present: bool,
	pub(in crate::orchestrator) continuation_wait: bool,
	pub(in crate::orchestrator) process_alive: Option<bool>,
	pub(in crate::orchestrator) execution_liveness_observed: bool,
	pub(in crate::orchestrator) host_boot_mismatch: bool,
	pub(in crate::orchestrator) not_running_signal: bool,
	pub(in crate::orchestrator) thread_active: bool,
	pub(in crate::orchestrator) protocol_recent: bool,
	pub(in crate::orchestrator) suspected_stall: bool,
	pub(in crate::orchestrator) stale_execution_without_known_process: bool,
	pub(in crate::orchestrator) policy: PolicyState,
	pub(in crate::orchestrator) loop_next_action: Option<&'a str>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::orchestrator) struct LaneControlKernelProjection {
	pub(in crate::orchestrator) axes: LaneStateAxes,
	pub(in crate::orchestrator) next_action: String,
	pub(in crate::orchestrator) conditions: Vec<&'static str>,
	pub(in crate::orchestrator) has_fresh_execution: bool,
	pub(in crate::orchestrator) has_live_execution: bool,
	pub(in crate::orchestrator) has_authoritative_live_owner: bool,
	pub(in crate::orchestrator) needs_attention_signal: bool,
	pub(in crate::orchestrator) counts_as_attention: bool,
	pub(in crate::orchestrator) counts_as_running: bool,
	pub(in crate::orchestrator) counts_as_current_lane: bool,
}
