use crate::orchestrator::{
	PostReviewLaneDecision,
	kernel::{
		action::OwnedLaneAction,
		command::{CommandFact, CommandIntentKind},
		post_review::{self, PostReviewLaneKernelInput},
		reason::ReasonCode,
	},
};

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
	let decision = post_review::decide_post_review_lane(&input);

	assert_eq!(decision.decision_class, OwnedLaneAction::ReadyToLand);
	assert_eq!(
		post_review::project_post_review_lane_decision(&input, &decision),
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
	let input =
		input(PostReviewLaneDecision::NeedsReviewRepair, "external_review_feedback_pending_repair");
	let decision = post_review::decide_post_review_lane(&input);

	assert_eq!(decision.decision_class, OwnedLaneAction::ResumeRetainedLane);
	assert_eq!(
		post_review::project_post_review_lane_decision(&input, &decision),
		PostReviewLaneDecision::NeedsReviewRepair,
	);
	assert_eq!(decision.command_intents[0].kind, CommandIntentKind::StartReviewRepair);
}

#[test]
fn missing_lifecycle_fails_closed() {
	let mut input = input(PostReviewLaneDecision::ReadyToLand, "external_review_passed_strict");

	input.lifecycle_present = false;

	let decision = post_review::decide_post_review_lane(&input);

	assert_eq!(decision.decision_class, OwnedLaneAction::ManualInterventionRequired);
	assert_eq!(decision.projection_hints.primary_reason, ReasonCode::PostReviewLifecycleMissing,);
	assert_eq!(
		post_review::project_post_review_lane_decision(&input, &decision),
		PostReviewLaneDecision::Block,
	);
}

#[test]
fn external_review_request_maps_to_request_command() {
	let input = input(PostReviewLaneDecision::WaitForReview, "external_review_request_pending");
	let decision = post_review::decide_post_review_lane(&input);

	assert_eq!(
		post_review::project_post_review_lane_decision(&input, &decision),
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
	let decision = post_review::decide_post_review_lane(&input);

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
	let closeout = input(PostReviewLaneDecision::Continue, "pull_request_merged_closeout_pending");
	let cleanup = input(PostReviewLaneDecision::Continue, "post_land_cleanup_pending");
	let closeout_decision = post_review::decide_post_review_lane(&closeout);
	let cleanup_decision = post_review::decide_post_review_lane(&cleanup);

	assert_eq!(closeout_decision.decision_class, OwnedLaneAction::Continue);
	assert_eq!(closeout_decision.command_intents[0].kind, CommandIntentKind::StartRetainedCloseout,);
	assert_eq!(cleanup_decision.decision_class, OwnedLaneAction::Continue);
	assert_eq!(cleanup_decision.command_intents[0].kind, CommandIntentKind::FinishRetainedCleanup,);
}

#[test]
fn retained_review_command_builder_preserves_idempotency_key_and_contract() {
	let intent = post_review::build_post_review_command_intent(
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
	let intent = post_review::build_post_review_command_intent(
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
	assert_eq!(intent.expected_postconditions, vec![CommandFact::ReviewOrchestrationMarkerCurrent],);
}
