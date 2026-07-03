use color_eyre::{Report, eyre::WrapErr};
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	manual::ManualLandLedgerContext,
	prelude::{Result, eyre},
	state::StateStore,
	tracker::{
		self, IssueTracker, TrackerIssue,
		records::{self, LinearExecutionEventIdentity, LinearExecutionEventRecord},
	},
};

pub(super) fn write_manual_land_landed_and_closeout_events<T>(
	tracker: &T,
	ledger: &ManualLandLedgerContext<'_>,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	let landed = manual_land_landed_event(ledger);
	let closeout = manual_land_closeout_event(ledger);

	write_manual_land_lifecycle_event(tracker, ledger, &landed)?;

	write_manual_land_lifecycle_event(tracker, ledger, &closeout)
}

pub(super) fn write_manual_land_lifecycle_event<T>(
	tracker: &T,
	ledger: &ManualLandLedgerContext<'_>,
	record: &LinearExecutionEventRecord,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	let retry_budget_attempt_count =
		ledger.state_store.retry_budget_attempt_count(&ledger.issue.id)?;
	let retry_budget_attempt_count =
		(retry_budget_attempt_count > 0).then_some(retry_budget_attempt_count);
	let body =
		records::render_linear_execution_event_comment_body(record, retry_budget_attempt_count);
	let projection =
		tracker::prepare_linear_execution_event_comment(&body, record, ledger.privacy_classifier)?;

	if ledger.state_store.record_linear_execution_event(&projection.record)?
		&& let Err(error) =
			tracker::create_prepared_linear_execution_event_comment_without_remote_scan(
				tracker,
				&ledger.issue.id,
				&projection,
			) {
		ledger.state_store.forget_linear_execution_event(&projection.record.idempotency_key)?;

		return Err(error);
	}

	Ok(())
}

pub(super) fn manual_land_landed_event(
	ledger: &ManualLandLedgerContext<'_>,
) -> LinearExecutionEventRecord {
	let anchor = records::stable_event_anchor(&[
		ledger.pr_url,
		ledger.handoff.pr_head_oid(),
		ledger.merge_commit,
		"manual_land_landed",
	]);
	let mut record = LinearExecutionEventRecord::new(
		manual_land_lifecycle_identity(ledger),
		"landed",
		manual_land_ordered_event_timestamp(-2),
		&anchor,
	);

	record.branch = Some(ledger.branch_name.to_owned());
	record.pr_url = Some(ledger.pr_url.to_owned());
	record.pr_head_sha = Some(ledger.handoff.pr_head_oid().to_owned());
	record.pr_base_ref =
		Some(ledger.handoff.target_base_ref_name().unwrap_or(ledger.default_branch).to_owned());
	record.commit_sha = Some(ledger.merge_commit.to_owned());
	record.summary =
		Some(format!("Manual land merged {} for {}.", ledger.pr_url, ledger.issue.identifier));

	record
}

pub(super) fn manual_land_closeout_event(
	ledger: &ManualLandLedgerContext<'_>,
) -> LinearExecutionEventRecord {
	let anchor =
		records::stable_event_anchor(&[ledger.pr_url, ledger.merge_commit, "manual_land_closeout"]);
	let mut record = LinearExecutionEventRecord::new(
		manual_land_lifecycle_identity(ledger),
		"closeout",
		manual_land_ordered_event_timestamp(-1),
		&anchor,
	);

	record.branch = Some(ledger.branch_name.to_owned());
	record.worktree_path = Some(ledger.worktree_path.to_owned());
	record.pr_url = Some(ledger.pr_url.to_owned());
	record.commit_sha = Some(ledger.merge_commit.to_owned());
	record.validation_result = Some(String::from("passed"));
	record.target_state = Some(ledger.completed_state.to_owned());
	record.summary = Some(format!(
		"Manual land closed out {} after merge {}.",
		ledger.issue.identifier, ledger.merge_commit
	));

	record
}

