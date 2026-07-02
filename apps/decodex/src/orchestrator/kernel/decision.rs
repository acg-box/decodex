use super::{
	action::OwnedLaneAction,
	command::{CommandFact, CommandIntent, CommandIntentKind},
	facts::LaneObservation,
	projection::ProjectionHints,
	reason::ReasonCode,
	state::{LaneStateAxes, OwnershipState, PolicyState, TerminalizationState},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::orchestrator) struct OwnedLaneDecision {
	pub(in crate::orchestrator) decision_class: OwnedLaneAction,
	pub(in crate::orchestrator) policy_state: PolicyState,
	pub(in crate::orchestrator) lane_state_axes: LaneStateAxes,
	pub(in crate::orchestrator) command_intents: Vec<CommandIntent>,
	pub(in crate::orchestrator) projection_hints: ProjectionHints,
	pub(in crate::orchestrator) blockers: Vec<DecisionBlocker>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::orchestrator) struct DecisionBlocker {
	pub(in crate::orchestrator) reason: ReasonCode,
	pub(in crate::orchestrator) public_summary: &'static str,
}

impl DecisionBlocker {
	pub(in crate::orchestrator) const fn new(reason: ReasonCode) -> Self {
		Self { reason, public_summary: reason.public_summary() }
	}
}

pub(in crate::orchestrator) fn decide_owned_lane(
	observation: &LaneObservation,
) -> OwnedLaneDecision {
	if observation.contradictory_authority {
		return manual_decision(observation, ReasonCode::ContradictoryAuthority);
	}
	if !observation.authority_complete {
		return manual_decision(observation, ReasonCode::IncompleteAuthority);
	}
	if observation.post_review_lifecycle_required && !observation.post_review_lifecycle_present {
		return manual_decision(observation, ReasonCode::PostReviewLifecycleMissing);
	}
	if observation.ready_to_land && !observation.post_review_lifecycle_present {
		return manual_decision(observation, ReasonCode::PostReviewLifecycleMissing);
	}
	if observation.human_attention_signal {
		return manual_decision(observation, ReasonCode::HumanAttentionSignal);
	}
	if observation.retry_budget_exhausted {
		return manual_decision(observation, ReasonCode::RetryBudgetExhausted);
	}
	if observation.ready_to_land {
		return decision(
			observation,
			OwnedLaneAction::ReadyToLand,
			PolicyState::Allowed,
			"ready_to_land",
			ReasonCode::ReadyToLand,
			vec![intent(
				observation,
				CommandIntentKind::LandReadyPullRequest,
				ReasonCode::ReadyToLand,
			)],
			Vec::new(),
		);
	}
	if observation.retained_lane_reusable {
		return decision(
			observation,
			OwnedLaneAction::ResumeRetainedLane,
			PolicyState::Allowed,
			"resume_retained_lane",
			ReasonCode::RetainedLaneReusable,
			vec![intent(
				observation,
				CommandIntentKind::ResumeRetainedLane,
				ReasonCode::RetainedLaneReusable,
			)],
			Vec::new(),
		);
	}
	if observation.retry_budget_available {
		return decision(
			observation,
			OwnedLaneAction::RetryAutomatically,
			PolicyState::Allowed,
			"schedule_retry",
			ReasonCode::RetryBudgetAvailable,
			vec![intent(
				observation,
				CommandIntentKind::ScheduleRetry,
				ReasonCode::RetryBudgetAvailable,
			)],
			Vec::new(),
		);
	}
	if observation.external_signal_pending {
		return decision(
			observation,
			OwnedLaneAction::WaitForExternalSignal,
			PolicyState::ReviewPending,
			"wait_external",
			ReasonCode::ExternalSignalPending,
			vec![intent(
				observation,
				CommandIntentKind::WaitExternal,
				ReasonCode::ExternalSignalPending,
			)],
			Vec::new(),
		);
	}
	if observation.terminalization == TerminalizationState::CleanupPending {
		return decision(
			observation,
			OwnedLaneAction::Continue,
			PolicyState::Allowed,
			"finish_terminalization",
			ReasonCode::TerminalCleanupPending,
			vec![intent(
				observation,
				CommandIntentKind::FinishTerminalCleanup,
				ReasonCode::TerminalCleanupPending,
			)],
			Vec::new(),
		);
	}
	if observation.active_owned_work {
		return decision(
			observation,
			OwnedLaneAction::Continue,
			PolicyState::Allowed,
			"continue_owned_attempt",
			ReasonCode::ActiveOwnedWork,
			vec![intent(
				observation,
				CommandIntentKind::ContinueAttempt,
				ReasonCode::ActiveOwnedWork,
			)],
			Vec::new(),
		);
	}

	decision(
		observation,
		OwnedLaneAction::WaitForExternalSignal,
		PolicyState::Allowed,
		"inspect_lane_state",
		ReasonCode::NoRunnableWork,
		Vec::new(),
		Vec::new(),
	)
}

