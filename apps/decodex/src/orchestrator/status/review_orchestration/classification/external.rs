use crate::orchestrator::status::{
	self, EXTERNAL_REVIEW_MERGE_VISIBILITY_TIMEOUT_SECS, ExternalReviewRequestCiGate,
	PostReviewLaneClassification, PostReviewLaneDecision, PostReviewLifecycleAction,
	PostReviewOrchestrationStatus, PullRequestReviewState,
};
use crate::state::ReviewLifecycleRecord;

pub(crate) fn apply_review_orchestration_phase_classification(
	classification: &mut PostReviewLaneClassification,
	review_state: &PullRequestReviewState,
	lifecycle_record: &ReviewLifecycleRecord,
	orchestration_status: &PostReviewOrchestrationStatus,
	now_unix_epoch: i64,
) {
	match orchestration_status.action {
		PostReviewLifecycleAction::StartReviewGateOrExternalReview
		| PostReviewLifecycleAction::RequestExternalReview => {
			match status::external_review_request_ci_gate(review_state) {
				ExternalReviewRequestCiGate::Ready => {
					classification.reason = String::from("external_review_request_pending");
				},
				ExternalReviewRequestCiGate::WaitForGreenChecks => {
					classification.reason =
						String::from("external_review_request_waiting_for_green_checks");
				},
				ExternalReviewRequestCiGate::RepairRequired => {
					classification.decision = PostReviewLaneDecision::NeedsReviewRepair;
					classification.reason =
						String::from("external_review_request_ci_red_repair_required");
				},
			}
		},
		PostReviewLifecycleAction::WaitForExternalReviewAck => {
			if orchestration_status.request_acknowledged {
				classification.reason = String::from("external_review_result_pending");
			} else if status::request_ack_timed_out(lifecycle_record, now_unix_epoch) {
				*classification = status::blocked_post_review_lane_from_state(
					review_state,
					"external_review_ack_timeout",
				);
			} else {
				classification.reason = String::from("external_review_ack_pending");
			}
		},
		PostReviewLifecycleAction::WaitForExternalReviewResult => {
			if !orchestration_status.request_acknowledged {
				classification.reason = String::from("external_review_ack_pending");
			} else if status::external_review_has_actionable_feedback(
				review_state,
				lifecycle_record,
			) {
				classification.decision = PostReviewLaneDecision::NeedsReviewRepair;
				classification.reason = String::from("external_review_feedback_pending_repair");
			} else if orchestration_status.strict_pass
				&& orchestration_status.clean_path_landing_gates_satisfied
			{
				classification.decision = PostReviewLaneDecision::ReadyToLand;
				classification.reason = String::from("external_review_passed_strict");
			} else if orchestration_status.strict_pass
				&& orchestration_status.landing_requires_agent_fallback
			{
				classification.decision = PostReviewLaneDecision::NeedsReviewRepair;
				classification.reason = String::from("retained_landing_agent_fallback_required");
			} else if orchestration_status.strict_pass {
				classification.reason = String::from("external_review_passed_waiting_gates");
			} else if orchestration_status.review_result_arrived {
				*classification = status::blocked_post_review_lane_from_state(
					review_state,
					"external_review_pass_signal_missing",
				);
			} else {
				classification.reason = String::from("external_review_result_pending");
			}
		},
		PostReviewLifecycleAction::RunReviewRepair => {
			classification.decision = PostReviewLaneDecision::NeedsReviewRepair;
			classification.reason = if orchestration_status.landing_requires_agent_fallback {
				String::from("retained_landing_agent_fallback_required")
			} else {
				String::from("external_review_feedback_pending_repair")
			};
		},
		PostReviewLifecycleAction::WaitForLandingGates => {
			if status::external_review_has_actionable_feedback(review_state, lifecycle_record) {
				classification.decision = PostReviewLaneDecision::NeedsReviewRepair;
				classification.reason = String::from("external_review_feedback_pending_repair");
			} else if orchestration_status.strict_pass
				&& orchestration_status.clean_path_landing_gates_satisfied
			{
				classification.decision = PostReviewLaneDecision::ReadyToLand;
				classification.reason = String::from("external_review_passed_strict");
			} else if orchestration_status.strict_pass
				&& orchestration_status.landing_requires_agent_fallback
			{
				classification.decision = PostReviewLaneDecision::NeedsReviewRepair;
				classification.reason = String::from("retained_landing_agent_fallback_required");
			} else if orchestration_status.strict_pass {
				classification.reason = String::from("external_review_passed_waiting_gates");
			} else {
				*classification = status::blocked_post_review_lane_from_state(
					review_state,
					"external_review_pass_signal_missing",
				);
			}
		},
		PostReviewLifecycleAction::PollLandingReadback
		| PostReviewLifecycleAction::RunCloseoutAdapter => {
			if let Some(auto_merge_enabled_at) = lifecycle_record.auto_merge_enabled_at_unix_epoch()
				&& now_unix_epoch - auto_merge_enabled_at
					> EXTERNAL_REVIEW_MERGE_VISIBILITY_TIMEOUT_SECS
			{
				*classification = status::blocked_post_review_lane_from_state(
					review_state,
					"external_review_merge_visibility_timeout",
				);
			} else {
				classification.reason = String::from("external_review_waiting_for_merge");
			}
		},
		PostReviewLifecycleAction::RequestManualAttention => {
			*classification = status::blocked_post_review_lane_from_state(
				review_state,
				"request_manual_attention",
			);
		},
	}
}
