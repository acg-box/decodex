use super::{
	action::OwnedLaneAction,
	command::{CommandFact, CommandIntent, CommandIntentKind},
	decision::{OwnedLaneDecision, decide_owned_lane},
	facts::LaneObservation,
	reason::ReasonCode,
	state::TerminalizationState,
};
use crate::orchestrator::PostReviewLaneDecision;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::orchestrator) struct PostReviewLaneKernelInput<'a> {
	pub(in crate::orchestrator) issue_id: &'a str,
	pub(in crate::orchestrator) run_id: Option<&'a str>,
	pub(in crate::orchestrator) lifecycle_present: bool,
	pub(in crate::orchestrator) proposed_decision: PostReviewLaneDecision,
	pub(in crate::orchestrator) reason: &'a str,
	pub(in crate::orchestrator) retry_budget_exhausted: bool,
}

pub(in crate::orchestrator) fn decide_post_review_lane(
	input: &PostReviewLaneKernelInput<'_>,
) -> OwnedLaneDecision {
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
		PostReviewLaneDecision::Continue =>
			if post_review_reason_is_cleanup(input.reason) {
				observation.terminalization = TerminalizationState::CleanupPending;
			} else {
				observation.active_owned_work = true;
			},
		PostReviewLaneDecision::CloseoutBlocked
		| PostReviewLaneDecision::CleanupBlocked
		| PostReviewLaneDecision::Block => {
			observation.human_attention_signal = true;
		},
	}

	let mut decision = decide_owned_lane(&observation);
	decision.command_intents = post_review_command_intents(input, &decision);
	decision
}

pub(in crate::orchestrator) fn project_post_review_lane_decision(
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
					PostReviewLaneDecision::CloseoutBlocked =>
						PostReviewLaneDecision::CloseoutBlocked,
					PostReviewLaneDecision::CleanupBlocked =>
						PostReviewLaneDecision::CleanupBlocked,
					_ => PostReviewLaneDecision::Block,
				}
			}
		},
	}
}

pub(in crate::orchestrator) fn build_post_review_command_intent(
	issue_id: &str,
	run_id: Option<&str>,
	reason: &str,
	kind: CommandIntentKind,
) -> CommandIntent {
	let input = PostReviewLaneKernelInput {
		issue_id,
		run_id,
		lifecycle_present: true,
		proposed_decision: post_review_decision_for_command_kind(kind),
		reason,
		retry_budget_exhausted: false,
	};

	post_review_command_intent(&input, kind)
}

fn post_review_decision_for_command_kind(kind: CommandIntentKind) -> PostReviewLaneDecision {
	match kind {
		CommandIntentKind::RequestExternalReview
		| CommandIntentKind::ProbeExternalReviewAcknowledgement
		| CommandIntentKind::ResendExternalReviewRequest
		| CommandIntentKind::SyncReviewOrchestrationMarker
		| CommandIntentKind::WaitExternal => PostReviewLaneDecision::WaitForReview,
		CommandIntentKind::StartReviewRepair => PostReviewLaneDecision::NeedsReviewRepair,
		CommandIntentKind::StartRetainedLanding | CommandIntentKind::LandReadyPullRequest =>
			PostReviewLaneDecision::ReadyToLand,
		CommandIntentKind::StartRetainedCloseout | CommandIntentKind::FinishRetainedCleanup =>
			PostReviewLaneDecision::Continue,
		_ => PostReviewLaneDecision::Block,
	}
}

fn post_review_command_intents(
	input: &PostReviewLaneKernelInput<'_>,
	decision: &OwnedLaneDecision,
) -> Vec<CommandIntent> {
	if decision.decision_class == OwnedLaneAction::ManualInterventionRequired {
		return decision.command_intents.clone();
	}

	let Some(kind) = post_review_command_kind(input) else {
		return decision.command_intents.clone();
	};

	vec![post_review_command_intent(input, kind)]
}

fn post_review_command_kind(input: &PostReviewLaneKernelInput<'_>) -> Option<CommandIntentKind> {
	match input.proposed_decision {
		PostReviewLaneDecision::ReadyToLand => Some(CommandIntentKind::StartRetainedLanding),
		PostReviewLaneDecision::NeedsReviewRepair => Some(CommandIntentKind::StartReviewRepair),
		PostReviewLaneDecision::WaitForReview => match input.reason {
			"external_review_request_pending" => Some(CommandIntentKind::RequestExternalReview),
			"external_review_ack_pending" =>
				Some(CommandIntentKind::ProbeExternalReviewAcknowledgement),
			_ => Some(CommandIntentKind::WaitExternal),
		},
		PostReviewLaneDecision::Continue =>
			if post_review_reason_is_cleanup(input.reason) {
				Some(CommandIntentKind::FinishRetainedCleanup)
			} else {
				Some(CommandIntentKind::StartRetainedCloseout)
			},
		PostReviewLaneDecision::CloseoutBlocked
		| PostReviewLaneDecision::CleanupBlocked
		| PostReviewLaneDecision::Block => None,
	}
}

