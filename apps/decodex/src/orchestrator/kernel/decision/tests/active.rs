use crate::orchestrator::kernel::{
	action::OwnedLaneAction,
	command::{CommandFact, CommandIntent, CommandIntentKind},
	decision::{self, OwnedLaneDecision, tests::support},
	projection::ProjectionHints,
	reason::ReasonCode,
	state::{LivenessState, OwnershipState, PolicyState, TerminalizationState},
};

#[test]
fn active_owned_work_continues() {
	let mut observation = support::authoritative_observation();

	observation.run_id = Some(String::from("run-1"));
	observation.run_lease = true;
	observation.active_owned_work = true;
	observation.liveness = LivenessState::ProcessAlive;

	let decision = decision::decide_owned_lane(&observation);

	assert_eq!(
		decision,
		OwnedLaneDecision {
			decision_class: OwnedLaneAction::Continue,
			policy_state: PolicyState::Allowed,
			lane_state_axes: support::axes(
				OwnershipState::LeasedRun,
				LivenessState::ProcessAlive,
				PolicyState::Allowed,
				TerminalizationState::None,
			),
			command_intents: vec![CommandIntent::new(
				CommandIntentKind::ContinueAttempt,
				"PUB-101:run-1:continue_attempt:active_owned_work",
				vec![
					CommandFact::AuthorityComplete,
					CommandFact::IssueStillOwned,
					CommandFact::NoContradictoryAuthority,
					CommandFact::ActiveOwnedWorkPresent,
				],
				vec![CommandFact::ActiveOwnedWorkPresent],
			)],
			projection_hints: ProjectionHints::new(
				OwnedLaneAction::Continue,
				"continue_owned_attempt",
				ReasonCode::ActiveOwnedWork,
			),
			blockers: Vec::new(),
		}
	);
}

#[test]
fn external_signal_waits() {
	let mut observation = support::authoritative_observation();

	observation.external_signal_pending = true;

	let decision = decision::decide_owned_lane(&observation);

	assert_eq!(
		decision,
		OwnedLaneDecision {
			decision_class: OwnedLaneAction::WaitForExternalSignal,
			policy_state: PolicyState::ReviewPending,
			lane_state_axes: support::axes(
				OwnershipState::Pending,
				LivenessState::Unknown,
				PolicyState::ReviewPending,
				TerminalizationState::None,
			),
			command_intents: vec![CommandIntent::new(
				CommandIntentKind::WaitExternal,
				"PUB-101:no-run:wait_external:external_signal_pending",
				vec![
					CommandFact::AuthorityComplete,
					CommandFact::IssueStillOwned,
					CommandFact::NoContradictoryAuthority,
					CommandFact::ExternalSignalStillPending,
				],
				vec![CommandFact::ExternalSignalStillPending],
			)],
			projection_hints: ProjectionHints::new(
				OwnedLaneAction::WaitForExternalSignal,
				"wait_external",
				ReasonCode::ExternalSignalPending,
			),
			blockers: Vec::new(),
		}
	);
}

#[test]
fn retry_budget_schedules_retry() {
	let mut observation = support::authoritative_observation();

	observation.retry_budget_available = true;

	let decision = decision::decide_owned_lane(&observation);

	assert_eq!(
		decision,
		OwnedLaneDecision {
			decision_class: OwnedLaneAction::RetryAutomatically,
			policy_state: PolicyState::Allowed,
			lane_state_axes: support::axes(
				OwnershipState::Pending,
				LivenessState::Unknown,
				PolicyState::Allowed,
				TerminalizationState::None,
			),
			command_intents: vec![CommandIntent::new(
				CommandIntentKind::ScheduleRetry,
				"PUB-101:no-run:schedule_retry:retry_budget_available",
				vec![
					CommandFact::AuthorityComplete,
					CommandFact::IssueStillOwned,
					CommandFact::NoContradictoryAuthority,
					CommandFact::RetryBudgetAvailable,
					CommandFact::NoHumanAttentionSignal,
				],
				vec![CommandFact::RetryScheduled],
			)],
			projection_hints: ProjectionHints::new(
				OwnedLaneAction::RetryAutomatically,
				"schedule_retry",
				ReasonCode::RetryBudgetAvailable,
			),
			blockers: Vec::new(),
		}
	);
}
