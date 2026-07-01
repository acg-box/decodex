use super::{
	BTreeSet, CodexAccountActivitySummary, Instant, IssueTracker, OffsetDateTime, Path,
	RecoverableWorktreeSkipCache, RecoveredRuntimeState, RetryIssueStateHint, RunActivityMarker,
	ServiceConfig, StateStore, TrackerIssue, WorkflowDocument, WorktreeManager, WorktreeMapping,
	WorktreeSpec, active_shared_issue_ids, clear_recovered_issue_lease, compare_issue_candidates,
	fs, issue_passes_closeout_dispatch_policy, issue_passes_retry_dispatch_policy, slice, state,
	tracker, worktree_activity_marker_is_fresh, worktree_mapping_is_stale_terminal_local_residue,
};

use crate::commit_message;

pub(in crate::orchestrator) fn recover_runtime_state_from_tracker_and_worktrees<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
) -> crate::prelude::Result<RecoveredRuntimeState>
where
	T: IssueTracker,
{
	recover_runtime_state_from_tracker_and_worktrees_with_skip_cache(
		tracker,
		project,
		workflow,
		state_store,
		None,
	)
}

pub(in crate::orchestrator) fn recover_runtime_state_from_tracker_and_worktrees_with_skip_cache<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	mut recoverable_worktree_skip_cache: Option<&mut RecoverableWorktreeSkipCache>,
) -> crate::prelude::Result<RecoveredRuntimeState>
where
	T: IssueTracker,
{
	let worktree_manager =
		WorktreeManager::new(project.service_id(), project.repo_root(), project.worktree_root());
	let active_issue_ids = active_shared_issue_ids(project, state_store)?;
	let mut issue_ids = Vec::new();

	for mapping in state_store.list_worktrees(project.service_id())? {
		if worktree_mapping_is_stale_terminal_local_residue(
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
		refresh_recoverable_runtime_issues(tracker, &issue_ids)?
	};
	let mut known_identifiers =
		issues.iter().map(|issue| issue.identifier.to_ascii_uppercase()).collect::<BTreeSet<_>>();

	for issue_identifier in recoverable_worktree_identifiers(project.worktree_root())? {
		append_recoverable_tracker_issue(
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

pub(in crate::orchestrator) fn refresh_recoverable_runtime_issues<T>(
	tracker: &T,
	issue_ids: &[String],
) -> crate::prelude::Result<Vec<TrackerIssue>>
where
	T: IssueTracker,
{
	match tracker.refresh_issues(issue_ids) {
		Ok(issues) => Ok(issues),
		Err(error)
			if issue_ids.iter().any(|issue_id| {
				tracker::issue_lookup_missing_error_for_candidate(&error, issue_id)
			}) =>
		{
			let mut issues = Vec::new();

			for issue_id in issue_ids {
				match tracker.refresh_issues(slice::from_ref(issue_id)) {
					Ok(mut refreshed) => issues.append(&mut refreshed),
					Err(error)
						if tracker::issue_lookup_missing_error_for_candidate(&error, issue_id) => {},
					Err(error) => return Err(error),
				}
			}

			Ok(issues)
		},
		Err(error) => Err(error),
	}
}

pub(in crate::orchestrator) fn recover_issue_runtime_state<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	worktree_manager: &WorktreeManager,
	issue: TrackerIssue,
	now_unix_epoch: i64,
) -> crate::prelude::Result<Option<TrackerIssue>>
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
		upsert_recovered_worktree_mapping(
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
		&& worktree_activity_marker_is_fresh(marker, now_unix_epoch)
	{
		upsert_recovered_worktree_mapping(
			project,
			state_store,
			&issue,
			&worktree,
			activity_marker.as_ref(),
		)?;
		record_recovered_activity_lease(project, state_store, &issue, marker)?;

		return Ok(None);
	}
	if issue_passes_closeout_dispatch_policy(tracker, &issue, project, workflow, state_store)? {
		upsert_recovered_worktree_mapping(
			project,
			state_store,
			&issue,
			&worktree,
			activity_marker.as_ref(),
		)?;

		match activity_marker.as_ref() {
			Some(marker) if worktree_activity_marker_is_fresh(marker, now_unix_epoch) => {
				record_recovered_activity_lease(project, state_store, &issue, marker)?;

				return Ok(None);
			},
			_ => {},
		}
	}
	if issue_passes_retry_dispatch_policy(
		tracker,
		&issue,
		project,
		workflow,
		state_store,
		RetryIssueStateHint::default(),
	)? {
		upsert_recovered_worktree_mapping(
			project,
			state_store,
			&issue,
			&worktree,
			activity_marker.as_ref(),
		)?;

		match activity_marker.as_ref() {
			Some(marker) if worktree_activity_marker_is_fresh(marker, now_unix_epoch) => {
				record_recovered_activity_lease(project, state_store, &issue, marker)?;

				return Ok(None);
			},
			Some(marker) => {
				clear_recovered_issue_lease(
					project.service_id(),
					&issue.id,
					Some(marker.run_id()),
					state_store,
				)?;
			},
			None => {
				clear_recovered_issue_lease(project.service_id(), &issue.id, None, state_store)?;
			},
		}

		return Ok(Some(issue));
	}

	Ok(None)
}

pub(in crate::orchestrator) fn existing_recoverable_worktree_spec(
	project_id: &str,
	issue: &TrackerIssue,
	mapping: Option<&WorktreeMapping>,
) -> crate::prelude::Result<Option<WorktreeSpec>> {
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

pub(in crate::orchestrator) fn upsert_recovered_worktree_mapping(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue: &TrackerIssue,
	worktree: &WorktreeSpec,
	activity_marker: Option<&RunActivityMarker>,
) -> crate::prelude::Result<()> {
	state_store.upsert_recovered_worktree(
		project.service_id(),
		&issue.id,
		&worktree.branch_name,
		&worktree.path.display().to_string(),
		recovered_worktree_observed_at_unix(activity_marker),
	)
}

pub(in crate::orchestrator) fn recovered_worktree_observed_at_unix(
	activity_marker: Option<&RunActivityMarker>,
) -> Option<i64> {
	activity_marker.and_then(|marker| {
		[
			marker.last_activity_unix_epoch(),
			marker.last_protocol_activity_unix_epoch(),
			marker.last_progress_unix_epoch(),
		]
		.into_iter()
		.flatten()
		.max()
	})
}

pub(in crate::orchestrator) fn record_recovered_activity_lease(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue: &TrackerIssue,
	marker: &RunActivityMarker,
) -> crate::prelude::Result<()> {
	state_store.record_run_attempt(
		marker.run_id(),
		&issue.id,
		marker.attempt_number(),
		"running",
	)?;
	state_store.upsert_lease(
		project.service_id(),
		&issue.id,
		marker.run_id(),
		&issue.state.name,
	)?;

	Ok(())
}

pub(in crate::orchestrator) fn issue_has_recovered_service_ownership<T>(
	tracker: &T,
	issue: &TrackerIssue,
	service_id: &str,
) -> crate::prelude::Result<bool>
where
	T: IssueTracker,
{
	tracker::issue_has_label_with_server_confirmation(
		tracker,
		issue,
		&tracker::automation_active_label(service_id),
	)
}

pub(in crate::orchestrator) fn append_recoverable_tracker_issue<T>(
	tracker: &T,
	project: &ServiceConfig,
	issue_identifier: &str,
	known_identifiers: &mut BTreeSet<String>,
	issues: &mut Vec<TrackerIssue>,
	mut recoverable_worktree_skip_cache: Option<&mut RecoverableWorktreeSkipCache>,
) -> crate::prelude::Result<()>
where
	T: IssueTracker,
{
	let canonical_identifier = issue_identifier.to_ascii_uppercase();

	if known_identifiers.contains(&canonical_identifier) {
		return Ok(());
	}

	let now = Instant::now();

	if let Some(cache) = recoverable_worktree_skip_cache.as_deref_mut()
		&& cache.is_suppressed(&canonical_identifier, now)
	{
		tracing::debug!(
			issue = canonical_identifier,
			"Skipped retained worktree tracker lookup because a recent recovery probe already found no service ownership."
		);

		return Ok(());
	}

	let issue = match tracker.get_issue_by_identifier(issue_identifier) {
		Ok(issue) => issue,
		Err(error)
			if tracker::issue_lookup_missing_error_for_candidate(&error, issue_identifier) =>
			None,
		Err(error) => return Err(error),
	};
	let Some(issue) = issue else {
		if let Some(cache) = recoverable_worktree_skip_cache {
			cache.remember(&canonical_identifier, now);
		}

		return Ok(());
	};

	if !issue_has_recovered_service_ownership(tracker, &issue, project.service_id())? {
		tracing::warn!(
			issue = issue.identifier,
			active_label = tracker::automation_active_label(project.service_id()),
			labels_complete = issue.labels_complete,
			"Skipping retained worktree recovery because the tracker issue is not explicitly owned by this service."
		);

		if let Some(cache) = recoverable_worktree_skip_cache {
			cache.remember(&canonical_identifier, now);
		}

		return Ok(());
	}

	known_identifiers.insert(issue.identifier.to_ascii_uppercase());
	issues.push(issue);

	Ok(())
}

pub(in crate::orchestrator) fn recoverable_worktree_identifiers(
	worktree_root: &Path,
) -> crate::prelude::Result<Vec<String>> {
	if !worktree_root.exists() {
		return Ok(Vec::new());
	}

	let mut issue_identifiers = fs::read_dir(worktree_root)?
		.filter_map(|entry| entry.ok())
		.filter_map(|entry| {
			entry
				.file_type()
				.ok()
				.filter(|file_type| file_type.is_dir())
				.and_then(|_| entry.file_name().into_string().ok())
		})
		.filter(|name| commit_message::looks_like_issue_identifier(name))
		.collect::<Vec<_>>();

	issue_identifiers.sort();
	issue_identifiers.dedup();

	Ok(issue_identifiers)
}

pub(in crate::orchestrator) fn hydrate_status_snapshot_state(
	_project: &ServiceConfig,
	_state_store: &StateStore,
	_recovered_state: RecoveredRuntimeState,
) -> crate::prelude::Result<()> {
	Ok(())
}

pub(in crate::orchestrator) fn append_primary_account_if_missing(
	accounts: &mut Vec<CodexAccountActivitySummary>,
	account: Option<&CodexAccountActivitySummary>,
) {
	if accounts.is_empty()
		&& let Some(account) = account
	{
		accounts.push(account.clone());
	}
}
