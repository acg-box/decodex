//! Local execution-ledger projection for operator status snapshots.

use std::collections::HashSet;

use crate::{
	config::ServiceConfig,
	orchestrator::{
		OperatorHistoryLaneStatus, OperatorHistoryLedgerOutcome, OperatorLaneTerminalProjection,
		OperatorRunStatus, OperatorStatusSnapshot,
		kernel::state::{LivenessState, OwnershipState, PolicyState, TerminalizationState},
		operator_run_group_key,
		status_history_ledger::{self},
		status_run_projection::{self},
		status_summary,
	},
	prelude::Result,
	state::StateStore,
};

pub(super) fn hydrate_history_lanes_from_local_ledger(
	project: &ServiceConfig,
	state_store: &StateStore,
	snapshot: &mut OperatorStatusSnapshot,
) -> Result<()> {
	for lane in &mut snapshot.history_lanes {
		let records =
			state_store.list_linear_execution_events(project.service_id(), &lane.issue_id)?;

		if records.is_empty() {
			lane.ledger_outcome = status_history_ledger::missing_history_ledger_outcome();

			continue;
		}

		let records = status_history_ledger::local_history_ledger_records(records);

		status_history_ledger::hydrate_history_lane_from_ledger_records(lane, &records);

		lane.ledger_outcome = status_history_ledger::operator_history_ledger_outcome(&records);
	}

	Ok(())
}

pub(super) fn current_lane_terminal_projection_from_local_ledger(
	project: &ServiceConfig,
	state_store: &StateStore,
	snapshot: &OperatorStatusSnapshot,
) -> Result<OperatorLaneTerminalProjection> {
	let mut projection = OperatorLaneTerminalProjection::default();

	for run in &snapshot.current_lanes {
		let records =
			state_store.list_linear_execution_events(project.service_id(), &run.issue_id)?;

		if records.is_empty() {
			continue;
		}

		let records = status_history_ledger::local_history_ledger_records(records);
		let outcome = status_history_ledger::operator_history_ledger_outcome(&records);

		if history_ledger_outcome_is_terminal(&outcome) {
			projection
				.outcomes_by_issue_key
				.insert(status_run_projection::operator_run_group_key(run), outcome);
		}
	}

	Ok(projection)
}

