use crate::orchestrator::kernel::state::{LaneStateAxes, PolicyState};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LaneControlKernelInput<'a> {
	pub(crate) run_lease: bool,
	pub(crate) attempt_active: bool,
	pub(crate) attempt_terminal: bool,
	pub(crate) status_starting_or_running: bool,
	pub(crate) status_needs_attention_or_terminal_failure: bool,
	pub(crate) phase_executing: bool,
	pub(crate) phase_needs_attention: bool,
	pub(crate) phase_stalled: bool,
	pub(crate) phase_terminal: bool,
	pub(crate) cleanup_complete_signal: bool,
	pub(crate) current_operation_ledger_outcome: bool,
	pub(crate) wait_reason_present: bool,
	pub(crate) continuation_wait: bool,
	pub(crate) process_alive: Option<bool>,
	pub(crate) execution_liveness_observed: bool,
	pub(crate) host_boot_mismatch: bool,
	pub(crate) not_running_signal: bool,
	pub(crate) thread_active: bool,
	pub(crate) thread_terminal_failure: bool,
	pub(crate) protocol_recent: bool,
	pub(crate) suspected_stall: bool,
	pub(crate) stale_execution_without_known_process: bool,
	pub(crate) policy: PolicyState,
	pub(crate) loop_next_action: Option<&'a str>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LaneControlKernelProjection {
	pub(crate) axes: LaneStateAxes,
	pub(crate) next_action: String,
	pub(crate) conditions: Vec<&'static str>,
	pub(crate) has_fresh_execution: bool,
	pub(crate) has_live_execution: bool,
	pub(crate) has_authoritative_live_owner: bool,
	pub(crate) needs_attention_signal: bool,
	pub(crate) counts_as_attention: bool,
	pub(crate) counts_as_running: bool,
	pub(crate) counts_as_current_lane: bool,
}