fn manual_decision(observation: &LaneObservation, reason: ReasonCode) -> OwnedLaneDecision {
	decision(
		observation,
		OwnedLaneAction::ManualInterventionRequired,
		PolicyState::HumanAttentionRequired,
		"resolve_policy_stop_before_mutating_lane",
		reason,
		vec![intent(observation, CommandIntentKind::RequestManualIntervention, reason)],
		vec![DecisionBlocker::new(reason)],
	)
}

fn decision(
	observation: &LaneObservation,
	action: OwnedLaneAction,
	policy_state: PolicyState,
	lane_control_next_action: &'static str,
	reason: ReasonCode,
	command_intents: Vec<CommandIntent>,
	blockers: Vec<DecisionBlocker>,
) -> OwnedLaneDecision {
	OwnedLaneDecision {
		decision_class: action,
		policy_state,
		lane_state_axes: lane_state_axes(observation, policy_state),
		command_intents,
		projection_hints: ProjectionHints::new(action, lane_control_next_action, reason),
		blockers,
	}
}

fn lane_state_axes(observation: &LaneObservation, policy_state: PolicyState) -> LaneStateAxes {
	let ownership = if policy_state == PolicyState::HumanAttentionRequired {
		OwnershipState::RetainedAttention
	} else if observation.terminalization != TerminalizationState::None {
		OwnershipState::Terminalizing
	} else if observation.run_lease && observation.active_owned_work {
		OwnershipState::LeasedRun
	} else if !observation.run_lease && observation.active_owned_work {
		OwnershipState::OrphanedLiveThread
	} else {
		OwnershipState::Pending
	};

	LaneStateAxes::new(ownership, observation.liveness, policy_state, observation.terminalization)
}

fn intent(
	observation: &LaneObservation,
	kind: CommandIntentKind,
	reason: ReasonCode,
) -> CommandIntent {
	let run_part = observation.run_id.as_deref().unwrap_or("no-run");
	let idempotency_key =
		format!("{}:{run_part}:{}:{}", observation.issue_id, kind.as_str(), reason.as_str());

	CommandIntent::new(
		kind,
		idempotency_key,
		intent_preconditions(kind),
		intent_expected_postconditions(kind),
	)
}