fn post_review_command_intent(
	input: &PostReviewLaneKernelInput<'_>,
	kind: CommandIntentKind,
) -> CommandIntent {
	CommandIntent::new(
		kind,
		post_review_idempotency_key(input, kind),
		post_review_command_preconditions(kind),
		post_review_command_postconditions(kind),
	)
}

fn post_review_idempotency_key(
	input: &PostReviewLaneKernelInput<'_>,
	kind: CommandIntentKind,
) -> String {
	let run_id = input.run_id.unwrap_or("no-run");
	format!("{}:{run_id}:{}:{}", input.issue_id, kind.as_str(), input.reason)
}

fn post_review_command_preconditions(kind: CommandIntentKind) -> Vec<CommandFact> {
	let mut preconditions = vec![
		CommandFact::AuthorityComplete,
		CommandFact::IssueStillOwned,
		CommandFact::NoContradictoryAuthority,
		CommandFact::PostReviewLifecyclePresent,
	];

	match kind {
		CommandIntentKind::RequestExternalReview => {
			preconditions.push(CommandFact::ReadyToLandPrerequisitesSatisfied);
		},
		CommandIntentKind::ProbeExternalReviewAcknowledgement => {
			preconditions.push(CommandFact::ExternalReviewRequestPresent);
		},
		CommandIntentKind::ResendExternalReviewRequest => {
			preconditions.push(CommandFact::ExternalReviewAcknowledgementPending);
			preconditions.push(CommandFact::ExternalReviewRequestRetryAvailable);
		},
		CommandIntentKind::StartRetainedLanding => {
			preconditions.push(CommandFact::ReadyToLandPrerequisitesSatisfied);
		},
		CommandIntentKind::FinishRetainedCleanup => {
			preconditions.push(CommandFact::TerminalCleanupPending);
		},
		CommandIntentKind::WaitExternal
		| CommandIntentKind::StartReviewRepair
		| CommandIntentKind::StartRetainedCloseout
		| CommandIntentKind::SyncReviewOrchestrationMarker => {},
		_ => {},
	}

	preconditions
}

fn post_review_command_postconditions(kind: CommandIntentKind) -> Vec<CommandFact> {
	match kind {
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
		CommandIntentKind::WaitExternal => vec![CommandFact::ExternalSignalStillPending],
		_ => Vec::new(),
	}
}

fn post_review_reason_is_cleanup(reason: &str) -> bool {
	reason.contains("cleanup")
}

#[cfg(test)]
mod tests {
	use super::*;