pub(super) fn manual_land_cleanup_complete_event(
	ledger: &ManualLandLedgerContext<'_>,
) -> LinearExecutionEventRecord {
	let anchor = records::stable_event_anchor(&[
		ledger.branch_name,
		ledger.merge_commit,
		"manual_land_cleanup_complete",
	]);
	let mut record = LinearExecutionEventRecord::new(
		manual_land_lifecycle_identity(ledger),
		"cleanup_complete",
		manual_land_ordered_event_timestamp(0),
		&anchor,
	);

	record.branch = Some(ledger.branch_name.to_owned());
	record.worktree_path = Some(ledger.worktree_path.to_owned());
	record.cleanup_status = Some(String::from("completed"));
	record.summary = Some(String::from("Manual land cleaned up the retained lane."));
	record.pr_url = Some(ledger.pr_url.to_owned());
	record.commit_sha = Some(ledger.merge_commit.to_owned());

	record
}

pub(super) fn manual_land_lifecycle_identity<'a>(
	ledger: &'a ManualLandLedgerContext<'_>,
) -> LinearExecutionEventIdentity<'a> {
	LinearExecutionEventIdentity {
		service_id: ledger.service_id,
		issue_id: &ledger.issue.id,
		issue_identifier: &ledger.issue.identifier,
		run_id: ledger.handoff.run_id(),
		attempt_number: ledger.handoff.attempt_number(),
	}
}

pub(super) fn manual_land_ordered_event_timestamp(offset_seconds: i64) -> String {
	(OffsetDateTime::now_utc() + Duration::seconds(offset_seconds))
		.format(&Rfc3339)
		.expect("timestamp formatting should succeed")
}

pub(super) fn clear_manual_closeout_issue_label<T>(
	tracker: &T,
	issue: &TrackerIssue,
	label_name: &str,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	if let Err(error) = tracker::set_issue_label_presence(tracker, issue, label_name, false)
		&& !linear_label_not_on_issue_error(&error)
	{
		return Err(error);
	}

	Ok(())
}

pub(super) fn succeed_manual_land_handoff_attempt(
	state_store: &StateStore,
	issue_id: &str,
	handoff_run_id: &str,
) -> Result<()> {
	let Some(attempt) = state_store.run_attempt(handoff_run_id)? else {
		return Ok(());
	};

	if attempt.issue_id() != issue_id {
		eyre::bail!(
			"Manual land handoff run `{handoff_run_id}` belongs to issue `{}`, not `{issue_id}`.",
			attempt.issue_id()
		);
	}
	if attempt.status() != "succeeded" {
		state_store.update_run_status(handoff_run_id, "succeeded")?;
	}

	Ok(())
}

pub(super) fn linear_label_not_on_issue_error(error: &Report) -> bool {
	error
		.chain()
		.any(|source| source.to_string().to_ascii_lowercase().contains("label not on issue"))
}

pub(in crate::manual) fn write_manual_land_cleanup_complete_event<T>(
	tracker: &T,
	ledger: &ManualLandLedgerContext<'_>,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	let cleanup_complete = manual_land_cleanup_complete_event(ledger);

	write_manual_land_lifecycle_event(tracker, ledger, &cleanup_complete)
}

pub(in crate::manual) fn clear_manual_closeout_issue_scope<T>(
	tracker: &T,
	issue: &TrackerIssue,
	service_id: &str,
	needs_attention_label: &str,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	let closeout_labels = [
		tracker::automation_active_label(service_id),
		tracker::automation_queue_label(service_id),
		needs_attention_label.to_owned(),
	];

	for label_name in closeout_labels {
		clear_manual_closeout_issue_label(tracker, issue, &label_name)?;
	}

	Ok(())
}

pub(in crate::manual) fn clear_manual_closeout_runtime_state(
	state_store: &StateStore,
	issue_id: &str,
	handoff_run_id: &str,
) -> Result<()> {
	state_store.succeed_running_run_attempts_for_issue(issue_id).wrap_err_with(|| {
		format!("Failed to finalize running runtime attempts for issue `{issue_id}`.")
	})?;

	succeed_manual_land_handoff_attempt(state_store, issue_id, handoff_run_id)?;

	state_store
		.clear_lease(issue_id)
		.wrap_err_with(|| format!("Failed to clear runtime lease for issue `{issue_id}`."))?;
	state_store.clear_worktree(issue_id).wrap_err_with(|| {
		format!("Failed to clear runtime worktree state for issue `{issue_id}`.")
	})?;

	Ok(())
}
