use crate::orchestrator::types::{
	Duration, File, IssueDispatchMode, Path, PathBuf, RetryQueue, Serialize, ServiceConfig,
	StateStore, TrackerIssue, WorkflowDocument, WorktreeManager, WorktreeSpec,
};

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

/// Multi-project local control-plane daemon request.
pub(crate) struct ServeRequest<'a> {
	pub(crate) config_path: Option<&'a Path>,
	pub(crate) listen_address: &'a str,
	pub(crate) dev: bool,
}

/// Agent-readable runtime diagnosis request.
pub(crate) struct DiagnoseRequest<'a> {
	pub(crate) config_path: Option<&'a Path>,
	pub(crate) json: bool,
	pub(crate) limit: usize,
}

/// Local private execution evidence readback request.
pub(crate) struct EvidenceRequest<'a> {
	pub(crate) config_path: Option<&'a Path>,
	pub(crate) project_id: Option<&'a str>,
	pub(crate) issue: &'a str,
	pub(crate) run_id: Option<&'a str>,
	pub(crate) attempt_number: Option<i64>,
	pub(crate) json: bool,
	pub(crate) include_payload: bool,
}

/// Current lane steer request.
pub(crate) struct LaneSteerRequest<'a> {
	pub(crate) config_path: Option<&'a Path>,
	pub(crate) project_id: Option<&'a str>,
	pub(crate) issue: &'a str,
	pub(crate) run_id: &'a str,
	pub(crate) expected_turn_id: &'a str,
	pub(crate) message: &'a str,
	pub(crate) source: &'a str,
	pub(crate) wait_timeout: Duration,
}

/// Current lane steer result without raw operator message content.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LaneSteerReport {
	pub(crate) project_id: String,
	pub(crate) issue_id: String,
	pub(crate) issue_identifier: Option<String>,
	pub(crate) run_id: String,
	pub(crate) attempt_number: i64,
	pub(crate) thread_id: Option<String>,
	pub(crate) expected_turn_id: String,
	pub(crate) current_turn_id: Option<String>,
	pub(crate) response_turn_id: Option<String>,
	pub(crate) audit_record_id: i64,
	pub(crate) request_id: String,
	pub(crate) request_path: Option<String>,
	pub(crate) outcome: String,
	pub(crate) reason: String,
	pub(crate) failure_class: Option<String>,
	pub(crate) delivery_status: String,
	pub(crate) message_byte_count: usize,
	pub(crate) message_line_count: usize,
}

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
}

pub(crate) struct MaterializedDaemonSpawnState {
	pub(crate) worktree: WorktreeSpec,
	pub(crate) retry_budget_base: i64,
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