	fn input(
		decision: PostReviewLaneDecision,
		reason: &'static str,
	) -> PostReviewLaneKernelInput<'static> {
		PostReviewLaneKernelInput {
			issue_id: "PUB-101",
			run_id: Some("run-1"),
			lifecycle_present: true,
			proposed_decision: decision,
			reason,
			retry_budget_exhausted: false,
		}
	}

	#[test]
	fn ready_to_land_projects_to_landing_command() {
		let input = input(PostReviewLaneDecision::ReadyToLand, "external_review_passed_strict");

		let decision = decide_post_review_lane(&input);

		assert_eq!(decision.decision_class, OwnedLaneAction::ReadyToLand);
		assert_eq!(
			project_post_review_lane_decision(&input, &decision),
			PostReviewLaneDecision::ReadyToLand
		);
		assert_eq!(decision.command_intents[0].kind, CommandIntentKind::StartRetainedLanding);
		assert!(
			decision.command_intents[0]
				.preconditions
				.contains(&CommandFact::PostReviewLifecyclePresent)
		);
		assert_eq!(
			decision.command_intents[0].expected_postconditions,
			vec![CommandFact::RetainedLandingStarted],
		);
	}

	#[test]
	fn review_repair_projects_to_resume_retained_lane() {
		let input = input(
			PostReviewLaneDecision::NeedsReviewRepair,
			"external_review_feedback_pending_repair",
		);

		let decision = decide_post_review_lane(&input);

		assert_eq!(decision.decision_class, OwnedLaneAction::ResumeRetainedLane);
		assert_eq!(
			project_post_review_lane_decision(&input, &decision),
			PostReviewLaneDecision::NeedsReviewRepair,
		);
		assert_eq!(decision.command_intents[0].kind, CommandIntentKind::StartReviewRepair);
	}

	#[test]
	fn missing_lifecycle_fails_closed() {
		let mut input = input(PostReviewLaneDecision::ReadyToLand, "external_review_passed_strict");
		input.lifecycle_present = false;

		let decision = decide_post_review_lane(&input);

		assert_eq!(decision.decision_class, OwnedLaneAction::ManualInterventionRequired);
		assert_eq!(
			decision.projection_hints.primary_reason,
			ReasonCode::PostReviewLifecycleMissing,
		);
		assert_eq!(
			project_post_review_lane_decision(&input, &decision),
			PostReviewLaneDecision::Block,
		);
	}

	#[test]
	fn external_review_request_maps_to_request_command() {
		let input = input(PostReviewLaneDecision::WaitForReview, "external_review_request_pending");

		let decision = decide_post_review_lane(&input);

		assert_eq!(
			project_post_review_lane_decision(&input, &decision),
			PostReviewLaneDecision::WaitForReview,
		);
		assert_eq!(decision.command_intents[0].kind, CommandIntentKind::RequestExternalReview,);
		assert_eq!(
			decision.command_intents[0].expected_postconditions,
			vec![CommandFact::ExternalReviewRequested],
		);
	}

	#[test]
	fn ack_pending_maps_to_probe_command() {
		let input = input(PostReviewLaneDecision::WaitForReview, "external_review_ack_pending");

		let decision = decide_post_review_lane(&input);

		assert_eq!(
			decision.command_intents[0].kind,
			CommandIntentKind::ProbeExternalReviewAcknowledgement,
		);
		assert!(
			decision.command_intents[0]
				.preconditions
				.contains(&CommandFact::ExternalReviewRequestPresent)
		);
	}

	#[test]
	fn closeout_and_cleanup_are_command_intents_not_actions() {
		let closeout =
			input(PostReviewLaneDecision::Continue, "pull_request_merged_closeout_pending");
		let cleanup = input(PostReviewLaneDecision::Continue, "post_land_cleanup_pending");

		let closeout_decision = decide_post_review_lane(&closeout);
		let cleanup_decision = decide_post_review_lane(&cleanup);

		assert_eq!(closeout_decision.decision_class, OwnedLaneAction::Continue);
		assert_eq!(
			closeout_decision.command_intents[0].kind,
			CommandIntentKind::StartRetainedCloseout,
		);
		assert_eq!(cleanup_decision.decision_class, OwnedLaneAction::Continue);
		assert_eq!(
			cleanup_decision.command_intents[0].kind,
			CommandIntentKind::FinishRetainedCleanup,
		);
	}

	#[test]
	fn retained_review_command_builder_preserves_idempotency_key_and_contract() {
		let intent = build_post_review_command_intent(
			"PUB-101",
			Some("run-1"),
			"external_review_ack_pending",
			CommandIntentKind::ResendExternalReviewRequest,
		);

		assert_eq!(intent.kind, CommandIntentKind::ResendExternalReviewRequest);
		assert_eq!(
			intent.idempotency_key,
			"PUB-101:run-1:resend_external_review_request:external_review_ack_pending",
		);
		assert!(intent.preconditions.contains(&CommandFact::ExternalReviewRequestRetryAvailable));
		assert_eq!(intent.expected_postconditions, vec![CommandFact::ExternalReviewRequested],);
	}

	#[test]
	fn retained_review_marker_sync_builder_preserves_kernel_contract() {
		let intent = build_post_review_command_intent(
			"PUB-101",
			Some("run-1"),
			"review_orchestration_marker_rebound",
			CommandIntentKind::SyncReviewOrchestrationMarker,
		);

		assert_eq!(intent.kind, CommandIntentKind::SyncReviewOrchestrationMarker);
		assert_eq!(
			intent.idempotency_key,
			"PUB-101:run-1:sync_review_orchestration_marker:review_orchestration_marker_rebound",
		);
		assert!(intent.preconditions.contains(&CommandFact::PostReviewLifecyclePresent));
		assert_eq!(
			intent.expected_postconditions,
			vec![CommandFact::ReviewOrchestrationMarkerCurrent],
		);
	}
}
