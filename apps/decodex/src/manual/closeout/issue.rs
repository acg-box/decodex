use std::path::Path;

use crate::{
	config::ServiceConfig,
	prelude::{Result, eyre},
	tracker::{self, IssueTracker, TrackerIssue, linear::LinearClient},
	workflow::WorkflowDocument,
};

use crate::manual::closeout::{ledger, marker};
use crate::manual::{ManualLandLedgerContext, PreparedCloseout};

pub(in crate::manual) fn prepare_closeout(
	config: &ServiceConfig,
	workflow: WorkflowDocument,
	authority: &str,
) -> Result<PreparedCloseout> {
	let tracker_policy = workflow.frontmatter().tracker();
	let completed_state = tracker_policy.resolved_completed_state().to_owned();
	let needs_attention_label = tracker_policy.needs_attention_label().to_owned();
	let tracker = LinearClient::new(config.tracker().resolve_api_key()?)?;
	let issue = tracker
		.get_issue_by_identifier(&authority.to_ascii_uppercase())?
		.ok_or_else(|| eyre::eyre!("Tracker does not contain issue `{authority}`."))?;

	ensure_manual_closeout_issue_scope(&tracker, &issue, config.service_id())?;

	Ok(PreparedCloseout {
		tracker,
		issue,
		completed_state,
		service_id: config.service_id().to_owned(),
		needs_attention_label,
	})
}

pub(in crate::manual) fn ensure_manual_closeout_issue_scope<T>(
	tracker: &T,
	issue: &TrackerIssue,
	service_id: &str,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	let active_label = tracker::automation_active_label(service_id);

	if tracker::issue_has_label_with_server_confirmation(tracker, issue, &active_label)? {
		return Ok(());
	}

	eyre::bail!(
		"Issue `{}` is not owned by service `{service_id}`; `decodex land` requires label `{active_label}`.",
		issue.identifier
	);
}

pub(in crate::manual) fn apply_closeout<T>(
	checkout_root: &Path,
	tracker: &T,
	completed_state: &str,
	ledger: &ManualLandLedgerContext<'_>,
	landed_change_record: &str,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	if ledger.issue.state.name != completed_state {
		let state_id = ledger.issue.state_id_for_name(completed_state).ok_or_else(|| {
			eyre::eyre!(
				"Issue `{}` does not expose tracker state `{}` on its team.",
				ledger.issue.identifier,
				completed_state
			)
		})?;

		tracker.update_issue_state(ledger.issue.id.as_str(), state_id)?;
	}
	if !marker::manual_land_closeout_matches(
		checkout_root,
		ledger.pr_url,
		ledger.merge_commit,
		ledger.branch_name,
		landed_change_record,
	)? {
		tracker::create_public_comment(
			tracker,
			ledger.issue.id.as_str(),
			format!(
				"decodex land completed\n\n- pr_url: `{}`\n- merge_commit: `{}`\n- branch: `{}`\n- landed_change: `{landed_change_record}`",
				ledger.pr_url, ledger.merge_commit, ledger.branch_name
			)
			.as_str(),
		)?;

		marker::write_manual_land_closeout_marker(
			checkout_root,
			ledger.pr_url,
			ledger.merge_commit,
			ledger.branch_name,
			landed_change_record,
		)?;
	}

	ledger::write_manual_land_landed_and_closeout_events(tracker, ledger)?;
	ledger::succeed_manual_land_handoff_attempt(
		ledger.state_store,
		&ledger.issue.id,
		ledger.handoff.run_id(),
	)?;

	Ok(())
}
