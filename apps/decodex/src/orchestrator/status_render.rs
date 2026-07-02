mod activity;
mod execution_programs;
mod post_review;
mod queue;
mod run_rows;
mod summary;
mod warnings;
mod worktrees;

pub(crate) use self::queue::render_queue_explain;

use crate::orchestrator::{self, OperatorStatusSnapshot, OperatorWorktreeStatus};

pub(in crate::orchestrator) fn rendered_recovery_worktrees(
	snapshot: &OperatorStatusSnapshot,
) -> Vec<(&str, &OperatorWorktreeStatus)> {
	worktrees::rendered_recovery_worktrees(snapshot)
}

pub(crate) fn render_operator_status(snapshot: &OperatorStatusSnapshot) -> String {
	let session_history_attempt_count =
		snapshot.history_lanes.iter().map(|lane| lane.attempt_count).sum::<usize>();
	let hides_current_lanes = session_history_attempt_count < snapshot.recent_runs.len();
	let (current_lane_claims, backlog_or_stale_queue_candidates): (Vec<_>, Vec<_>) =
		snapshot.queued_candidates.iter().partition(|queued_issue| {
			queue::queue_claim_belongs_to_current_lane(queued_issue, snapshot)
		});
	let (stale_closed_queue_labels, backlog_candidates) =
		queue::rendered_backlog_queue_groups(backlog_or_stale_queue_candidates);
	let recovery_worktrees = worktrees::rendered_recovery_worktrees(snapshot);
	let hides_owned_worktrees = recovery_worktrees.len() < snapshot.worktrees.len();
	let mut output = String::new();

	output.push_str(&format!("Project: {}\n", snapshot.project_id));

	if let Some(status_source) = snapshot.status_source.as_deref() {
		output.push_str(&format!("Status source: {status_source}\n"));
	}
	if let Some(snapshot_age_seconds) = snapshot.snapshot_age_seconds {
		output.push_str(&format!("Snapshot age: {snapshot_age_seconds}s\n"));
	}

	output.push_str(&format!("Warnings: {}\n", snapshot.warnings.len()));

	if !snapshot.warnings.is_empty() {
		output.push_str(&format!(
			"Warning details: {}\n",
			warnings::render_warning_details(snapshot)
		));
	}

	summary::append_rendered_github_cli_authority(&mut output, snapshot);

	let running_lane_count = snapshot
		.current_lanes
		.iter()
		.filter(|run| orchestrator::operator_run_counts_as_running(run))
		.count();

	output.push_str(&format!("Current lanes: {}\n", snapshot.current_lanes.len()));
	output.push_str(&format!("Running lanes: {running_lane_count}\n"));
	output.push_str(&format!(
		"Run ledger shown: {} issue lanes from {} history attempts{}\n",
		snapshot.history_lanes.len(),
		session_history_attempt_count,
		if hides_current_lanes { " (current lanes inline)" } else { "" },
	));
	output.push_str(&format!("Backlog: {}\n", backlog_candidates.len()));
	output.push_str(&format!("Claimed queue echoes: {}\n", current_lane_claims.len()));
	output.push_str(&format!("Stale closed queue labels: {}\n", stale_closed_queue_labels.len()));
	output.push_str(&format!("Execution programs: {}\n", snapshot.execution_programs.len()));
	output.push_str(&format!("Recovery worktrees: {}\n", recovery_worktrees.len()));
	output.push_str(&format!("Post-review lanes: {}\n", snapshot.post_review_lanes.len()));

	summary::append_rendered_attention_summary(&mut output, snapshot);
	execution_programs::append_rendered_execution_programs(&mut output, snapshot);

	output.push_str("\nCurrent Lanes\n");

	if snapshot.current_lanes.is_empty() {
		output.push_str("- none\n");
	} else {
		for run in &snapshot.current_lanes {
			run_rows::append_rendered_run(&mut output, run);
		}
	}

	output.push_str("\nRun Ledger\n");

	if snapshot.history_lanes.is_empty() {
		if hides_current_lanes {
			output.push_str("- none (current lanes are shown above)\n");
		} else {
			output.push_str("- none\n");
		}
	} else {
		for lane in &snapshot.history_lanes {
			run_rows::append_rendered_history_lane(&mut output, lane);
		}
	}

	queue::append_rendered_queued_issue_section(
		&mut output,
		"Backlog",
		&backlog_candidates,
		snapshot,
		false,
	);
	queue::append_rendered_queued_issue_section(
		&mut output,
		"Claimed Queue Echoes",
		&current_lane_claims,
		snapshot,
		true,
	);
	queue::append_rendered_queued_issue_section(
		&mut output,
		"Stale Closed Queue Labels",
		&stale_closed_queue_labels,
		snapshot,
		false,
	);

	output.push_str("\nRecovery Worktrees\n");

	worktrees::append_rendered_recovery_worktrees(
		&mut output,
		&recovery_worktrees,
		hides_owned_worktrees,
	);

	output.push_str("\nPost-Review Lanes\n");

	post_review::append_rendered_post_review_lanes(&mut output, snapshot);

	output
}
