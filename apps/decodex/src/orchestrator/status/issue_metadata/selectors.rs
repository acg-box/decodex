use std::collections::{BTreeSet, HashSet};

use crate::{
	commit_message,
	orchestrator::{self, OperatorRunStatus, OperatorStatusSnapshot, RunIssueMetadataHydration},
};

pub(in crate::orchestrator) fn operator_run_is_stale_terminal_local_residue(
	run: &OperatorRunStatus,
	stale_terminal_local_issue_ids: &HashSet<String>,
) -> bool {
	operator_run_is_terminal_unleased_identifier(run)
		&& stale_terminal_local_issue_ids.contains(run.issue_id.trim())
}

pub(in crate::orchestrator) fn operator_run_tracker_issue_identifier_selector(
	run: &OperatorRunStatus,
) -> Option<String> {
	run.issue_identifier
		.as_ref()
		.filter(|identifier| commit_message::looks_like_issue_identifier(identifier))
		.map(|identifier| identifier.to_ascii_uppercase())
		.or_else(|| {
			orchestrator::operator_run_issue_identifier_from_fields(
				&run.run_id,
				run.branch_name.as_deref(),
				run.worktree_path.as_deref(),
			)
		})
		.or_else(|| {
			commit_message::looks_like_issue_identifier(&run.issue_id)
				.then(|| run.issue_id.to_ascii_uppercase())
		})
}

pub(in crate::orchestrator::status::issue_metadata) fn operator_snapshot_run_issue_ids(
	snapshot: &OperatorStatusSnapshot,
	hydration: RunIssueMetadataHydration,
	stale_terminal_local_issue_ids: &HashSet<String>,
) -> Vec<String> {
	let mut issue_ids = BTreeSet::new();

	for run in &snapshot.current_lanes {
		append_operator_run_issue_id(&mut issue_ids, run, stale_terminal_local_issue_ids);
	}

	if matches!(hydration, RunIssueMetadataHydration::AllRows) {
		for run in &snapshot.recent_runs {
			append_operator_run_issue_id(&mut issue_ids, run, stale_terminal_local_issue_ids);
		}
		for lane in &snapshot.history_lanes {
			append_operator_run_issue_id(
				&mut issue_ids,
				&lane.latest_run,
				stale_terminal_local_issue_ids,
			);

			for attempt in &lane.attempts {
				append_operator_run_issue_id(
					&mut issue_ids,
					attempt,
					stale_terminal_local_issue_ids,
				);
			}
		}
	}

	issue_ids.into_iter().collect()
}

fn append_operator_run_issue_id(
	issue_ids: &mut BTreeSet<String>,
	run: &OperatorRunStatus,
	stale_terminal_local_issue_ids: &HashSet<String>,
) {
	if operator_run_is_stale_terminal_local_residue(run, stale_terminal_local_issue_ids) {
		return;
	}

	let issue_id = run.issue_id.trim();

	if !issue_id.is_empty() && !issue_id.eq_ignore_ascii_case("unknown") {
		issue_ids.insert(issue_id.to_owned());
	}
}

fn operator_run_is_terminal_unleased_identifier(run: &OperatorRunStatus) -> bool {
	!run.run_lease
		&& orchestrator::looks_like_tracker_issue_identifier_key(&run.issue_id)
		&& orchestrator::local_run_attempt_status_is_terminal(&run.attempt_status)
}
