use crate::orchestrator::types::{
	IssueDispatchMode, Path, PathBuf, ProgramDispatchSelection, RetryQueue, ServiceConfig,
	StateStore, TrackerIssue, WorkflowDocument, WorktreeManager, WorktreeSpec,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RunSummary {
	pub(crate) project_id: String,
	pub(crate) issue_id: String,
	pub(crate) issue_identifier: String,
	pub(crate) issue_state: String,
	pub(crate) initial_issue_state: String,
	#[cfg(test)]
	pub(crate) retry_project_slug: String,
	pub(crate) dispatch_mode: IssueDispatchMode,
	pub(crate) branch_name: String,
	pub(crate) worktree_path: PathBuf,
	pub(crate) attempt_number: i64,
	pub(crate) run_id: String,
	pub(crate) continuation_pending: bool,
	pub(crate) program_dispatch: Option<ProgramDispatchSelection>,
}

#[derive(Clone, Debug)]
pub(crate) struct IssueRunPlan {
	pub(crate) issue: TrackerIssue,
	pub(crate) issue_state: String,
	pub(crate) initial_issue_state: String,
	pub(crate) worktree: WorktreeSpec,
	#[allow(dead_code)]
	#[cfg(test)]
	pub(crate) retry_project_slug: String,
	pub(crate) dispatch_mode: IssueDispatchMode,
	pub(crate) attempt_number: i64,
	pub(crate) run_id: String,
	pub(crate) retry_budget_base: i64,
}

#[derive(Default)]
pub(crate) struct RecoveredRuntimeState {
	pub(crate) recoverable_issues: Vec<TrackerIssue>,
}

#[derive(Clone, Copy)]
pub(crate) struct RunCycleRequest<'a> {
	pub(crate) config_path: &'a Path,
	pub(crate) state_store: &'a StateStore,
	pub(crate) dry_run: bool,
	pub(crate) preferred_issue_id: Option<&'a str>,
	pub(crate) preferred_issue_state: Option<&'a str>,
	pub(crate) preferred_initial_issue_state: Option<&'a str>,
	pub(crate) preferred_lease_acquired: bool,
	pub(crate) preferred_issue_claim_fd: Option<i32>,
	pub(crate) preferred_dispatch_slot_fd: Option<i32>,
	pub(crate) preferred_dispatch_slot_index: Option<usize>,
	pub(crate) preferred_dispatch_mode: Option<IssueDispatchMode>,
	pub(crate) preferred_run_identity: Option<PreferredRunIdentity<'a>>,
	pub(crate) preferred_retry_budget_base: Option<i64>,
	pub(crate) preferred_workflow_snapshot: Option<&'a str>,
}

#[derive(Clone, Copy)]
pub(crate) struct PrepareIssueRunContext<'a, T> {
	pub(crate) tracker: &'a T,
	pub(crate) project: &'a ServiceConfig,
	pub(crate) workflow: &'a WorkflowDocument,
	pub(crate) state_store: &'a StateStore,
	pub(crate) worktree_manager: &'a WorktreeManager,
	pub(crate) dry_run: bool,
	pub(crate) lease_preacquired: bool,
	pub(crate) dispatch_mode: IssueDispatchMode,
	pub(crate) preferred_issue_state: Option<&'a str>,
	pub(crate) preferred_initial_issue_state: Option<&'a str>,
	pub(crate) preferred_run_identity: Option<PreferredRunIdentity<'a>>,
	pub(crate) preferred_retry_budget_base: Option<i64>,
}

#[derive(Clone, Copy)]
pub(crate) struct PreferredRunIdentity<'a> {
	pub(crate) run_id: &'a str,
	pub(crate) attempt_number: i64,
}

pub(crate) struct ChildExitRetryContext<'a, T> {
	pub(crate) retry_queue: &'a mut RetryQueue,
	pub(crate) tracker: &'a T,
	pub(crate) project: &'a ServiceConfig,
	pub(crate) workflow: &'a WorkflowDocument,
	pub(crate) state_store: &'a StateStore,
}

#[derive(Clone, Copy)]
pub(crate) struct TargetIssueRunContext<'a, T> {
	pub(crate) tracker: &'a T,
	pub(crate) project: &'a ServiceConfig,
	pub(crate) workflow: &'a WorkflowDocument,
	pub(crate) state_store: &'a StateStore,
	pub(crate) issue_id: &'a str,
	pub(crate) preferred_issue_state: Option<&'a str>,
	pub(crate) preferred_initial_issue_state: Option<&'a str>,
	pub(crate) dry_run: bool,
	pub(crate) lease_preacquired: bool,
	pub(crate) preferred_issue_claim_fd: Option<i32>,
	pub(crate) preferred_dispatch_slot_fd: Option<i32>,
	pub(crate) preferred_dispatch_slot_index: Option<usize>,
	pub(crate) dispatch_mode: IssueDispatchMode,
	pub(crate) preferred_run_identity: Option<PreferredRunIdentity<'a>>,
	pub(crate) preferred_retry_budget_base: Option<i64>,
}
