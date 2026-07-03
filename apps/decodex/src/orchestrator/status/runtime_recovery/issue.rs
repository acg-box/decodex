use crate::{
	orchestrator::status::{
		self, IssueTracker, RetryIssueStateHint, ServiceConfig, StateStore, TrackerIssue,
		WorkflowDocument, WorktreeManager, WorktreeMapping, WorktreeSpec, runtime_recovery::lease,
	},
	prelude::Result,
	state, tracker,
};

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
	let planned_worktree = worktree_manager.plan_for_issue(&issue.identifier);
	let existing_worktree_mapping = state_store.worktree_for_issue(&issue.id)?;
	let existing_worktree = existing_recoverable_worktree_spec(
		project.service_id(),
		&issue,
		existing_worktree_mapping.as_ref(),
	)?;
	let worktree = existing_worktree.unwrap_or(planned_worktree);

	if !worktree.path.exists() {
		return Ok(None);
	}

	state_store.canonicalize_issue_identity(&issue.identifier, &issue.id)?;

	let activity_marker = state::read_run_activity_marker_snapshot(&worktree.path)?;
	let recovered_service_ownership =
		issue_has_recovered_service_ownership(tracker, &issue, project.service_id())?;

	if existing_worktree_mapping.is_none() && recovered_service_ownership {
		lease::upsert_recovered_worktree_mapping(
			project,
			state_store,
			&issue,
			&worktree,
			activity_marker.as_ref(),
		)?;
	}
	if issue.state.name == workflow.frontmatter().tracker().success_state()
		&& recovered_service_ownership
		&& let Some(marker) = activity_marker.as_ref()
		&& status::worktree_activity_marker_is_fresh(marker, now_unix_epoch)
	{
		lease::upsert_recovered_worktree_mapping(
			project,
			state_store,
			&issue,
			&worktree,
			activity_marker.as_ref(),
		)?;
		lease::record_recovered_activity_lease(project, state_store, &issue, marker)?;

		return Ok(None);
	}
	if status::issue_passes_closeout_dispatch_policy(
		tracker,
		&issue,
		project,
		workflow,
		state_store,
	)? {
		lease::upsert_recovered_worktree_mapping(
			project,
			state_store,
			&issue,
			&worktree,
			activity_marker.as_ref(),
		)?;

		match activity_marker.as_ref() {
			Some(marker) if status::worktree_activity_marker_is_fresh(marker, now_unix_epoch) => {
				lease::record_recovered_activity_lease(project, state_store, &issue, marker)?;

				return Ok(None);
			},
			_ => {},
		}
	}
	if status::issue_passes_retry_dispatch_policy(
		tracker,
		&issue,
		project,
		workflow,
		state_store,
		RetryIssueStateHint::default(),
	)? {
		lease::upsert_recovered_worktree_mapping(
			project,
			state_store,
			&issue,
			&worktree,
			activity_marker.as_ref(),
		)?;

		match activity_marker.as_ref() {
			Some(marker) if status::worktree_activity_marker_is_fresh(marker, now_unix_epoch) => {
				lease::record_recovered_activity_lease(project, state_store, &issue, marker)?;

				return Ok(None);
			},
			Some(marker) => {
				status::clear_recovered_issue_lease(
					project.service_id(),
					&issue.id,
					Some(marker.run_id()),
					state_store,
				)?;
			},
			None => {
				status::clear_recovered_issue_lease(
					project.service_id(),
					&issue.id,
					None,
					state_store,
				)?;
			},
		}

		return Ok(Some(issue));
	}

	Ok(None)
}

pub(crate) fn existing_recoverable_worktree_spec(
	project_id: &str,
	issue: &TrackerIssue,
	mapping: Option<&WorktreeMapping>,
) -> Result<Option<WorktreeSpec>> {
	let Some(mapping) = mapping else {
		return Ok(None);
	};

	if mapping.project_id() != project_id || !mapping.worktree_path().try_exists()? {
		return Ok(None);
	}

	Ok(Some(WorktreeSpec {
		branch_name: mapping.branch_name().to_owned(),
		issue_identifier: issue.identifier.clone(),
		path: mapping.worktree_path().to_path_buf(),
		reused_existing: true,
	}))
}

pub(crate) fn issue_has_recovered_service_ownership<T>(
	tracker: &T,
	issue: &TrackerIssue,
	service_id: &str,
) -> Result<bool>
where
	T: IssueTracker,
{
	tracker::issue_has_label_with_server_confirmation(
		tracker,
		issue,
		&tracker::automation_active_label(service_id),
	)
}
