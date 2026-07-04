use crate::{
	prelude::{Result, eyre},
	pull_request::PullRequestLandingState,
	recovery::{
		context::RecoveryContext,
		pull_request_inspection,
		review_handoff_policy::{self, RebindMode, RebindSuccessStateTransition},
	},
	state::{ReviewHandoffMarker, ReviewOrchestrationMarker, RunAttempt, WorktreeMapping},
	tracker::{IssueTracker, TrackerIssue, records::LinearExecutionEventRecord},
	workflow::WorkflowTracker,
};

const REVIEW_HANDOFF_WRITEBACK_FAILED: &str = "review_handoff_writeback_failed";

pub(in crate::recovery) fn load_issue_by_identifier<T>(
	tracker: &T,
	issue_identifier: &str,
) -> Result<TrackerIssue>
where
	T: IssueTracker + ?Sized,
{
	tracker
		.get_issue_by_identifier(issue_identifier)?
		.ok_or_else(|| eyre::eyre!("Tracker issue `{issue_identifier}` was not found."))
}

pub(in crate::recovery) fn validate_existing_handoff_refresh(
	tracker_policy: &WorkflowTracker,
	issue: &TrackerIssue,
	worktree: &WorktreeMapping,
	existing_handoff: &ReviewHandoffMarker,
	existing_orchestration: Option<&ReviewOrchestrationMarker>,
	landing_state: &PullRequestLandingState,
	local_head_oid: &str,
) -> Result<(String, i64, RebindMode)> {
	if existing_handoff.pr_url() != pull_request_inspection::landing_url(landing_state) {
		eyre::bail!(
			"Issue `{}` already has a review lifecycle record for branch `{}` and PR `{}`; refusing to rebind it to `{}`.",
			issue.identifier,
			worktree.branch_name(),
			existing_handoff.pr_url(),
			pull_request_inspection::landing_url(landing_state)
		);
	}

	let orchestration_is_current = existing_orchestration.is_none_or(|marker| {
		marker.branch_name() == worktree.branch_name()
			&& marker.pr_url() == pull_request_inspection::landing_url(landing_state)
			&& marker.head_sha() == local_head_oid
	});

	if existing_handoff.pr_head_oid() == local_head_oid && orchestration_is_current {
		if issue.state.name == tracker_policy.in_progress_state()
			|| issue.state.name == tracker_policy.failure_state()
		{
			return Ok((
				existing_handoff.run_id().to_owned(),
				existing_handoff.attempt_number(),
				RebindMode::CompleteExistingHandoffState,
			));
		}

		eyre::bail!(
			"Issue `{}` already has a review lifecycle record for branch `{}` and PR `{}` at head `{local_head_oid}`; no rebind is needed.",
			issue.identifier,
			worktree.branch_name(),
			existing_handoff.pr_url()
		);
	}

	Ok((
		existing_handoff.run_id().to_owned(),
		existing_handoff.attempt_number(),
		RebindMode::RefreshExistingHandoff,
	))
}

pub(in crate::recovery) fn validate_rebind_existing_handoff(
	context: &RecoveryContext,
	issue: &TrackerIssue,
	worktree: &WorktreeMapping,
	existing_handoff: Option<&ReviewHandoffMarker>,
	existing_orchestration: Option<&ReviewOrchestrationMarker>,
	landing_state: &PullRequestLandingState,
	local_head_oid: &str,
) -> Result<(String, i64, RebindMode)> {
	let Some(existing_handoff) = existing_handoff else {
		let attempt =
			context.state_store.latest_run_attempt_for_issue(&issue.id)?.ok_or_else(|| {
				eyre::eyre!("Issue `{}` has no recorded run attempt to rebind.", issue.identifier)
			})?;

		return Ok((
			attempt.run_id().to_owned(),
			attempt.attempt_number(),
			missing_handoff_rebind_mode(
				context,
				issue,
				worktree,
				landing_state,
				local_head_oid,
				&attempt,
			)?,
		));
	};

	validate_existing_handoff_refresh(
		context.workflow.frontmatter().tracker(),
		issue,
		worktree,
		existing_handoff,
		existing_orchestration,
		landing_state,
		local_head_oid,
	)
}

pub(super) fn validate_rebind_issue_context(
	context: &RecoveryContext,
	issue: &TrackerIssue,
) -> Result<WorktreeMapping> {
	let tracker_policy = context.workflow.frontmatter().tracker();

	if issue.has_label(tracker_policy.opt_out_label()) {
		eyre::bail!(
			"Issue `{}` has opt-out label `{}`.",
			issue.identifier,
			tracker_policy.opt_out_label()
		);
	}

	let worktree = context.state_store.worktree_for_issue(&issue.id)?.ok_or_else(|| {
		eyre::eyre!("Issue `{}` has no retained worktree mapping.", issue.identifier)
	})?;

	Ok(worktree)
}

pub(super) fn validate_rebind_issue_state(
	context: &RecoveryContext,
	issue: &TrackerIssue,
	mode: RebindMode,
) -> Result<Option<RebindSuccessStateTransition>> {
	review_handoff_policy::validate_rebind_issue_state_for_policy(
		context.workflow.frontmatter().tracker(),
		issue,
		mode,
	)
}

fn missing_handoff_rebind_mode(
	context: &RecoveryContext,
	issue: &TrackerIssue,
	worktree: &WorktreeMapping,
	landing_state: &PullRequestLandingState,
	local_head_oid: &str,
	attempt: &RunAttempt,
) -> Result<RebindMode> {
	if issue.state.name != context.workflow.frontmatter().tracker().failure_state() {
		return Ok(RebindMode::RestoreMissingHandoff);
	}

	let events =
		context.state_store.list_linear_execution_events(context.config.service_id(), &issue.id)?;
	let latest_lifecycle_outcome = events.iter().rev().find(|event| {
		matches!(
			event.event_type.as_str(),
			"cleanup_complete"
				| "closeout" | "needs_attention"
				| "terminal_failure"
				| "landed" | "review_handoff"
				| "repair_handoff"
		)
	});

	if latest_lifecycle_outcome.is_some_and(|event| {
		event.run_id == attempt.run_id()
			&& event.attempt_number == attempt.attempt_number()
			&& event_matches_rebind_target(event, worktree, landing_state, local_head_oid)
			&& event_proves_review_handoff_writeback_failure(event)
	}) {
		Ok(RebindMode::RestoreMissingHandoffAfterWritebackFailure)
	} else {
		Ok(RebindMode::RestoreMissingHandoff)
	}
}

fn event_matches_rebind_target(
	event: &LinearExecutionEventRecord,
	worktree: &WorktreeMapping,
	landing_state: &PullRequestLandingState,
	local_head_oid: &str,
) -> bool {
	event.branch.as_deref() == Some(worktree.branch_name())
		&& event.pr_url.as_deref() == Some(pull_request_inspection::landing_url(landing_state))
		&& event.pr_head_sha.as_deref().is_none_or(|head_sha| head_sha == local_head_oid)
}

fn event_proves_review_handoff_writeback_failure(event: &LinearExecutionEventRecord) -> bool {
	if event.error_class.as_deref() == Some(REVIEW_HANDOFF_WRITEBACK_FAILED) {
		return true;
	}

	let has_writeback_failure_text = event
		.evidence
		.as_deref()
		.unwrap_or_default()
		.iter()
		.chain(event.blockers.as_deref().unwrap_or_default())
		.any(|value| {
			value.contains(REVIEW_HANDOFF_WRITEBACK_FAILED)
				|| value.contains("review handoff writeback")
		});

	event.event_type == "needs_attention" && has_writeback_failure_text
}
