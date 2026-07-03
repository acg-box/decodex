use crate::{
	orchestrator,
	orchestrator::retained_review_orchestration::{
		IssueDispatchMode, IssueRunPlan, IssueTracker, PassiveRetainedAttentionRuntime, Report,
		Result, RetainedReviewNeedsAttention, RetainedReviewRunIdentity, RetainedReviewRuntime,
		ReviewOrchestrationMarker, TerminalFailureWritebackRuntime, TrackerIssue, WorktreeMapping,
		WorktreeSpec,
	},
};

pub(crate) fn apply_passive_retained_manual_attention<T>(
	runtime: PassiveRetainedAttentionRuntime<'_, T>,
	issue: &TrackerIssue,
	worktree: &WorktreeMapping,
	orchestration_marker: &ReviewOrchestrationMarker,
	reason: &str,
) -> Result<()>
where
	T: IssueTracker,
{
	apply_passive_retained_manual_attention_with_run_identity(
		runtime,
		issue,
		worktree,
		&RetainedReviewRunIdentity {
			run_id: orchestration_marker.run_id().to_owned(),
			attempt_number: orchestration_marker.attempt_number(),
		},
		reason,
	)
}

pub(crate) fn apply_passive_retained_manual_attention_with_run_identity<T>(
	runtime: PassiveRetainedAttentionRuntime<'_, T>,
	issue: &TrackerIssue,
	worktree: &WorktreeMapping,
	run_identity: &RetainedReviewRunIdentity,
	reason: &str,
) -> Result<()>
where
	T: IssueTracker,
{
	if passive_retained_attention_blocker_was_resolved(&runtime, issue, worktree, reason)? {
		return Ok(());
	}

	let synthetic_issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: issue.state.name.clone(),
		initial_issue_state: issue.state.name.clone(),
		worktree: WorktreeSpec {
			branch_name: worktree.branch_name().to_owned(),
			issue_identifier: issue.identifier.clone(),
			path: worktree.worktree_path().to_path_buf(),
			reused_existing: true,
		},
		#[cfg(test)]
		retry_project_slug: String::new(),
		dispatch_mode: IssueDispatchMode::ReviewRepair,
		attempt_number: run_identity.attempt_number,
		run_id: run_identity.run_id.clone(),
		retry_budget_base: 0,
	};
	let worktree_path = orchestrator::relative_worktree_path_for_path(
		runtime.project,
		synthetic_issue_run.worktree.path.as_path(),
	);
	let privacy_classifier =
		orchestrator::configured_public_projection_privacy_classifier(runtime.project)?;
	let _ = orchestrator::apply_terminal_failure_writeback(
		runtime.tracker,
		TerminalFailureWritebackRuntime {
			service_id: runtime.project.service_id(),
			state_store: Some(runtime.state_store),
			privacy_classifier: &privacy_classifier,
		},
		runtime.workflow,
		&synthetic_issue_run,
		&worktree_path,
		true,
		&Report::new(RetainedReviewNeedsAttention { reason: reason.to_owned() }),
	)?;

	Ok(())
}

pub(super) fn passive_attention_runtime<'a, T>(
	runtime: &'a RetainedReviewRuntime<'_, T>,
) -> PassiveRetainedAttentionRuntime<'a, T> {
	PassiveRetainedAttentionRuntime {
		tracker: runtime.tracker,
		project: runtime.project,
		workflow: runtime.workflow,
		state_store: runtime.state_store,
	}
}

fn passive_retained_attention_blocker_was_resolved<T>(
	runtime: &PassiveRetainedAttentionRuntime<'_, T>,
	issue: &TrackerIssue,
	worktree: &WorktreeMapping,
	reason: &str,
) -> Result<bool>
where
	T: IssueTracker,
{
	if reason != "missing_review_handoff_record" {
		return Ok(false);
	}

	let Some(review_handoff) = runtime.state_store.review_handoff_marker(
		runtime.project.service_id(),
		&issue.id,
		worktree.branch_name(),
	)?
	else {
		return Ok(false);
	};

	tracing::info!(
		service_id = runtime.project.service_id(),
		issue_id = issue.id.as_str(),
		issue = issue.identifier.as_str(),
		branch = worktree.branch_name(),
		pr_url = review_handoff.pr_url(),
		pr_head_sha = review_handoff.pr_head_oid(),
		"Skipping stale retained review attention writeback because review handoff is now rebound."
	);

	Ok(true)
}
