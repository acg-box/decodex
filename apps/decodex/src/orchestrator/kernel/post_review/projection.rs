use crate::orchestrator::{
	PostReviewLaneDecision,
	kernel::{
		action::OwnedLaneAction,
		decision::{self, OwnedLaneDecision},
		facts::LaneObservation,
		post_review::{command, model::PostReviewLaneKernelInput},
		reason::ReasonCode,
		state::TerminalizationState,
	},
};

pub(crate) fn decide_post_review_lane(input: &PostReviewLaneKernelInput<'_>) -> OwnedLaneDecision {
	let mut observation = LaneObservation::for_issue(input.issue_id);

	observation.run_id = input.run_id.map(str::to_owned);
	observation.authority_complete = true;
	observation.post_review_lifecycle_required = true;
	observation.post_review_lifecycle_present = input.lifecycle_present;
	observation.retry_budget_exhausted = input.retry_budget_exhausted;

	match input.proposed_decision {
		PostReviewLaneDecision::ReadyToLand => {
			observation.ready_to_land = true;
		},
		PostReviewLaneDecision::NeedsReviewRepair => {
			observation.retained_lane_reusable = true;
		},
		PostReviewLaneDecision::WaitForReview => {
			observation.external_signal_pending = true;
		},
		PostReviewLaneDecision::Continue => {
			if command::post_review_reason_is_cleanup(input.reason) {
				observation.terminalization = TerminalizationState::CleanupPending;
			} else {
				observation.active_owned_work = true;
			}
		},
		PostReviewLaneDecision::CloseoutBlocked
		| PostReviewLaneDecision::CleanupBlocked
		| PostReviewLaneDecision::Block => {
			observation.human_attention_signal = true;
		},
	}

	let mut decision = decision::decide_owned_lane(&observation);

	decision.command_intents = command::post_review_command_intents(input, &decision);

	decision
}

pub(crate) fn project_post_review_lane_decision(
	input: &PostReviewLaneKernelInput<'_>,
	decision: &OwnedLaneDecision,
) -> PostReviewLaneDecision {
	match decision.decision_class {
		OwnedLaneAction::ReadyToLand => PostReviewLaneDecision::ReadyToLand,
		OwnedLaneAction::ResumeRetainedLane => PostReviewLaneDecision::NeedsReviewRepair,
		OwnedLaneAction::WaitForExternalSignal => PostReviewLaneDecision::WaitForReview,
		OwnedLaneAction::Continue => input.proposed_decision,
		OwnedLaneAction::RetryAutomatically => input.proposed_decision,
		OwnedLaneAction::ManualInterventionRequired => {
			if decision.projection_hints.primary_reason == ReasonCode::PostReviewLifecycleMissing {
				PostReviewLaneDecision::Block
			} else {
				match input.proposed_decision {
					PostReviewLaneDecision::CloseoutBlocked => {
						PostReviewLaneDecision::CloseoutBlocked
					},
					PostReviewLaneDecision::CleanupBlocked => {
						PostReviewLaneDecision::CleanupBlocked
					},
					_ => PostReviewLaneDecision::Block,
				}
			}
		},
	}
}
