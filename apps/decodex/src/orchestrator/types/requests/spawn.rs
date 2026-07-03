use crate::orchestrator::types::{File, IssueDispatchMode, Path, WorkflowDocument};

pub(crate) struct SpawnRunOnceChildRequest<'a> {
	pub(crate) config_path: &'a Path,
	pub(crate) preferred_issue_id: &'a str,
	pub(crate) preferred_issue_state: &'a str,
	pub(crate) preferred_initial_issue_state: Option<&'a str>,
	pub(crate) dispatch_mode: IssueDispatchMode,
	pub(crate) preferred_run_id: &'a str,
	pub(crate) preferred_attempt_number: i64,
	pub(crate) preferred_retry_budget_base: i64,
	pub(crate) workflow: &'a WorkflowDocument,
	pub(crate) issue_claim_handoff: Option<&'a File>,
	pub(crate) dispatch_slot_handoff: Option<&'a File>,
	pub(crate) dispatch_slot_index_handoff: Option<usize>,
}
