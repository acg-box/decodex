use crate::orchestrator::kernel::{
	action::OwnedLaneAction,
	command::{CommandFact, CommandIntent, CommandIntentKind},
	decision::{self, DecisionBlocker, OwnedLaneDecision, tests::support},
	projection::ProjectionHints,
	reason::ReasonCode,
	state::{LaneStateAxes, LivenessState, OwnershipState, PolicyState, TerminalizationState},
};

#[test]
fn ready_pull_request_lands() {
	let mut observation = support::authoritative_observation();

	observation.ready_to_land = true;
	observation.post_review_lifecycle_present = true;

	let decision = decision::decide_owned_lane(&observation);

	assert_eq!(
		decision,
		OwnedLaneDecision {
			decision_class: OwnedLaneAction::ReadyToLand,
			policy_state: PolicyState::Allowed,
			lane_state_axes: LaneStateAxes::new(
				OwnershipState::Pending,
				LivenessState::Unknown,
				PolicyState::Allowed,
				TerminalizationState::None,
			),
			command_intents: vec![CommandIntent::new(
				CommandIntentKind::LandReadyPullRequest,
				"PUB-101:no-run:land_ready_pull_request:ready_to_land",
				vec![
					CommandFact::AuthorityComplete,
					CommandFact::IssueStillOwned,
					CommandFact::NoContradictoryAuthority,
					CommandFact::PostReviewLifecyclePresent,
					CommandFact::ReadyToLandPrerequisitesSatisfied,
				],
				vec![CommandFact::LandingSequenceStarted],
			)],
			projection_hints: ProjectionHints::new(
				OwnedLaneAction::ReadyToLand,
				"ready_to_land",
				ReasonCode::ReadyToLand,
			),
			blockers: Vec::new(),
		}
	);
}

#[test]
fn missing_post_review_lifecycle_fails_closed() {
	let mut observation = support::authoritative_observation();

	observation.ready_to_land = true;

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
				"PUB-101:no-run:request_manual_intervention:post_review_lifecycle_missing",
				Vec::new(),
				vec![CommandFact::HumanInterventionRecorded],
			)],
			projection_hints: ProjectionHints::new(
				OwnedLaneAction::ManualInterventionRequired,
				"resolve_policy_stop_before_mutating_lane",
				ReasonCode::PostReviewLifecycleMissing,
			),
			blockers: vec![DecisionBlocker::new(ReasonCode::PostReviewLifecycleMissing)],
		}
	);
}
