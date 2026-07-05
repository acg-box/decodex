use crate::orchestrator::kernel::{
	action::OwnedLaneAction,
	command::{CommandFact, CommandIntent, CommandIntentKind},
	decision::{self, OwnedLaneDecision, tests::support},
	projection::ProjectionHints,
	reason::ReasonCode,
	state::{LivenessState, OwnershipState, PolicyState, TerminalizationState},
};

#[test]
fn retained_lane_reentry_resumes() {
	let mut observation = support::authoritative_observation();

	observation.retained_lane_reusable = true;

	let decision = decision::decide_owned_lane(&observation);

	assert_eq!(
		decision,
		OwnedLaneDecision {
			decision_class: OwnedLaneAction::ResumeRetainedLane,
			policy_state: PolicyState::Allowed,
			lane_state_axes: support::axes(
				OwnershipState::Pending,
				LivenessState::Unknown,
				PolicyState::Allowed,
				TerminalizationState::None,
			),
			command_intents: vec![CommandIntent::new(
				CommandIntentKind::ResumeRetainedLane,
				"PUB-101:no-run:resume_retained_lane:retained_lane_reusable",
				vec![
					CommandFact::AuthorityComplete,
					CommandFact::IssueStillOwned,
					CommandFact::NoContradictoryAuthority,
					CommandFact::RetainedLaneReusable,
				],
				vec![CommandFact::RetainedLaneResumed],
			)],
			projection_hints: ProjectionHints::new(
				OwnedLaneAction::ResumeRetainedLane,
				"resume_retained_lane",
				ReasonCode::RetainedLaneReusable,
			),
			blockers: Vec::new(),
		}
	);
}
