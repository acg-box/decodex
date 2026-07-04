use crate::{
	prelude::{Result, eyre},
	recovery::{closeout::MergedCloseoutRetainedContext, context::RecoveryContext, review_handoff},
	state::WorktreeMapping,
	tracker::{
		self, IssueTracker, TrackerIssue,
		records::{self, LinearExecutionEventRecord},
	},
};

pub(in crate::recovery) fn validate_merged_closeout_issue_context(
	context: &RecoveryContext,
	issue: &TrackerIssue,
) -> Result<()> {
	let tracker_policy = context.workflow.frontmatter().tracker();
	let completed_state = tracker_policy.resolved_completed_state();

	if issue.state.name != completed_state {
		eyre::bail!(
			"Issue `{}` is in `{}`, but merged closeout recovery requires `{completed_state}`.",
			issue.identifier,
			issue.state.name
		);
	}
	if issue.has_label(tracker_policy.opt_out_label()) {
		eyre::bail!(
			"Issue `{}` has opt-out label `{}`.",
			issue.identifier,
			tracker_policy.opt_out_label()
		);
	}

	for label in [
		tracker::automation_queue_label(context.config.service_id()),
		tracker::automation_active_label(context.config.service_id()),
		tracker_policy.needs_attention_label().to_owned(),
	] {
		if tracker::issue_has_label_with_server_confirmation(&context.tracker, issue, &label)? {
			eyre::bail!(
				"Issue `{}` still has Linear label `{label}`; merged closeout recovery requires queue, active, and needs-attention labels to be absent.",
				issue.identifier
			);
		}
	}

	Ok(())
}

pub(in crate::recovery) fn retained_worktree_mapping_for_issue(
	context: &RecoveryContext,
	issue: &TrackerIssue,
) -> Result<Option<WorktreeMapping>> {
	if let Some(worktree) = context.state_store.worktree_for_issue(&issue.id)? {
		return Ok(Some(worktree));
	}

	context.state_store.worktree_for_issue(&issue.identifier)
}

pub(in crate::recovery) fn merged_closeout_retained_context(
	context: &RecoveryContext,
	issue: &TrackerIssue,
	worktree_mapping: Option<&WorktreeMapping>,
) -> Result<MergedCloseoutRetainedContext> {
	let latest_record = latest_merged_closeout_source_record(context, issue)?;
	let branch_name = worktree_mapping
		.map(|mapping| mapping.branch_name().to_owned())
		.or_else(|| latest_record.as_ref().and_then(|record| record.branch.clone()))
		.ok_or_else(|| {
			eyre::eyre!(
				"Issue `{}` has no retained branch in runtime state or execution ledger.",
				issue.identifier
			)
		})?;
	let worktree_path = worktree_mapping
		.and_then(|mapping| {
			review_handoff::relative_worktree_path_for_recovery(context, mapping.worktree_path())
		})
		.or_else(|| latest_record.as_ref().and_then(|record| record.worktree_path.clone()))
		.unwrap_or_else(|| format!(".worktrees/{}", issue.identifier));
	let (run_id, attempt_number) = if let Some(record) = latest_record
		.as_ref()
		.filter(|record| !record.run_id.trim().is_empty() && record.attempt_number >= 1)
	{
		(record.run_id.clone(), record.attempt_number)
	} else if let Some(attempt) = context.state_store.latest_run_attempt_for_issue(&issue.id)? {
		(attempt.run_id().to_owned(), attempt.attempt_number())
	} else {
		(format!("merged-closeout-{}", issue.identifier.to_ascii_lowercase()), 1)
	};

	Ok(MergedCloseoutRetainedContext { branch_name, worktree_path, run_id, attempt_number })
}

pub(in crate::recovery) fn latest_merged_closeout_source_record(
	context: &RecoveryContext,
	issue: &TrackerIssue,
) -> Result<Option<LinearExecutionEventRecord>> {
	let mut records =
		context.state_store.list_linear_execution_events(context.config.service_id(), &issue.id)?;

	if issue.identifier != issue.id {
		records.extend(
			context
				.state_store
				.list_linear_execution_events(context.config.service_id(), &issue.identifier)?,
		);
	}

	let comments = context.tracker.list_comments(&issue.id)?;

	records.extend(
		comments
			.iter()
			.filter_map(|comment| records::parse_linear_execution_event_record(&comment.body))
			.filter(|record| {
				record.service_id == context.config.service_id()
					&& (record.issue_id == issue.id || record.issue_identifier == issue.identifier)
			}),
	);

	Ok(records
		.into_iter()
		.filter(|record| record.branch.as_ref().is_some_and(|branch| !branch.trim().is_empty()))
		.max_by(|left, right| {
			left.event_timestamp
				.cmp(&right.event_timestamp)
				.then_with(|| left.idempotency_key.cmp(&right.idempotency_key))
		}))
}
