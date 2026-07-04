use crate::{
	orchestrator,
	orchestrator::retained_review_orchestration::{
		self, CommandIntentKind, IssueTracker, PullRequestReviewState, Result,
		RetainedAdminMergeReasons, RetainedReviewLane, RetainedReviewOrchestrationMarkerFields,
		RetainedReviewRuntime, ReviewOrchestrationMarker, ReviewOrchestrationPhase, admin_merge,
		attention, markers,
	},
};

pub(in crate::orchestrator::retained_review_orchestration::phases) fn handle_waiting_for_result_phase<
	T,
>(
	runtime: &mut RetainedReviewRuntime<'_, T>,
	lane: &RetainedReviewLane,
	phase: ReviewOrchestrationPhase,
) -> Result<()>
where
	T: IssueTracker,
{
	if external_review_requires_repair(&lane.review_state, &lane.orchestration_marker) {
		return markers::write_retained_review_orchestration_marker_for_command(
			runtime.state_store,
			lane,
			CommandIntentKind::StartReviewRepair,
			"external_review_feedback_pending_repair",
			ReviewOrchestrationPhase::RepairRequired,
			RetainedReviewOrchestrationMarkerFields {
				external_round_count: lane
					.orchestration_marker
					.external_round_count()
					.saturating_add(1),
				..RetainedReviewOrchestrationMarkerFields::from_marker(&lane.orchestration_marker)
			},
		);
	}
	if orchestrator::failed_checks_require_repair(
		lane.review_state.status_check_rollup_state.as_deref(),
		&lane.review_state.merge_state_status,
	) || orchestrator::merge_state_requires_review_repair(
		&lane.review_state.mergeable,
		&lane.review_state.merge_state_status,
	)
	.is_some()
	{
		return markers::write_retained_review_orchestration_marker_for_command(
			runtime.state_store,
			lane,
			CommandIntentKind::StartReviewRepair,
			"required_checks_or_merge_state_repair_required",
			ReviewOrchestrationPhase::RepairRequired,
			RetainedReviewOrchestrationMarkerFields::from_marker(&lane.orchestration_marker),
		);
	}
	if orchestrator::external_review_has_strict_pass_signals(
		&lane.review_state,
		&lane.orchestration_marker,
	) {
		if orchestrator::review_state_clean_path_landing_gates_satisfied(&lane.review_state) {
			return admin_merge::start_retained_admin_merge(
				runtime,
				lane,
				RetainedAdminMergeReasons {
					start_landing: "external_review_passed_strict",
					admin_merge_unavailable: "external_review_admin_merge_unavailable",
					admin_merge_failed: "external_review_admin_merge_failed",
				},
			);
		}
		if orchestrator::review_state_landing_requires_agent_fallback(&lane.review_state) {
			return markers::write_retained_review_orchestration_marker_for_command(
				runtime.state_store,
				lane,
				CommandIntentKind::StartReviewRepair,
				"retained_landing_agent_fallback_required",
				ReviewOrchestrationPhase::RepairRequired,
				RetainedReviewOrchestrationMarkerFields::from_marker(&lane.orchestration_marker),
			);
		}
		if phase == ReviewOrchestrationPhase::WaitingForResult {
			return markers::write_retained_review_orchestration_marker_for_command(
				runtime.state_store,
				lane,
				CommandIntentKind::WaitExternal,
				"external_review_passed_waiting_for_gates",
				ReviewOrchestrationPhase::PassWaitingForGates,
				RetainedReviewOrchestrationMarkerFields::from_marker(&lane.orchestration_marker),
			);
		}

		return Ok(());
	}
	if orchestrator::external_review_result_arrived(&lane.review_state, &lane.orchestration_marker)
	{
		return retained_review_orchestration::apply_passive_retained_manual_attention(
			attention::passive_attention_runtime(runtime),
			&lane.snapshot.issue,
			&lane.snapshot.worktree,
			&lane.orchestration_marker,
			"external_review_pass_signal_missing",
		);
	}

	Ok(())
}

pub(in crate::orchestrator::retained_review_orchestration::phases) fn external_review_requires_repair(
	review_state: &PullRequestReviewState,
	marker: &ReviewOrchestrationMarker,
) -> bool {
	review_state.unresolved_review_threads > 0
		|| matches!(review_state.review_decision.as_deref(), Some("CHANGES_REQUESTED"))
		|| orchestrator::external_review_has_actionable_feedback(review_state, marker)
}
