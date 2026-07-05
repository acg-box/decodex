use crate::{
	prelude::Result,
	pull_request::PullRequestLandingState,
	recovery::{
		context::RecoveryContext, pull_request_inspection, review_handoff_policy::RebindMode,
	},
	state::{RunAttempt, WorktreeMapping},
	tracker::{TrackerIssue, records::LinearExecutionEventRecord},
};

const REVIEW_HANDOFF_WRITEBACK_FAILED: &str = "review_handoff_writeback_failed";

pub(in crate::recovery::review_handoff::issue) fn missing_handoff_rebind_mode(
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
