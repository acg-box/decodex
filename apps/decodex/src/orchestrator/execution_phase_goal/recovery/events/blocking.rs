use serde_json::Value;

use crate::{
	orchestrator::{
		AUTHORITY_DECISION_REQUEST_EVENT_TYPE, PHASE_GOAL_RECOVERY_BLOCKED_EVENT_TYPE, Result,
		ServiceConfig, StateStore, execution_phase_goal::recovery::events::parsing,
	},
	state::{PROGRESS_CHECKPOINT_EVENT_TYPE, PROGRESS_CHECKPOINT_SCHEMA},
};

pub(crate) fn issue_has_blocking_lane_decision_evidence(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_id: &str,
) -> Result<bool> {
	let events =
		state_store.list_private_execution_events_for_issue(project.service_id(), issue_id)?;

	for event in events.iter().rev() {
		match event.event_type() {
			"terminal_finalize" | "review_completion_intent" => return Ok(false),
			AUTHORITY_DECISION_REQUEST_EVENT_TYPE | PHASE_GOAL_RECOVERY_BLOCKED_EVENT_TYPE => {
				return Ok(true);
			},
			"lane_decision" if lane_decision_event_blocks_automatic_recovery(event.payload()) => {
				return Ok(true);
			},
			event_type
				if event_type == PROGRESS_CHECKPOINT_EVENT_TYPE
					&& event.matches_contract(
						PROGRESS_CHECKPOINT_EVENT_TYPE,
						PROGRESS_CHECKPOINT_SCHEMA,
						2,
					) && parsing::progress_checkpoint_has_blockers(event.payload()) =>
			{
				return Ok(true);
			},
			event_type
				if event_type == PROGRESS_CHECKPOINT_EVENT_TYPE
					&& event.matches_contract(
						PROGRESS_CHECKPOINT_EVENT_TYPE,
						PROGRESS_CHECKPOINT_SCHEMA,
						2,
					) && parsing::progress_checkpoint_clears_blockers(event.payload()) =>
			{
				return Ok(false);
			},
			"phase_goal_next" | "phase_goal_transition" | "phase_goal_completed" => {
				return Ok(false);
			},
			_ => {},
		}
	}

	Ok(false)
}

fn lane_decision_event_blocks_automatic_recovery(payload: &Value) -> bool {
	if let Some(kernel_decision) = payload.get("kernel_decision") {
		return kernel_decision
			.get("decision_class")
			.and_then(Value::as_str)
			.is_some_and(|decision_class| decision_class == "manual_intervention_required")
			|| kernel_decision.get("command_intents").and_then(Value::as_array).is_some_and(
				|intents| {
					intents.iter().any(|intent| {
						intent.get("kind").and_then(Value::as_str)
							== Some("request_manual_intervention")
					})
				},
			);
	}

	payload.get("next_action").and_then(Value::as_str).is_some_and(|action| {
		matches!(action, "needs_attention" | "stop_blocked" | "forbidden_stale_or_ambiguous")
	})
}
