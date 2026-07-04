use crate::{
	orchestrator,
	orchestrator::retained_review_orchestration::{
		self, CommandIntentKind, EXTERNAL_REVIEW_ACK_TIMEOUT_SECS, EXTERNAL_REVIEW_REQUEST_BODY,
		ExternalReviewRequestCiGate, IssueTracker, PassiveRetainedAttentionRuntime, Result,
		RetainedReviewLane, RetainedReviewOrchestrationMarkerFields, ReviewOrchestrationPhase,
		ServiceConfig, StateStore, WorkflowDocument, admin_merge, github, markers,
	},
};

pub(in crate::orchestrator::retained_review_orchestration::phases) fn handle_request_pending_phase(
	project: &ServiceConfig,
	state_store: &StateStore,
	lane: &RetainedReviewLane,
	github_token: &mut Option<String>,
) -> Result<()> {
	match orchestrator::external_review_request_ci_gate(&lane.review_state) {
		ExternalReviewRequestCiGate::Ready => {},
		ExternalReviewRequestCiGate::WaitForGreenChecks => return Ok(()),
		ExternalReviewRequestCiGate::RepairRequired => {
			return markers::write_retained_review_orchestration_marker_for_command(
				state_store,
				lane,
				CommandIntentKind::StartReviewRepair,
				"external_review_request_ci_red_repair_required",
				ReviewOrchestrationPhase::RepairRequired,
				RetainedReviewOrchestrationMarkerFields::from_marker(&lane.orchestration_marker),
			);
		},
	}

	let (comment_id, created_at_unix_epoch) = post_external_review_request_for_command(
		project,
		lane,
		github_token,
		CommandIntentKind::RequestExternalReview,
		"external_review_request_pending",
	)?;

	markers::write_retained_review_orchestration_marker_for_command(
		state_store,
		lane,
		CommandIntentKind::RequestExternalReview,
		"external_review_request_pending",
		ReviewOrchestrationPhase::WaitingForAck,
		RetainedReviewOrchestrationMarkerFields {
			request_comment_database_id: Some(comment_id),
			request_created_at_unix_epoch: Some(created_at_unix_epoch),
			request_retry_count: 0,
			..RetainedReviewOrchestrationMarkerFields::from_marker(&lane.orchestration_marker)
		},
	)
}

pub(in crate::orchestrator::retained_review_orchestration::phases) fn handle_waiting_for_ack_phase<
	T,
>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	lane: &RetainedReviewLane,
	github_token: &mut Option<String>,
	now_unix_epoch: i64,
) -> Result<()>
where
	T: IssueTracker,
{
	if orchestrator::request_comment_has_eyes(&lane.review_state, &lane.orchestration_marker)
		.unwrap_or(false)
	{
		return markers::write_retained_review_orchestration_marker_for_command(
			state_store,
			lane,
			CommandIntentKind::ProbeExternalReviewAcknowledgement,
			"external_review_acknowledged",
			ReviewOrchestrationPhase::WaitingForResult,
			RetainedReviewOrchestrationMarkerFields {
				..RetainedReviewOrchestrationMarkerFields::from_marker(&lane.orchestration_marker)
			},
		);
	}

	let Some(request_created_at_unix_epoch) =
		lane.orchestration_marker.request_created_at_unix_epoch()
	else {
		return Ok(());
	};

	if now_unix_epoch - request_created_at_unix_epoch <= EXTERNAL_REVIEW_ACK_TIMEOUT_SECS {
		return Ok(());
	}
	if lane.orchestration_marker.request_retry_count() == 0 {
		let (comment_id, created_at_unix_epoch) = post_external_review_request_for_command(
			project,
			lane,
			github_token,
			CommandIntentKind::ResendExternalReviewRequest,
			"external_review_ack_pending",
		)?;

		return markers::write_retained_review_orchestration_marker_for_command(
			state_store,
			lane,
			CommandIntentKind::ResendExternalReviewRequest,
			"external_review_ack_pending",
			ReviewOrchestrationPhase::WaitingForAck,
			RetainedReviewOrchestrationMarkerFields {
				request_comment_database_id: Some(comment_id),
				request_created_at_unix_epoch: Some(created_at_unix_epoch),
				request_retry_count: 1,
				..RetainedReviewOrchestrationMarkerFields::from_marker(&lane.orchestration_marker)
			},
		);
	}

	retained_review_orchestration::apply_passive_retained_manual_attention(
		PassiveRetainedAttentionRuntime { tracker, project, workflow, state_store },
		&lane.snapshot.issue,
		&lane.snapshot.worktree,
		&lane.orchestration_marker,
		"external_review_ack_timeout",
	)
}

fn post_external_review_request_for_command(
	project: &ServiceConfig,
	lane: &RetainedReviewLane,
	github_token: &mut Option<String>,
	kind: CommandIntentKind,
	reason: &str,
) -> Result<(i64, i64)> {
	retained_review_orchestration::retained_review_command_adapter(
		retained_review_orchestration::retained_review_command_intent(lane, kind, reason),
		kind,
	)?;

	let github_token = admin_merge::retained_review_github_token(project, github_token)?;

	github::post_pull_request_issue_comment(
		lane.snapshot.worktree.worktree_path(),
		lane.review_state.url.as_str(),
		EXTERNAL_REVIEW_REQUEST_BODY,
		github_token,
		project.github().command_path(),
	)
}
