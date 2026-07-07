use crate::{
	orchestrator,
	orchestrator::retained_review_orchestration::phases::RetainedReviewLifecycleAction,
	orchestrator::retained_review_orchestration::{
		self, CommandIntentKind, IssueTracker, PullRequestReviewState, Result,
		RetainedAdminMergeReasons, RetainedReviewLane, RetainedReviewLifecycleAuthorityFields,
		RetainedReviewRuntime, ReviewLifecycleReadback, admin_merge, attention,
		lifecycle_authority,
	},
	orchestrator::runtime_standard_review::RuntimeStandardReviewRunner,
};

pub(in crate::orchestrator::retained_review_orchestration::phases) fn handle_waiting_for_result_phase<
	T,
>(
	runtime: &mut RetainedReviewRuntime<'_, T>,
	lane: &RetainedReviewLane,
	action: RetainedReviewLifecycleAction,
	runtime_review_runner: &impl RuntimeStandardReviewRunner,
) -> Result<()>
where
	T: IssueTracker,
{
	if external_review_requires_repair(&lane.review_state, lane.lifecycle_record()) {
		return lifecycle_authority::write_retained_review_lifecycle_authority_for_command(
			runtime.state_store,
			lane,
			CommandIntentKind::StartReviewRepair,
			"external_review_feedback_pending_repair",
			"repair_required",
			RetainedReviewLifecycleAuthorityFields {
				external_round_count: lane
					.lifecycle_record()
					.external_round_count()
					.saturating_add(1),
				..RetainedReviewLifecycleAuthorityFields::from_lifecycle_record(
					lane.lifecycle_record(),
				)
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
		return lifecycle_authority::write_retained_review_lifecycle_authority_for_command(
			runtime.state_store,
			lane,
			CommandIntentKind::StartReviewRepair,
			"required_checks_or_merge_state_repair_required",
			"repair_required",
			RetainedReviewLifecycleAuthorityFields::from_lifecycle_record(lane.lifecycle_record()),
		);
	}
	if orchestrator::external_review_has_strict_pass_signals(
		&lane.review_state,
		lane.lifecycle_record(),
	) {
		if orchestrator::review_state_clean_path_landing_gates_satisfied(&lane.review_state) {
			if super::non_github::runtime_standard_review_gate_requires_wait_or_repair(
				runtime.tracker,
				runtime.project,
				runtime.workflow,
				runtime.state_store,
				lane,
				runtime_review_runner,
			)? {
				return Ok(());
			}

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
			return lifecycle_authority::write_retained_review_lifecycle_authority_for_command(
				runtime.state_store,
				lane,
				CommandIntentKind::StartReviewRepair,
				"retained_landing_agent_fallback_required",
				"repair_required",
				RetainedReviewLifecycleAuthorityFields::from_lifecycle_record(
					lane.lifecycle_record(),
				),
			);
		}
		if action == RetainedReviewLifecycleAction::WaitForExternalReviewResult {
			return lifecycle_authority::write_retained_review_lifecycle_authority_for_command(
				runtime.state_store,
				lane,
				CommandIntentKind::WaitExternal,
				"external_review_passed_waiting_for_gates",
				"pass_waiting_for_gates",
				RetainedReviewLifecycleAuthorityFields::from_lifecycle_record(
					lane.lifecycle_record(),
				),
			);
		}

		return Ok(());
	}
	if orchestrator::external_review_result_arrived(&lane.review_state, lane.lifecycle_record()) {
		return retained_review_orchestration::apply_passive_retained_manual_attention(
			attention::passive_attention_runtime(runtime),
			&lane.snapshot.issue,
			&lane.snapshot.worktree,
			lane.lifecycle_record(),
			"external_review_pass_signal_missing",
		);
	}

	Ok(())
}

pub(in crate::orchestrator::retained_review_orchestration::phases) fn external_review_requires_repair(
	review_state: &PullRequestReviewState,
	marker: &impl ReviewLifecycleReadback,
) -> bool {
	review_state.unresolved_review_threads > 0
		|| matches!(review_state.review_decision.as_deref(), Some("CHANGES_REQUESTED"))
		|| orchestrator::external_review_has_actionable_feedback(review_state, marker)
}
