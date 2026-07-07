mod data;
mod derived_program;
mod guards;
mod locks;
mod ordering;

pub(super) use self::{
	data::StateData,
	derived_program::{
		apply_derived_program_intake_state, derived_program_intake_plan_records,
		derived_program_issue_mapping_records,
	},
	guards::{DispatchSlotConfig, DispatchSlotGuard, IssueClaimGuard},
	locks::{
		acquire_shared_lock_coordinator, dispatch_slot_lock_path, issue_claim_id_from_path,
		issue_claim_lock_path, prune_unlocked_shared_lock_files, read_issue_claim_record,
		remove_lock_file_if_exists, write_issue_claim_record,
	},
	ordering::compare_project_run_status,
};
#[cfg(unix)]
pub(super) use locks::{clear_close_on_exec, set_close_on_exec};

use crate::state::{
	ChildAgentActivitySummary, CodexAccountActivitySummary, ProtocolActivitySummary,
};

pub(crate) struct EffectiveRuntimeMarker<'a> {
	pub(crate) thread_id: Option<&'a str>,
	pub(crate) turn_id: Option<&'a str>,
	pub(crate) effective_model: &'a str,
	pub(crate) effective_model_provider: &'a str,
	pub(crate) effective_cwd: &'a str,
	pub(crate) effective_approval_policy: &'a str,
	pub(crate) effective_approvals_reviewer: &'a str,
	pub(crate) effective_sandbox_mode: &'a str,
}

pub(crate) struct ProtocolActivityMarker<'a> {
	pub(crate) run_id: &'a str,
	pub(crate) attempt_number: i64,
	pub(crate) thread_id: Option<&'a str>,
	pub(crate) turn_id: Option<&'a str>,
	pub(crate) event_count: i64,
	pub(crate) last_event_type: &'a str,
	pub(crate) child_agent_activity: Option<&'a ChildAgentActivitySummary>,
	pub(crate) protocol_activity: Option<&'a ProtocolActivitySummary>,
}

pub(crate) struct CodexAccountMarker<'a> {
	pub(crate) run_id: &'a str,
	pub(crate) attempt_number: i64,
	pub(crate) account: &'a CodexAccountActivitySummary,
	pub(crate) accounts: &'a [CodexAccountActivitySummary],
}
