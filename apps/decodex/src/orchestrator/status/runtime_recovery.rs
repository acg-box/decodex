mod discovery;
mod issue;
mod lease;
mod snapshot;

pub(crate) use self::{
	discovery::{recoverable_worktree_identifiers, refresh_recoverable_runtime_issues},
	snapshot::{append_primary_account_if_missing, hydrate_status_snapshot_state},
};

use crate::{
	orchestrator::status::{
		self, BTreeSet, IssueTracker, OffsetDateTime, RecoverableWorktreeSkipCache,
		RecoveredRuntimeState, ServiceConfig, StateStore, TrackerIssue, WorkflowDocument,
		WorktreeManager, compare_issue_candidates,
	},
	prelude::Result,
};

pub(crate) fn recover_runtime_state_from_tracker_and_worktrees<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
) -> Result<RecoveredRuntimeState>
where
	T: IssueTracker,
{
	recover_runtime_state_with_skip_cache(tracker, project, workflow, state_store, None)
}

pub(crate) fn recover_runtime_state_with_skip_cache<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	mut recoverable_worktree_skip_cache: Option<&mut RecoverableWorktreeSkipCache>,
) -> Result<RecoveredRuntimeState>
where
	T: IssueTracker,
{
	let worktree_manager =
		WorktreeManager::new(project.service_id(), project.repo_root(), project.worktree_root());
	let active_issue_ids = status::active_shared_issue_ids(project, state_store)?;
	let mut issue_ids = Vec::new();

	for mapping in state_store.list_worktrees(project.service_id())? {
		if status::worktree_mapping_is_stale_terminal_local_residue(
			project,
			state_store,
			&mapping,
			&active_issue_ids,
		)? {
			continue;
		}

		issue_ids.push(mapping.issue_id().to_owned());
	}
	for lease in state_store.list_active_shared_leases(project.service_id())? {
		if !issue_ids.iter().any(|issue_id| issue_id == lease.issue_id()) {
			issue_ids.push(lease.issue_id().to_owned());
		}
	}

	let mut issues = if issue_ids.is_empty() && recoverable_worktree_skip_cache.is_some() {
		Vec::new()
	} else {
		discovery::refresh_recoverable_runtime_issues(tracker, &issue_ids)?
	};
	let mut known_identifiers =
		issues.iter().map(|issue| issue.identifier.to_ascii_uppercase()).collect::<BTreeSet<_>>();

	for issue_identifier in discovery::recoverable_worktree_identifiers(project.worktree_root())? {
		discovery::append_recoverable_tracker_issue(
			tracker,
			project,
			&issue_identifier,
			&mut known_identifiers,
			&mut issues,
			recoverable_worktree_skip_cache.as_deref_mut(),
		)?;
	}

	let now_unix_epoch = OffsetDateTime::now_utc().unix_timestamp();
	let mut recoverable_issues = Vec::new();

	for issue in issues {
		if let Some(recoverable_issue) = recover_issue_runtime_state(
			tracker,
			project,
			workflow,
			state_store,
			&worktree_manager,
			issue,
			now_unix_epoch,
		)? {
			recoverable_issues.push(recoverable_issue);
		}
	}

	recoverable_issues.sort_by(compare_issue_candidates);

	Ok(RecoveredRuntimeState { recoverable_issues })
}

pub(crate) fn recover_issue_runtime_state<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	worktree_manager: &WorktreeManager,
	issue: TrackerIssue,
	now_unix_epoch: i64,
) -> Result<Option<TrackerIssue>>
where
	T: IssueTracker,
{
	issue::recover_issue_runtime_state(
		tracker,
		project,
		workflow,
		state_store,
		worktree_manager,
		issue,
		now_unix_epoch,
	)
}
