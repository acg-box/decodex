use serde_json::Value;

use crate::orchestrator::kernel::{
	command::{CommandFact, CommandIntent},
	decision::{DecisionBlocker, OwnedLaneDecision},
	state::LaneStateAxes,
};

pub(super) fn owned_lane_decision_to_json(decision: &OwnedLaneDecision) -> Value {
	serde_json::json!({
		"decision_class": decision.decision_class.as_str(),
		"policy_state": decision.policy_state.as_str(),
		"lane_state_axes": lane_state_axes_to_json(decision.lane_state_axes),
		"command_intents": decision
			.command_intents
			.iter()
			.map(command_intent_to_json)
			.collect::<Vec<_>>(),
		"projection_hints": {
			"lane_control_next_action": decision.projection_hints.lane_control_next_action,
			"primary_reason": decision.projection_hints.primary_reason.as_str(),
		},
		"blockers": decision
			.blockers
			.iter()
			.map(decision_blocker_to_json)
			.collect::<Vec<_>>(),
	})
}

fn lane_state_axes_to_json(axes: LaneStateAxes) -> Value {
	serde_json::json!({
		"ownership": axes.ownership.as_str(),
		"liveness": axes.liveness.as_str(),
		"policy": axes.policy.as_str(),
		"terminalization": axes.terminalization.as_str(),
	})
}

fn command_intent_to_json(intent: &CommandIntent) -> Value {
	serde_json::json!({
		"kind": intent.kind.as_str(),
		"idempotency_key": intent.idempotency_key,
		"preconditions": facts_to_json(&intent.preconditions),
		"expected_postconditions": facts_to_json(&intent.expected_postconditions),
	})
}

fn facts_to_json(facts: &[CommandFact]) -> Vec<&'static str> {
	facts.iter().map(|fact| fact.as_str()).collect()
}

fn decision_blocker_to_json(blocker: &DecisionBlocker) -> Value {
	serde_json::json!({
		"reason": blocker.reason.as_str(),
		"public_summary": blocker.public_summary,
	})
}
