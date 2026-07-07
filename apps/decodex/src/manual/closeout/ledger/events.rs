use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	manual::ManualLandLedgerContext,
	prelude::Result,
	tracker::{
		self, IssueTracker,
		records::{self, LinearExecutionEventIdentity, LinearExecutionEventRecord},
	},
};

pub(in crate::manual) fn write_manual_land_landed_and_closeout_events<T>(
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

pub(in crate::manual::closeout::ledger) fn write_manual_land_lifecycle_event<T>(
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

fn manual_land_landed_event(ledger: &ManualLandLedgerContext<'_>) -> LinearExecutionEventRecord {
	let anchor = records::stable_event_anchor(&[
		ledger.pr_url,
		ledger.lifecycle_record.pr_head_oid(),
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
	record.pr_head_sha = Some(ledger.lifecycle_record.pr_head_oid().to_owned());
	record.pr_base_ref = Some(
		ledger.lifecycle_record.target_base_ref_name().unwrap_or(ledger.default_branch).to_owned(),
	);
	record.commit_sha = Some(ledger.merge_commit.to_owned());
	record.summary =
		Some(format!("Manual land merged {} for {}.", ledger.pr_url, ledger.issue.identifier));

	record
}

fn manual_land_closeout_event(ledger: &ManualLandLedgerContext<'_>) -> LinearExecutionEventRecord {
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

fn manual_land_cleanup_complete_event(
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

fn manual_land_lifecycle_identity<'a>(
	ledger: &'a ManualLandLedgerContext<'_>,
) -> LinearExecutionEventIdentity<'a> {
	LinearExecutionEventIdentity {
		service_id: ledger.service_id,
		issue_id: &ledger.issue.id,
		issue_identifier: &ledger.issue.identifier,
		run_id: ledger.lifecycle_record.run_id(),
		attempt_number: ledger.lifecycle_record.attempt_number(),
	}
}

fn manual_land_ordered_event_timestamp(offset_seconds: i64) -> String {
	(OffsetDateTime::now_utc() + Duration::seconds(offset_seconds))
		.format(&Rfc3339)
		.expect("timestamp formatting should succeed")
}
