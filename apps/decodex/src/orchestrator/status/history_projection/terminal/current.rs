use std::collections::HashSet;

use crate::orchestrator::{
	OperatorHistoryLaneStatus, OperatorHistoryLedgerOutcome, OperatorLaneTerminalProjection,
	OperatorRunStatus, OperatorStatusSnapshot,
	status_history_projection::{predicates, terminal::ledger},
	status_run_projection, status_summary,
};

pub(crate) fn apply_operator_lane_terminal_projection(
	snapshot: &mut OperatorStatusSnapshot,
	projection: OperatorLaneTerminalProjection,
	completed_state: Option<&str>,
) {
	ledger::apply_terminal_history_ledger_outcomes(snapshot);

	if projection.outcomes_by_issue_key.is_empty() {
		return;
	}

	let current_attention_worktree_keys = snapshot
		.worktrees
		.iter()
		.map(|worktree| {
			status_summary::operator_issue_attention_key(
				&worktree.issue_id,
				worktree.issue_identifier.as_deref(),
			)
		})
		.collect::<HashSet<_>>();
	let mut retained_current_lanes = Vec::new();
	let mut demoted_lanes = Vec::new();

	for run in snapshot.current_lanes.drain(..) {
		if let Some(outcome) = projection
			.outcomes_by_issue_key
			.get(&status_run_projection::operator_run_group_key(&run))
			&& predicates::current_lane_terminal_outcome_supersedes(
				&run,
				outcome,
				completed_state,
				&current_attention_worktree_keys,
			) {
			demoted_lanes.push((run, outcome.clone()));
		} else {
			retained_current_lanes.push(run);
		}
	}

	snapshot.current_lanes = retained_current_lanes;

	for (run, outcome) in demoted_lanes {
		append_current_lane_to_history(snapshot, run, outcome);
	}

	ledger::apply_terminal_history_ledger_outcomes(snapshot);
}

fn append_current_lane_to_history(
	snapshot: &mut OperatorStatusSnapshot,
	run: OperatorRunStatus,
	outcome: OperatorHistoryLedgerOutcome,
) {
	let group_key = status_run_projection::operator_run_group_key(&run);

	if let Some(lane) = snapshot
		.history_lanes
		.iter_mut()
		.find(|lane| predicates::history_lane_group_key(lane) == group_key)
	{
		if !lane.attempts.iter().any(|attempt| attempt.run_id == run.run_id) {
			status_run_projection::hydrate_history_lane_from_run(lane, &run);

			if run.attempt_number > lane.latest_run.attempt_number {
				lane.latest_run = run.clone();
			}

			lane.attempts.push(run);

			lane.attempt_count = lane.attempts.len();
			lane.lifecycle_metrics =
				status_run_projection::operator_lane_lifecycle_metrics(&lane.attempts);
		}

		lane.ledger_outcome = outcome;

		ledger::apply_terminal_history_ledger_outcome_to_latest_run(lane);

		return;
	}

	let attempts = vec![run.clone()];
	let lifecycle_metrics = status_run_projection::operator_lane_lifecycle_metrics(&attempts);
	let issue_identifier = run.issue_identifier.clone();
	let issue_key = status_run_projection::operator_run_issue_key(&run);
	let mut lane = OperatorHistoryLaneStatus {
		project_id: run.project_id.clone(),
		issue_id: run.issue_id.clone(),
		issue_identifier,
		title: run.title.clone(),
		author: run.author.clone(),
		issue_state: run.issue_state.clone(),
		active_label_present: run.active_label_present,
		needs_attention_label_present: run.needs_attention_label_present,
		issue_key,
		attempt_count: 1,
		ledger_outcome: outcome,
		lifecycle_metrics,
		latest_run: run,
		attempts,
	};

	ledger::apply_terminal_history_ledger_outcome_to_latest_run(&mut lane);

	snapshot.history_lanes.push(lane);
}
