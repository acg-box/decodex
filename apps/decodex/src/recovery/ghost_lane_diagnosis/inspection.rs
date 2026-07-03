mod control;
mod outcome;
mod private;
mod review_lineage;
mod tracker_issue;
mod worktree;

use std::path::Path;

use crate::{
	prelude::Result,
	recovery::{self, identifiers, reports::GhostLaneDiagnostic},
	state::{ProjectRunStatus, StateStore},
	tracker::IssueTracker,
};

pub(super) fn inspect_ghost_lane<T>(
	project_id: &str,
	worktree_root: &Path,
	state_store: &StateStore,
	tracker: &T,
	run: &ProjectRunStatus,
	requested_selector: Option<&str>,
) -> Result<GhostLaneDiagnostic>
where
	T: IssueTracker + ?Sized,
{
	let issue_identifier = identifiers::ghost_lane_issue_identifier(run, requested_selector);
	let mcp_test_fixture = private::ghost_lane_mcp_test_fixture_control_evidence(
		project_id,
		state_store,
		run,
		issue_identifier.as_deref(),
	)?;
	let mut evidence = Vec::new();
	let mut blockers = Vec::new();

	if run.run_lease() {
		evidence.push(String::from("run_lease_present"));
	} else {
		blockers.push(String::from("run_lease_missing"));
	}

	tracker_issue::inspect_ghost_lane_tracker_issue(
		tracker,
		run,
		issue_identifier.as_deref(),
		requested_selector,
		&mut evidence,
		&mut blockers,
	)?;
	worktree::inspect_ghost_lane_worktree(
		worktree_root,
		state_store,
		run,
		issue_identifier.as_deref(),
		requested_selector,
		&mut evidence,
		&mut blockers,
	)?;

	let control_channel = control::inspect_ghost_lane_control_channel(
		run,
		mcp_test_fixture,
		&mut evidence,
		&mut blockers,
	);

	control::inspect_ghost_lane_live_evidence(run, mcp_test_fixture, &mut evidence, &mut blockers);
	private::inspect_ghost_lane_private_evidence(
		project_id,
		state_store,
		run,
		mcp_test_fixture,
		&mut evidence,
		&mut blockers,
	)?;
	review_lineage::inspect_ghost_lane_review_lineage(
		project_id,
		state_store,
		run,
		issue_identifier.as_deref(),
		&mut evidence,
		&mut blockers,
	)?;

	let (classification, reason, next_action) = outcome::ghost_lane_diagnostic_outcome(
		run,
		issue_identifier.as_deref(),
		mcp_test_fixture,
		&blockers,
	);

	Ok(GhostLaneDiagnostic {
		project_id: project_id.to_owned(),
		issue_id: run.issue_id().to_owned(),
		issue_identifier,
		run_id: run.run_id().to_owned(),
		attempt_number: run.attempt_number(),
		attempt_status: run.status().to_owned(),
		classification,
		reason,
		run_lease: run.run_lease(),
		control_channel,
		evidence: recovery::sorted_unique(evidence),
		blockers: recovery::sorted_unique(blockers),
		next_action,
	})
}
