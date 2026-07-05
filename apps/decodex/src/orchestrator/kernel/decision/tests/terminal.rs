use crate::orchestrator::kernel::{
	action::OwnedLaneAction,
	command::{CommandFact, CommandIntent, CommandIntentKind},
	decision::{self, OwnedLaneDecision, tests::support},
	projection::ProjectionHints,
	reason::ReasonCode,
	state::{LivenessState, OwnershipState, PolicyState, TerminalizationState},
};

#[test]
fn cleanup_pending_is_continue_with_cleanup_intent() {
	let mut observation = support::authoritative_observation();

	observation.terminalization = TerminalizationState::CleanupPending;

	let decision = decision::decide_owned_lane(&observation);

	assert_eq!(
		decision,
		OwnedLaneDecision {
			decision_class: OwnedLaneAction::Continue,
			policy_state: PolicyState::Allowed,
			lane_state_axes: support::axes(
				OwnershipState::Terminalizing,
				LivenessState::Unknown,
				PolicyState::Allowed,
				TerminalizationState::CleanupPending,
			),
			command_intents: vec![CommandIntent::new(
				CommandIntentKind::FinishTerminalCleanup,
				"PUB-101:no-run:finish_terminal_cleanup:terminal_cleanup_pending",
				vec![
					CommandFact::AuthorityComplete,
					CommandFact::IssueStillOwned,
					CommandFact::NoContradictoryAuthority,
					CommandFact::TerminalCleanupPending,
				],
				vec![CommandFact::TerminalCleanupCompleted],
			)],
			projection_hints: ProjectionHints::new(
				OwnedLaneAction::Continue,
				"finish_terminalization",
				ReasonCode::TerminalCleanupPending,
			),
			blockers: Vec::new(),
		}
	);
}
