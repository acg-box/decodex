use std::path::Path;

use crate::{
	prelude::Result,
	recovery::{
		self, GHOST_LANE_BLOCKED_CLASSIFICATION, GHOST_LANE_CLASSIFICATION,
		MCP_TEST_FIXTURE_GHOST_LANE_CLASSIFICATION, evidence, identifiers,
		reports::GhostLaneDiagnostic,
	},
	state::{ProjectRunStatus, StateStore},
	tracker::{self, IssueTracker},
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
	let mcp_test_fixture = ghost_lane_mcp_test_fixture_control_evidence(
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

	inspect_ghost_lane_tracker_issue(
		tracker,
		run,
		issue_identifier.as_deref(),
		requested_selector,
		&mut evidence,
		&mut blockers,
	)?;
	inspect_ghost_lane_worktree(
		worktree_root,
		state_store,
		run,
		issue_identifier.as_deref(),
		requested_selector,
		&mut evidence,
		&mut blockers,
	)?;

	let control_channel =
		inspect_ghost_lane_control_channel(run, mcp_test_fixture, &mut evidence, &mut blockers);

	inspect_ghost_lane_live_evidence(run, mcp_test_fixture, &mut evidence, &mut blockers);
	inspect_ghost_lane_private_evidence(
		project_id,
		state_store,
		run,
		mcp_test_fixture,
		&mut evidence,
		&mut blockers,
	)?;
	inspect_ghost_lane_review_lineage(
		project_id,
		state_store,
		run,
		issue_identifier.as_deref(),
		&mut evidence,
		&mut blockers,
	)?;

	let (classification, reason, next_action) = if blockers.is_empty() {
		let (classification, reason) = if mcp_test_fixture {
			(
				String::from(MCP_TEST_FIXTURE_GHOST_LANE_CLASSIFICATION),
				String::from("tracker_issue_missing_and_only_mcp_test_control_fixture_evidence"),
			)
		} else {
			(
				String::from(GHOST_LANE_CLASSIFICATION),
				String::from("tracker_issue_missing_and_no_live_or_retained_lane_evidence"),
			)
		};

		(
			classification,
			reason,
			format!(
				"Run `decodex recover ghost-lane cleanup {} --dry-run`, then rerun without `--dry-run` if the report stays safe.",
				issue_identifier.as_deref().unwrap_or(run.issue_id())
			),
		)
	} else {
		(
			String::from(GHOST_LANE_BLOCKED_CLASSIFICATION),
			String::from("safety_check_blocked"),
			String::from(
				"Preserve attention and inspect the listed blockers before using a recovery command.",
			),
		)
	};

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

fn inspect_ghost_lane_tracker_issue<T>(
	tracker: &T,
	run: &ProjectRunStatus,
	issue_identifier: Option<&str>,
	requested_selector: Option<&str>,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	let refreshed = match tracker.refresh_issues(&[run.issue_id().to_owned()]) {
		Ok(refreshed) => refreshed,
		Err(error) if tracker::issue_lookup_missing_error_for_candidate(&error, run.issue_id()) =>
			Vec::new(),
		Err(error) => return Err(error),
	};

	if !refreshed.is_empty() {
		blockers.push(String::from("tracker_issue_present"));

		return Ok(());
	}

	for selector in
		identifiers::ghost_lane_tracker_issue_selectors(run, issue_identifier, requested_selector)
	{
		match tracker.get_issue_by_identifier(&selector) {
			Ok(Some(_)) => {
				blockers.push(String::from("tracker_issue_present"));

				return Ok(());
			},
			Ok(None) => {},
			Err(error) if tracker::issue_lookup_missing_error_for_candidate(&error, &selector) => {
			},
			Err(error) => return Err(error),
		}
	}

	evidence.push(String::from("tracker_issue_missing"));

	Ok(())
}

fn inspect_ghost_lane_worktree(
	worktree_root: &Path,
	state_store: &StateStore,
	run: &ProjectRunStatus,
	issue_identifier: Option<&str>,
	requested_selector: Option<&str>,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) -> Result<()> {
	let mut retained_worktree_present = false;
	let mut mapping_checked = false;

	if let Some(worktree_path) = run.worktree_path() {
		mapping_checked = true;

		if worktree_path.exists() {
			retained_worktree_present = true;
		} else {
			evidence.push(String::from("worktree_mapping_path_missing"));
		}
	}
	if let Some(mapping) = state_store.worktree_for_issue(run.issue_id())? {
		mapping_checked = true;

		if mapping.worktree_path().exists() {
			retained_worktree_present = true;
		} else {
			evidence.push(String::from("worktree_mapping_path_missing"));
		}
	}

	for selector in
		identifiers::ghost_lane_worktree_selectors(run, issue_identifier, requested_selector)
	{
		if worktree_root.join(&selector).exists() {
			retained_worktree_present = true;
		}
	}

	if retained_worktree_present {
		blockers.push(String::from("retained_worktree_present"));
	} else {
		if !mapping_checked {
			evidence.push(String::from("worktree_mapping_missing"));
		}

		evidence.push(String::from("worktree_missing"));
	}

	Ok(())
}

fn inspect_ghost_lane_control_channel(
	run: &ProjectRunStatus,
	mcp_test_fixture: bool,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) -> String {
	let Some(channel) = run.control_channel() else {
		evidence.push(String::from("control_channel_missing"));

		return String::from("missing");
	};

	if channel.channel_path().exists() {
		evidence.push(String::from("control_channel_file_present"));
		blockers.push(String::from("control_channel_present"));
	} else {
		evidence.push(String::from("control_channel_file_missing"));

		if mcp_test_fixture {
			evidence.push(String::from("mcp_test_fixture_control_channel_row_present"));
		} else {
			blockers.push(String::from("control_channel_present"));
		}
	}

	format!("{}:present", channel.status())
}

fn inspect_ghost_lane_live_evidence(
	run: &ProjectRunStatus,
	mcp_test_fixture: bool,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) {
	let mut live_blockers = Vec::new();

	if run.event_count() > 0 || run.last_event_type().is_some() || run.last_event_at().is_some() {
		live_blockers.push(String::from("protocol_event_evidence_present"));
	}
	if run.child_agent_activity().is_some() {
		live_blockers.push(String::from("child_agent_activity_present"));
	}
	if run.protocol_activity().is_some() {
		live_blockers.push(String::from("protocol_activity_present"));
	}
	if run.thread_id().is_some() || run.turn_id().is_some() {
		live_blockers.push(String::from("thread_reference_present"));
	}
	if live_blockers.is_empty() {
		evidence.push(String::from("no_live_execution_evidence"));

		return;
	}
	if mcp_test_fixture
		&& live_blockers
			.iter()
			.all(|blocker| evidence::ghost_lane_mcp_test_fixture_allowed_live_blocker(blocker))
	{
		evidence.push(String::from("mcp_test_fixture_protocol_or_thread_evidence_present"));

		return;
	}

	blockers.extend(live_blockers);
}

fn inspect_ghost_lane_private_evidence(
	project_id: &str,
	state_store: &StateStore,
	run: &ProjectRunStatus,
	mcp_test_fixture: bool,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) -> Result<()> {
	let events = state_store.list_private_execution_events(
		project_id,
		run.issue_id(),
		run.run_id(),
		run.attempt_number(),
	)?;

	if events.is_empty() {
		evidence.push(String::from("private_evidence_missing"));
	} else if mcp_test_fixture {
		evidence.push(String::from("mcp_test_fixture_private_control_evidence_present"));

		if events.iter().any(evidence::ghost_lane_private_event_is_cleanup_audit) {
			evidence.push(String::from("ghost_lane_cleanup_audit_present"));
		}
	} else if evidence::ghost_lane_private_events_are_cleanup_audit_evidence(&events) {
		evidence.push(String::from("ghost_lane_cleanup_audit_present"));
	} else {
		blockers.push(String::from("private_evidence_present"));
	}

	Ok(())
}

fn ghost_lane_mcp_test_fixture_control_evidence(
	project_id: &str,
	state_store: &StateStore,
	run: &ProjectRunStatus,
	issue_identifier: Option<&str>,
) -> Result<bool> {
	if !evidence::ghost_lane_has_mcp_test_fixture_identity(project_id, run, issue_identifier) {
		return Ok(false);
	}

	let events = state_store.list_private_execution_events(
		project_id,
		run.issue_id(),
		run.run_id(),
		run.attempt_number(),
	)?;

	Ok(evidence::ghost_lane_private_events_are_mcp_test_recovery_evidence(&events))
}

fn inspect_ghost_lane_review_lineage(
	project_id: &str,
	state_store: &StateStore,
	run: &ProjectRunStatus,
	issue_identifier: Option<&str>,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) -> Result<()> {
	if state_store.issue_has_review_lifecycle_record(project_id, run.issue_id())? {
		blockers.push(String::from("review_lifecycle_present"));

		return Ok(());
	}
	if ghost_lane_run_has_review_policy_checkpoint(project_id, state_store, run)? {
		blockers.push(String::from("review_policy_checkpoint_present"));

		return Ok(());
	}

	let mut records = state_store.list_linear_execution_events(project_id, run.issue_id())?;

	if let Some(issue_identifier) = issue_identifier
		.filter(|issue_identifier| !issue_identifier.eq_ignore_ascii_case(run.issue_id()))
	{
		records.extend(state_store.list_linear_execution_events(project_id, issue_identifier)?);
	}

	if records.iter().any(evidence::ghost_lane_record_has_pr_or_review_lineage) {
		blockers.push(String::from("pr_or_review_lineage_present"));
	} else {
		evidence.push(String::from("review_lineage_missing"));
	}

	Ok(())
}

fn ghost_lane_run_has_review_policy_checkpoint(
	project_id: &str,
	state_store: &StateStore,
	run: &ProjectRunStatus,
) -> Result<bool> {
	for phase in ["handoff", "repair"] {
		if state_store
			.review_policy_checkpoint(
				project_id,
				run.issue_id(),
				run.run_id(),
				run.attempt_number(),
				phase,
			)?
			.is_some()
		{
			return Ok(true);
		}
	}

	Ok(false)
}
