use crate::orchestrator::types::{
	CachedWorkflowDocument, DaemonRunChild, Instant, LinearClient, RecoverableWorktreeSkipCache,
	RetryQueue, RunAttempt, RunLeaseDisposition, ServiceConfig, TrackerIssue, WorkflowDocument,
	WorktreeManager, WorktreeMapping,
};

pub(crate) struct DaemonTickContext {
	pub(crate) config: ServiceConfig,
	pub(crate) workflow: WorkflowDocument,
	pub(crate) tracker: LinearClient,
	pub(crate) worktree_manager: WorktreeManager,
}

#[derive(Default)]
pub(crate) struct ProjectDaemonRuntime {
	pub(crate) active_children: Vec<DaemonRunChild>,
	pub(crate) retry_queue: RetryQueue,
	pub(crate) tracker_backoff: Option<TrackerConnectorBackoff>,
	pub(crate) next_linear_scan_at: Option<Instant>,
	pub(crate) workflow_cache: Option<CachedWorkflowDocument>,
	pub(crate) recoverable_worktree_skip_cache: RecoverableWorktreeSkipCache,
}

#[derive(Clone, Debug)]
pub(crate) struct TrackerConnectorBackoff {
	pub(crate) until: Instant,
	pub(crate) quota_class: &'static str,
	pub(crate) reset_unix_epoch: i64,
	pub(crate) reset_source: &'static str,
	pub(crate) sync_phase: &'static str,
	pub(crate) warning: &'static str,
	pub(crate) next_action: &'static str,
}

#[derive(Clone, Debug)]
pub(crate) struct RunLeaseReconciliation {
	pub(crate) issue: TrackerIssue,
	pub(crate) run_attempt: RunAttempt,
	pub(crate) worktree_mapping: Option<WorktreeMapping>,
	pub(crate) disposition: RunLeaseDisposition,
	pub(crate) workflow: WorkflowDocument,
}

pub(crate) struct TerminalFailureOutcome {
	pub(crate) error_class: &'static str,
	pub(crate) retry_guarded_by_state: bool,
}
