use crate::orchestrator::kernel::{
	action::OwnedLaneAction,
	command::{CommandFact, CommandIntent, CommandIntentKind},
	decision::{self, DecisionBlocker, OwnedLaneDecision, tests::support},
	projection::ProjectionHints,
	reason::ReasonCode,
	state::{LivenessState, OwnershipState, PolicyState, TerminalizationState},
};

#[test]
fn contradictory_authority_requires_manual_intervention() {
	let mut observation = support::authoritative_observation();

	observation.contradictory_authority = true;
	observation.retry_budget_available = true;

	let decision = decision::decide_owned_lane(&observation);

	assert_eq!(
		decision,
		OwnedLaneDecision {
			decision_class: OwnedLaneAction::ManualInterventionRequired,
			policy_state: PolicyState::HumanAttentionRequired,
			lane_state_axes: support::axes(
				OwnershipState::RetainedAttention,
				LivenessState::Unknown,
				PolicyState::HumanAttentionRequired,
				TerminalizationState::None,
			),
			command_intents: vec![CommandIntent::new(
				CommandIntentKind::RequestManualIntervention,
				"PUB-101:no-run:request_manual_intervention:contradictory_authority",
				Vec::new(),
				vec![CommandFact::HumanInterventionRecorded],
			)],
			projection_hints: ProjectionHints::new(
				OwnedLaneAction::ManualInterventionRequired,
				"resolve_policy_stop_before_mutating_lane",
				ReasonCode::ContradictoryAuthority,
			),
			blockers: vec![DecisionBlocker::new(ReasonCode::ContradictoryAuthority)],
		}
	);
}

#[test]
fn incomplete_authority_fails_closed() {
	let observation = support::observation();
	let decision = decision::decide_owned_lane(&observation);

	assert_eq!(
		decision,
		OwnedLaneDecision {
			decision_class: OwnedLaneAction::ManualInterventionRequired,
			policy_state: PolicyState::HumanAttentionRequired,
			lane_state_axes: support::axes(
				OwnershipState::RetainedAttention,
				LivenessState::Unknown,
				PolicyState::HumanAttentionRequired,
				TerminalizationState::None,
			),
			command_intents: vec![CommandIntent::new(
				CommandIntentKind::RequestManualIntervention,
				"PUB-101:no-run:request_manual_intervention:incomplete_authority",
				Vec::new(),
				vec![CommandFact::HumanInterventionRecorded],
			)],
			projection_hints: ProjectionHints::new(
				OwnedLaneAction::ManualInterventionRequired,
				"resolve_policy_stop_before_mutating_lane",
				ReasonCode::IncompleteAuthority,
			),
			blockers: vec![DecisionBlocker::new(ReasonCode::IncompleteAuthority)],
		}
	);
}
