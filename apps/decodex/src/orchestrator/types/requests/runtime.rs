use crate::orchestrator::types::{IssueDispatchMode, Path, WorktreeSpec};

/// One bounded run invocation and its optional daemon-planned overrides.
pub(crate) struct RunOnceRequest<'a> {
	pub(crate) config_path: Option<&'a Path>,
	pub(crate) dry_run: bool,
	pub(crate) explain_queue: bool,
	pub(crate) preferred_issue_id: Option<&'a str>,
	pub(crate) preferred_issue_state: Option<&'a str>,
	pub(crate) preferred_initial_issue_state: Option<&'a str>,
	pub(crate) preferred_lease_acquired: bool,
	pub(crate) preferred_issue_claim_fd: Option<i32>,
	pub(crate) preferred_dispatch_slot_fd: Option<i32>,
	pub(crate) preferred_dispatch_slot_index: Option<usize>,
	pub(crate) preferred_dispatch_mode: Option<IssueDispatchMode>,
	pub(crate) preferred_run_id: Option<&'a str>,
	pub(crate) preferred_attempt_number: Option<i64>,
	pub(crate) preferred_retry_budget_base: Option<i64>,
	pub(crate) preferred_workflow_snapshot: Option<&'a str>,
}

pub(crate) struct MaterializedDaemonSpawnState {
	pub(crate) worktree: WorktreeSpec,
	pub(crate) retry_budget_base: i64,
}