pub(super) fn apply_operator_lane_terminal_projection(
	snapshot: &mut OperatorStatusSnapshot,
	projection: OperatorLaneTerminalProjection,
	completed_state: Option<&str>,
) {
	apply_terminal_history_ledger_outcomes(snapshot);

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
			&& current_lane_terminal_outcome_supersedes(
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

	apply_terminal_history_ledger_outcomes(snapshot);
}

pub(super) fn current_lane_has_authoritative_live_owner(run: &OperatorRunStatus) -> bool {
	status_run_projection::operator_run_lane_control_readback(run).has_authoritative_live_owner
}

pub(super) fn apply_terminal_history_ledger_outcomes(snapshot: &mut OperatorStatusSnapshot) {
	let mut terminal_history_keys = HashSet::new();

	for lane in &mut snapshot.history_lanes {
		if !history_ledger_outcome_supersedes_local_attempts(&lane.ledger_outcome) {
			continue;
		}

		terminal_history_keys.insert(history_lane_group_key(lane));

		apply_terminal_history_ledger_outcome_to_latest_run(lane);
	}

	if terminal_history_keys.is_empty() {
		return;
	}

	let current_lane_run_ids =
		snapshot.current_lanes.iter().map(|run| run.run_id.clone()).collect::<HashSet<_>>();
	let current_lane_issue_keys =
		snapshot.current_lanes.iter().map(operator_run_group_key).collect::<HashSet<_>>();

	snapshot.recent_runs.retain(|run| {
		let run_group_key = status_run_projection::operator_run_group_key(run);

		current_lane_run_ids.contains(&run.run_id)
			|| current_lane_issue_keys.contains(&run_group_key)
			|| !terminal_history_keys.contains(&run_group_key)
	});
}

pub(super) fn suppress_terminal_attention_queue_echoes(snapshot: &mut OperatorStatusSnapshot) {
	let terminal_attention_keys = snapshot
		.history_lanes
		.iter()
		.filter(|lane| history_ledger_outcome_requires_attention(&lane.ledger_outcome))
		.map(history_lane_group_key)
		.collect::<HashSet<_>>();

	if terminal_attention_keys.is_empty() {
		return;
	}

	snapshot.queued_candidates.retain(|candidate| {
		let candidate_key = status_summary::operator_issue_attention_key(
			&candidate.issue_id,
			Some(&candidate.issue_identifier),
		);
		let is_terminal_attention_echo = candidate.reason == "issue_needs_attention"
			&& terminal_attention_keys.contains(&candidate_key);

		!is_terminal_attention_echo
	});
}

pub(super) fn history_ledger_outcome_is_terminal(outcome: &OperatorHistoryLedgerOutcome) -> bool {
	outcome.ledger_status == "present"
		&& matches!(
			outcome.final_outcome.as_str(),
			"cleanup_complete" | "closeout" | "landed" | "needs_attention" | "terminal_failure"
		)
}

pub(super) fn history_ledger_outcome_requires_attention(
	outcome: &OperatorHistoryLedgerOutcome,
) -> bool {
	outcome.ledger_status == "present"
		&& matches!(outcome.final_outcome.as_str(), "needs_attention" | "terminal_failure")
}

pub(super) fn apply_terminal_history_ledger_outcome_to_run(
	run: &mut OperatorRunStatus,
	outcome: &OperatorHistoryLedgerOutcome,
) {
	let final_outcome = outcome.final_outcome.clone();
	let final_event_at = outcome.final_event_at.clone();
	let requires_attention = history_ledger_outcome_requires_attention(outcome);

	run.status = final_outcome.clone();
	run.attempt_status = final_outcome;
	run.status_projection_reason = None;
	run.phase = String::from(if requires_attention { "needs_attention" } else { "completed" });
	run.run_phase = run.phase.clone();
	run.wait_reason = None;
	run.current_operation = String::from("ledger_outcome");
	run.continuation_pending = false;
	run.run_lease = false;
	run.queue_lease_state = String::from("not_held");
	run.execution_liveness = String::from(LivenessState::NotRunning.as_str());
	run.ownership_state = String::from(
		if requires_attention { OwnershipState::RetainedAttention } else { OwnershipState::Closed }
			.as_str(),
	);
	run.liveness_state = String::from(LivenessState::NotRunning.as_str());
	run.policy_state = String::from(PolicyState::Allowed.as_str());
	run.terminalization_state = String::from(
		if requires_attention {
			TerminalizationState::None
		} else {
			TerminalizationState::CleanupComplete
		}
		.as_str(),
	);

	run.lane_control_conditions.clear();

	run.suspected_stall = false;
	run.retry_kind = None;
	run.next_retry_at = None;
	run.has_fresh_execution = false;
	run.counts_as_running = false;
	run.needs_attention = requires_attention;
	run.control_capability = None;

	if let Some(loop_status) = run.loop_status.as_mut() {
		loop_status.summary = format!(
			"terminal {}: {}",
			if requires_attention { "attention" } else { "lifecycle" },
			run.status
		);
		loop_status.next_action =
			requires_attention.then(|| outcome.needs_attention_reason.clone()).flatten();

		if loop_status
			.review
			.as_ref()
			.is_some_and(|review| review.status == "pending" && review.checkpoint.is_none())
		{
			loop_status.review = None;
		}
	}

	run.lane_control_next_action = if requires_attention {
		run.loop_status
			.as_ref()
			.and_then(|loop_status| loop_status.next_action.clone())
			.unwrap_or_else(|| String::from("inspect_lane_state"))
	} else {
		String::from("no_action")
	};

	if let Some(final_event_at) = final_event_at {
		run.updated_at = final_event_at.clone();
		run.last_run_activity_at = Some(final_event_at);
	}
}

pub(super) fn history_lane_group_key(lane: &OperatorHistoryLaneStatus) -> String {
	let issue_id = lane.issue_id.trim();

	if !issue_id.is_empty() && !issue_id.eq_ignore_ascii_case("unknown") {
		return issue_id.to_ascii_uppercase();
	}

	let issue_key = lane.issue_key.trim();

	if !issue_key.is_empty() && !issue_key.eq_ignore_ascii_case("unknown") {
		return issue_key.to_ascii_uppercase();
	}

	status_run_projection::operator_run_group_key(&lane.latest_run)
}

fn current_lane_terminal_outcome_supersedes(
	run: &OperatorRunStatus,
	outcome: &OperatorHistoryLedgerOutcome,
	completed_state: Option<&str>,
	current_attention_worktree_keys: &HashSet<String>,
) -> bool {
	if !history_ledger_outcome_is_terminal(outcome) {
		return false;
	}
	if current_lane_has_authoritative_live_owner(run) {
		return false;
	}
	if history_ledger_outcome_requires_attention(outcome) {
		return !current_lane_has_current_attention_signal(run, current_attention_worktree_keys);
	}

	current_lane_tracker_terminal_is_clean(run, completed_state)
}

fn current_lane_has_current_attention_signal(
	run: &OperatorRunStatus,
	current_attention_worktree_keys: &HashSet<String>,
) -> bool {
	run.needs_attention_label_present == Some(true)
		|| run.active_label_present == Some(true)
		|| current_attention_worktree_keys
			.contains(&status_run_projection::operator_run_group_key(run))
}

fn current_lane_tracker_terminal_is_clean(
	run: &OperatorRunStatus,
	completed_state: Option<&str>,
) -> bool {
	let has_tracker_metadata = run.issue_state.is_some()
		|| run.active_label_present.is_some()
		|| run.needs_attention_label_present.is_some();

	if !has_tracker_metadata {
		return true;
	}

	let Some(completed_state) = completed_state else {
		return false;
	};

	run.issue_state.as_deref() == Some(completed_state)
		&& run.active_label_present == Some(false)
		&& run.needs_attention_label_present == Some(false)
}

fn append_current_lane_to_history(
	snapshot: &mut OperatorStatusSnapshot,
	run: OperatorRunStatus,
	outcome: OperatorHistoryLedgerOutcome,
) {
	let group_key = status_run_projection::operator_run_group_key(&run);

	if let Some(lane) =
		snapshot.history_lanes.iter_mut().find(|lane| history_lane_group_key(lane) == group_key)
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

		apply_terminal_history_ledger_outcome_to_latest_run(lane);

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

	apply_terminal_history_ledger_outcome_to_latest_run(&mut lane);

	snapshot.history_lanes.push(lane);
}

fn history_ledger_outcome_supersedes_local_attempts(
	outcome: &OperatorHistoryLedgerOutcome,
) -> bool {
	history_ledger_outcome_is_terminal(outcome)
}

fn apply_terminal_history_ledger_outcome_to_latest_run(lane: &mut OperatorHistoryLaneStatus) {
	apply_terminal_history_ledger_outcome_to_run(&mut lane.latest_run, &lane.ledger_outcome);
}
