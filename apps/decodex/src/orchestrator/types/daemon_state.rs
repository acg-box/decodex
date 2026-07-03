use crate::orchestrator::types::{
	Child, HashMap, Instant, IssueDispatchMode, LinearClient, PathBuf,
	RECOVERABLE_WORKTREE_SKIP_TTL, RetryKind, RunAttempt, RunLeaseDisposition, ServiceConfig,
	TrackerIssue, WorkflowDocument, WorktreeManager, WorktreeMapping,
};

pub(crate) struct DaemonRunChild {
	pub(crate) child: Child,
	pub(crate) issue_id: String,
	pub(crate) run_id: String,
	pub(crate) attempt_number: i64,
	pub(crate) initial_issue_state: String,
	#[allow(dead_code)]
	#[cfg(test)]
	pub(crate) retry_project_slug: String,
	pub(crate) dispatch_mode: IssueDispatchMode,
	pub(crate) from_retry_queue: bool,
	pub(crate) workflow: WorkflowDocument,
}

#[derive(Clone, Copy)]
pub(crate) struct ChildRunRef<'a> {
	pub(crate) issue_id: &'a str,
	pub(crate) run_id: &'a str,
	pub(crate) attempt_number: i64,
}

#[derive(Clone, Copy)]
pub(crate) struct CurrentChildRunContext<'a> {
	pub(crate) child: ChildRunRef<'a>,
	pub(crate) workflow: &'a WorkflowDocument,
	pub(crate) dispatch_mode: IssueDispatchMode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RetryEntryLifecycle {
	Active,
	ReviewRepair,
	Closeout,
}
impl RetryEntryLifecycle {
	pub(crate) const fn for_dispatch_mode(dispatch_mode: IssueDispatchMode) -> Self {
		match dispatch_mode {
			IssueDispatchMode::ReviewRepair => Self::ReviewRepair,
			IssueDispatchMode::Closeout => Self::Closeout,
			IssueDispatchMode::Normal | IssueDispatchMode::Program | IssueDispatchMode::Retry =>
				Self::Active,
		}
	}
}

#[derive(Clone, Debug)]
pub(crate) struct RetryEntry {
	pub(crate) issue_id: String,
	#[allow(dead_code)]
	#[cfg(test)]
	pub(crate) retry_project_slug: String,
	pub(crate) continuation_initial_issue_state: Option<String>,
	pub(crate) lifecycle: RetryEntryLifecycle,
	pub(crate) dispatch_mode: IssueDispatchMode,
	pub(crate) kind: RetryKind,
	pub(crate) attempt: u32,
	pub(crate) ready_at: Instant,
}

#[derive(Default)]
pub(crate) struct RetryQueue {
	pub(crate) entries: HashMap<String, RetryEntry>,
}
impl RetryQueue {
	pub(crate) fn is_empty(&self) -> bool {
		self.entries.is_empty()
	}

	pub(crate) fn upsert(&mut self, entry: RetryEntry) {
		self.entries.insert(entry.issue_id.clone(), entry);
	}

	pub(crate) fn release(&mut self, issue_id: &str) {
		self.entries.remove(issue_id);
	}

	pub(crate) fn next_entry(&self) -> Option<&RetryEntry> {
		self.entries.values().min_by(|left, right| {
			left.ready_at.cmp(&right.ready_at).then_with(|| left.issue_id.cmp(&right.issue_id))
		})
	}

	pub(crate) fn ordered_entries(&self) -> Vec<RetryEntry> {
		let mut entries = self.entries.values().cloned().collect::<Vec<_>>();

		entries.sort_by(|left, right| {
			left.ready_at.cmp(&right.ready_at).then_with(|| left.issue_id.cmp(&right.issue_id))
		});

		entries
	}
}

#[derive(Default)]
pub(crate) struct RecoverableWorktreeSkipCache {
	pub(crate) entries: HashMap<String, Instant>,
}
impl RecoverableWorktreeSkipCache {
	pub(crate) fn is_suppressed(&mut self, issue_identifier: &str, now: Instant) -> bool {
		self.retain_active(now);

		self.entries.get(&issue_identifier.to_ascii_uppercase()).is_some_and(|until| *until > now)
	}

	pub(crate) fn remember(&mut self, issue_identifier: &str, now: Instant) {
		self.retain_active(now);
		self.entries
			.insert(issue_identifier.to_ascii_uppercase(), now + RECOVERABLE_WORKTREE_SKIP_TTL);
	}

	pub(crate) fn retain_active(&mut self, now: Instant) {
		self.entries.retain(|_, until| *until > now);
	}
}

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

#[derive(Clone)]
pub(crate) struct CachedWorkflowDocument {
	pub(crate) path: PathBuf,
	pub(crate) document: WorkflowDocument,
}

#[derive(Clone, Copy)]
pub(crate) struct ActiveWorkflowOverride<'a> {
	pub(crate) child: ChildRunRef<'a>,
	pub(crate) workflow: &'a WorkflowDocument,
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
