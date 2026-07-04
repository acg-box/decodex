use crate::{
	orchestrator,
	orchestrator::retained_review_orchestration::{
		CommandIntentKind, IssueTracker, Result, RetainedAdminMergeReasons, RetainedReviewLane,
		RetainedReviewOrchestrationMarkerFields, RetainedReviewRuntime, ReviewOrchestrationPhase,
		ServiceConfig, StateStore, WorkflowDocument, admin_merge, eyre, markers,
		phases::{merge, result},
	},
};

pub(in crate::orchestrator::retained_review_orchestration::phases) fn handle_non_github_review_lane<
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
	let phase =
		ReviewOrchestrationPhase::parse(lane.orchestration_marker.phase()).map_err(|error| {
			eyre::eyre!("Failed to parse retained review orchestration phase: {error}")
		})?;

	if phase == ReviewOrchestrationPhase::WaitingForMerge {
		return merge::handle_waiting_for_merge_phase(
			tracker,
			project,
			workflow,
			state_store,
			lane,
			now_unix_epoch,
			"non_github_review_merge_visibility_timeout",
		);
	}
	if result::external_review_requires_repair(&lane.review_state, &lane.orchestration_marker)
		|| orchestrator::failed_checks_require_repair(
			lane.review_state.status_check_rollup_state.as_deref(),
			&lane.review_state.merge_state_status,
		) || orchestrator::merge_state_requires_review_repair(
		&lane.review_state.mergeable,
		&lane.review_state.merge_state_status,
	)
	.is_some()
	{
		return markers::write_retained_review_orchestration_marker_for_command(
			state_store,
			lane,
			CommandIntentKind::StartReviewRepair,
			"non_github_review_repair_required",
			ReviewOrchestrationPhase::RepairRequired,
			RetainedReviewOrchestrationMarkerFields::from_marker(&lane.orchestration_marker),
		);
	}
	if orchestrator::review_state_landing_requires_agent_fallback(&lane.review_state) {
		return markers::write_retained_review_orchestration_marker_for_command(
			state_store,
			lane,
			CommandIntentKind::StartReviewRepair,
			"retained_landing_agent_fallback_required",
			ReviewOrchestrationPhase::RepairRequired,
			RetainedReviewOrchestrationMarkerFields::from_marker(&lane.orchestration_marker),
		);
	}
	if !orchestrator::review_state_landing_gates_satisfied(&lane.review_state) {
		return Ok(());
	}

	let mut runtime = RetainedReviewRuntime {
		tracker,
		project,
		workflow,
		state_store,
		now_unix_epoch,
		github_token,
	};

	admin_merge::start_retained_admin_merge(
		&mut runtime,
		lane,
		RetainedAdminMergeReasons {
			start_landing: "non_github_review_ready_to_land",
			admin_merge_unavailable: "non_github_review_admin_merge_unavailable",
			admin_merge_failed: "non_github_review_admin_merge_failed",
		},
	)
}