fn intent_preconditions(kind: CommandIntentKind) -> Vec<CommandFact> {
	if kind == CommandIntentKind::RequestManualIntervention {
		return Vec::new();
	}

	let mut preconditions = vec![
		CommandFact::AuthorityComplete,
		CommandFact::IssueStillOwned,
		CommandFact::NoContradictoryAuthority,
	];
	match kind {
		CommandIntentKind::ContinueAttempt => {
			preconditions.push(CommandFact::ActiveOwnedWorkPresent);
		},
		CommandIntentKind::WaitExternal => {
			preconditions.push(CommandFact::ExternalSignalStillPending);
		},
		CommandIntentKind::ScheduleRetry => {
			preconditions.push(CommandFact::RetryBudgetAvailable);
			preconditions.push(CommandFact::NoHumanAttentionSignal);
		},
		CommandIntentKind::ResumeRetainedLane => {
			preconditions.push(CommandFact::RetainedLaneReusable);
		},
		CommandIntentKind::RequestManualIntervention => {},
		CommandIntentKind::LandReadyPullRequest => {
			preconditions.push(CommandFact::PostReviewLifecyclePresent);
			preconditions.push(CommandFact::ReadyToLandPrerequisitesSatisfied);
		},
		CommandIntentKind::FinishTerminalCleanup => {
			preconditions.push(CommandFact::TerminalCleanupPending);
		},
		CommandIntentKind::RequestExternalReview => {
			preconditions.push(CommandFact::PostReviewLifecyclePresent);
			preconditions.push(CommandFact::ReadyToLandPrerequisitesSatisfied);
		},
		CommandIntentKind::ProbeExternalReviewAcknowledgement => {
			preconditions.push(CommandFact::PostReviewLifecyclePresent);
			preconditions.push(CommandFact::ExternalReviewRequestPresent);
		},
		CommandIntentKind::ResendExternalReviewRequest => {
			preconditions.push(CommandFact::PostReviewLifecyclePresent);
			preconditions.push(CommandFact::ExternalReviewAcknowledgementPending);
			preconditions.push(CommandFact::ExternalReviewRequestRetryAvailable);
		},
		CommandIntentKind::StartReviewRepair => {
			preconditions.push(CommandFact::PostReviewLifecyclePresent);
		},
		CommandIntentKind::StartRetainedLanding => {
			preconditions.push(CommandFact::PostReviewLifecyclePresent);
			preconditions.push(CommandFact::ReadyToLandPrerequisitesSatisfied);
		},
		CommandIntentKind::StartRetainedCloseout => {
			preconditions.push(CommandFact::PostReviewLifecyclePresent);
		},
		CommandIntentKind::FinishRetainedCleanup => {
			preconditions.push(CommandFact::PostReviewLifecyclePresent);
			preconditions.push(CommandFact::TerminalCleanupPending);
		},
		CommandIntentKind::SyncReviewOrchestrationMarker => {
			preconditions.push(CommandFact::PostReviewLifecyclePresent);
		},
		CommandIntentKind::ObserveLoopGuardrailCheckpoint => {
			preconditions.push(CommandFact::OpenTrackerBlockersPresent);
		},
		CommandIntentKind::ClearLoopGuardrailCheckpoint => {
			preconditions.push(CommandFact::OpenTrackerBlockersResolved);
		},
	}
	preconditions
}

fn intent_expected_postconditions(kind: CommandIntentKind) -> Vec<CommandFact> {
	match kind {
		CommandIntentKind::ContinueAttempt => vec![CommandFact::ActiveOwnedWorkPresent],
		CommandIntentKind::WaitExternal => vec![CommandFact::ExternalSignalStillPending],
		CommandIntentKind::ScheduleRetry => vec![CommandFact::RetryScheduled],
		CommandIntentKind::ResumeRetainedLane => vec![CommandFact::RetainedLaneResumed],
		CommandIntentKind::RequestManualIntervention => {
			vec![CommandFact::HumanInterventionRecorded]
		},
		CommandIntentKind::LandReadyPullRequest => vec![CommandFact::LandingSequenceStarted],
		CommandIntentKind::FinishTerminalCleanup => vec![CommandFact::TerminalCleanupCompleted],
		CommandIntentKind::RequestExternalReview => vec![CommandFact::ExternalReviewRequested],
		CommandIntentKind::ProbeExternalReviewAcknowledgement => {
			vec![CommandFact::ExternalReviewAcknowledgementObserved]
		},
		CommandIntentKind::ResendExternalReviewRequest => {
			vec![CommandFact::ExternalReviewRequested]
		},
		CommandIntentKind::StartReviewRepair => vec![CommandFact::ReviewRepairStarted],
		CommandIntentKind::StartRetainedLanding => vec![CommandFact::RetainedLandingStarted],
		CommandIntentKind::StartRetainedCloseout => vec![CommandFact::RetainedCloseoutStarted],
		CommandIntentKind::FinishRetainedCleanup => vec![CommandFact::RetainedCleanupCompleted],
		CommandIntentKind::SyncReviewOrchestrationMarker => {
			vec![CommandFact::ReviewOrchestrationMarkerCurrent]
		},
		CommandIntentKind::ObserveLoopGuardrailCheckpoint => {
			vec![CommandFact::LoopGuardrailCheckpointObserved]
		},
		CommandIntentKind::ClearLoopGuardrailCheckpoint => {
			vec![CommandFact::LoopGuardrailCheckpointCleared]
		},
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::orchestrator::kernel::state::{LivenessState, TerminalizationState};

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

		let decision = decide_owned_lane(&observation);

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

		let decision = decide_owned_lane(&observation);

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

		let decision = decide_owned_lane(&observation);

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

		let decision = decide_owned_lane(&observation);

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

		let decision = decide_owned_lane(&observation);

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

		let decision = decide_owned_lane(&observation);

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

		let decision = decide_owned_lane(&observation);

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

		let decision = decide_owned_lane(&observation);

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

		let decision = decide_owned_lane(&observation);

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
}
