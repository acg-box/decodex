mod axes;
mod intent;
mod model;

pub(crate) use self::model::{DecisionBlocker, OwnedLaneDecision};

use crate::orchestrator::kernel::{
	action::OwnedLaneAction,
	command::{CommandIntent, CommandIntentKind},
	facts::LaneObservation,
	projection::ProjectionHints,
	reason::ReasonCode,
	state::{PolicyState, TerminalizationState},
};

pub(crate) fn decide_owned_lane(observation: &LaneObservation) -> OwnedLaneDecision {
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
			vec![intent::intent(
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
			vec![intent::intent(
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
			vec![intent::intent(
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
			vec![intent::intent(
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
			vec![intent::intent(
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
			vec![intent::intent(
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
		vec![intent::intent(observation, CommandIntentKind::RequestManualIntervention, reason)],
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
		lane_state_axes: axes::lane_state_axes(observation, policy_state),
		command_intents,
		projection_hints: ProjectionHints::new(action, lane_control_next_action, reason),
		blockers,
	}
}

#[cfg(test)]
mod tests;
