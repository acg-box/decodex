use std::collections::HashSet;

use crate::orchestrator::{
	OperatorHistoryLaneStatus, OperatorHistoryLedgerOutcome, OperatorRunStatus,
	OperatorStatusSnapshot,
	kernel::state::{LivenessState, OwnershipState, PolicyState, TerminalizationState},
	status_history_projection::predicates,
	status_run_projection,
};

pub(crate) fn apply_terminal_history_ledger_outcomes(snapshot: &mut OperatorStatusSnapshot) {
	let mut terminal_history_keys = HashSet::new();

	for lane in &mut snapshot.history_lanes {
		if !predicates::history_ledger_outcome_supersedes_local_attempts(&lane.ledger_outcome) {
			continue;
		}

		terminal_history_keys.insert(predicates::history_lane_group_key(lane));

		apply_terminal_history_ledger_outcome_to_latest_run(lane);
	}

	if terminal_history_keys.is_empty() {
		return;
	}

	let current_lane_run_ids =
		snapshot.current_lanes.iter().map(|run| run.run_id.clone()).collect::<HashSet<_>>();
	let current_lane_issue_keys = snapshot
		.current_lanes
		.iter()
		.map(status_run_projection::operator_run_group_key)
		.collect::<HashSet<_>>();

	snapshot.recent_runs.retain(|run| {
		let run_group_key = status_run_projection::operator_run_group_key(run);

		current_lane_run_ids.contains(&run.run_id)
			|| current_lane_issue_keys.contains(&run_group_key)
			|| !terminal_history_keys.contains(&run_group_key)
	});
}

pub(crate) fn apply_terminal_history_ledger_outcome_to_run(
	run: &mut OperatorRunStatus,
	outcome: &OperatorHistoryLedgerOutcome,
) {
	let final_outcome = outcome.final_outcome.clone();
	let final_event_at = outcome.final_event_at.clone();
	let requires_attention = predicates::history_ledger_outcome_requires_attention(outcome);

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

pub(in crate::orchestrator::status::history_projection::terminal) fn apply_terminal_history_ledger_outcome_to_latest_run(
	lane: &mut OperatorHistoryLaneStatus,
) {
	apply_terminal_history_ledger_outcome_to_run(&mut lane.latest_run, &lane.ledger_outcome);
}
