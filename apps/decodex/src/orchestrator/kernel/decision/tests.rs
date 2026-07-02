use crate::orchestrator::kernel::{
	action::OwnedLaneAction,
	command::{CommandFact, CommandIntent, CommandIntentKind},
	decision::{self, DecisionBlocker, OwnedLaneDecision},
	facts::LaneObservation,
	projection::ProjectionHints,
	reason::ReasonCode,
	state::{LaneStateAxes, LivenessState, OwnershipState, PolicyState, TerminalizationState},
};

fn observation() -> LaneObservation {
	LaneObservation::for_issue("PUB-101")
}

fn authoritative_observation() -> LaneObservation {
	let mut observation = observation();

	observation.authority_complete = true;

	observation
}

fn axes(
	ownership: OwnershipState,
	liveness: LivenessState,
	policy: PolicyState,
	terminalization: TerminalizationState,
) -> LaneStateAxes {
	LaneStateAxes::new(ownership, liveness, policy, terminalization)
}

#[test]
fn active_owned_work_continues() {
	let mut observation = authoritative_observation();

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
			lane_state_axes: axes(
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
	let mut observation = authoritative_observation();

	observation.external_signal_pending = true;

	let decision = decision::decide_owned_lane(&observation);

	assert_eq!(
		decision,
		OwnedLaneDecision {
			decision_class: OwnedLaneAction::WaitForExternalSignal,
			policy_state: PolicyState::ReviewPending,
			lane_state_axes: axes(
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
	let mut observation = authoritative_observation();

	observation.retry_budget_available = true;

	let decision = decision::decide_owned_lane(&observation);

	assert_eq!(
		decision,
		OwnedLaneDecision {
			decision_class: OwnedLaneAction::RetryAutomatically,
			policy_state: PolicyState::Allowed,
			lane_state_axes: axes(
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

#[test]
fn retained_lane_reentry_resumes() {
	let mut observation = authoritative_observation();

	observation.retained_lane_reusable = true;

	let decision = decision::decide_owned_lane(&observation);

	assert_eq!(
		decision,
		OwnedLaneDecision {
			decision_class: OwnedLaneAction::ResumeRetainedLane,
			policy_state: PolicyState::Allowed,
			lane_state_axes: axes(
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

#[test]
fn contradictory_authority_requires_manual_intervention() {
	let mut observation = authoritative_observation();

	observation.contradictory_authority = true;
	observation.retry_budget_available = true;

	let decision = decision::decide_owned_lane(&observation);

	assert_eq!(
		decision,
		OwnedLaneDecision {
			decision_class: OwnedLaneAction::ManualInterventionRequired,
			policy_state: PolicyState::HumanAttentionRequired,
			lane_state_axes: axes(
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
fn ready_pull_request_lands() {
	let mut observation = authoritative_observation();

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
	let mut observation = authoritative_observation();

	observation.ready_to_land = true;

	let decision = decision::decide_owned_lane(&observation);

	assert_eq!(
		decision,
		OwnedLaneDecision {
			decision_class: OwnedLaneAction::ManualInterventionRequired,
			policy_state: PolicyState::HumanAttentionRequired,
			lane_state_axes: axes(
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

#[test]
fn cleanup_pending_is_continue_with_cleanup_intent() {
	let mut observation = authoritative_observation();

	observation.terminalization = TerminalizationState::CleanupPending;

	let decision = decision::decide_owned_lane(&observation);

	assert_eq!(
		decision,
		OwnedLaneDecision {
			decision_class: OwnedLaneAction::Continue,
			policy_state: PolicyState::Allowed,
			lane_state_axes: axes(
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

#[test]
fn incomplete_authority_fails_closed() {
	let observation = observation();
	let decision = decision::decide_owned_lane(&observation);

	assert_eq!(
		decision,
		OwnedLaneDecision {
			decision_class: OwnedLaneAction::ManualInterventionRequired,
			policy_state: PolicyState::HumanAttentionRequired,
			lane_state_axes: axes(
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
