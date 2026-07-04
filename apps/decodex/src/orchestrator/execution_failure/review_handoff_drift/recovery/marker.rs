use crate::orchestrator::{
	execution_failure::{
		IssueRunPlan, Result, ReviewHandoffMarker, ReviewOrchestrationMarker, ServiceConfig,
		StateStore,
		review_handoff_drift::{command, types::REVIEW_HANDOFF_REBOUND_ORCHESTRATION_PHASE},
	},
	kernel::command::CommandIntentKind,
};

pub(in crate::orchestrator::execution_failure::review_handoff_drift::recovery) fn rebound_review_handoff_orchestration_marker(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	review_handoff: &ReviewHandoffMarker,
	local_head_sha: &str,
) -> Result<bool> {
	let existing_orchestration = state_store.review_orchestration_marker(
		project.service_id(),
		&issue_run.issue.id,
		review_handoff,
	)?;
	let rebounded_orchestration = existing_orchestration.as_ref().is_none_or(|marker| {
		marker.branch_name() != review_handoff.branch_name()
			|| marker.pr_url() != review_handoff.pr_url()
			|| marker.head_sha() != local_head_sha
			|| marker.phase() != REVIEW_HANDOFF_REBOUND_ORCHESTRATION_PHASE
	});
	let orchestration_marker = ReviewOrchestrationMarker::new(
		review_handoff.run_id().to_owned(),
		review_handoff.attempt_number(),
		review_handoff.branch_name().to_owned(),
		review_handoff.pr_url().to_owned(),
		local_head_sha.to_owned(),
		REVIEW_HANDOFF_REBOUND_ORCHESTRATION_PHASE,
		None,
		None,
		None,
		0,
		existing_orchestration.as_ref().map_or(0, ReviewOrchestrationMarker::external_round_count),
		None,
	);

	command::review_handoff_drift_command_adapter(
		command::review_handoff_drift_marker_rebind_command_intent(
			&issue_run.issue.id,
			review_handoff.run_id(),
		),
		CommandIntentKind::SyncReviewOrchestrationMarker,
	)?;

	state_store.upsert_review_orchestration_marker(
		project.service_id(),
		&issue_run.issue.id,
		&orchestration_marker,
	)?;

	Ok(rebounded_orchestration)
}
