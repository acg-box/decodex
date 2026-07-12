use crate::orchestrator::daemon::{self, Child, ChildRunRef, Result, RunAttempt, StateStore};

pub(crate) fn clear_orphaned_daemon_child_state(
	state_store: &StateStore,
	child: ChildRunRef<'_>,
	mark_interrupted: bool,
) -> Result<()> {
	let resolved_run_attempt = resolve_child_exit_run_attempt(state_store, child)?;

	if resolved_run_attempt.is_none() {
		tracing::debug!(
			issue_id = child.issue_id,
			run_id = child.run_id,
			attempt = child.attempt_number,
			"Daemon child exited without a matching recorded run attempt; skipping orphan cleanup."
		);
	}
	if mark_interrupted && let Some(run_attempt) = resolved_run_attempt.as_ref() {
		daemon::mark_run_attempt_if_active(state_store, run_attempt.run_id(), "interrupted")?;
	}

	let existing_claim = resolved_run_attempt
		.as_ref()
		.and_then(|attempt| attempt.project_id())
		.map(|project_id| state_store.claim_for_lane(project_id, child.issue_id))
		.transpose()?
		.flatten();
	let issue_unowned_or_matches_run =
		existing_claim.as_ref().is_none_or(|claim| claim.run_id() == child.run_id);

	if let Some(claim) = existing_claim
		&& issue_unowned_or_matches_run
	{
		state_store.release_lane_claim(
			claim.id().project_key(),
			child.issue_id,
			claim.run_id(),
		)?;
	}
	if resolved_run_attempt.is_some()
		&& issue_unowned_or_matches_run
		&& let Some(mapping) = state_store.worktree_for_issue(child.issue_id)?
		&& !mapping.worktree_path().try_exists()?
	{
		tracing::debug!(
			issue_id = child.issue_id,
			run_id = child.run_id,
			attempt = child.attempt_number,
			branch = mapping.branch_name(),
			worktree_path = %mapping.worktree_path().display(),
			"Cleared daemon child worktree mapping after the checkout was removed."
		);

		state_store.clear_worktree(child.issue_id)?;
	}

	Ok(())
}

pub(crate) fn resolve_child_exit_run_attempt(
	state_store: &StateStore,
	child: ChildRunRef<'_>,
) -> Result<Option<RunAttempt>> {
	state_store.run_attempt(child.run_id)
}

pub(crate) fn stop_daemon_child(child: &mut Child) -> Result<()> {
	if child.try_wait()?.is_some() {
		return Ok(());
	}

	let _ = child.kill();
	let _ = child.wait();

	Ok(())
}
