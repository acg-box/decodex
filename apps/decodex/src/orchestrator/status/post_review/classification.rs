#[allow(clippy::wildcard_imports)] use super::*;

#[cfg_attr(not(test), allow(dead_code))]
#[cfg(test)]
pub(in crate::orchestrator) fn classify_post_review_lane<I>(
	snapshot: &PostReviewLaneSnapshot,
	state_store: &StateStore,
	workflow: &WorkflowDocument,
	review_state_inspector: &I,
) -> crate::prelude::Result<PostReviewLaneClassification>
where
	I: PullRequestReviewStateInspector,
{
	classify_post_review_lane_with_external_review(
		snapshot,
		workflow,
		review_state_inspector,
		true,
		Some(PostReviewRuntimeState {
			state_store,
			project_id: "pubfi",
			review_level: ReviewLevel::Standard,
		}),
	)
}

pub(in crate::orchestrator) fn classify_post_review_lane_with_project<I>(
	snapshot: &PostReviewLaneSnapshot,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	review_state_inspector: &I,
) -> crate::prelude::Result<PostReviewLaneClassification>
where
	I: PullRequestReviewStateInspector,
{
	let mut classification = classify_post_review_lane_with_external_review(
		snapshot,
		workflow,
		review_state_inspector,
		project.codex().review_level().uses_github_review(),
		Some(PostReviewRuntimeState {
			state_store,
			project_id: project.service_id(),
			review_level: project.codex().review_level(),
		}),
	)?;

	confirm_status_visible_merged_closeout(snapshot, project, &mut classification);

	Ok(classification)
}

pub(in crate::orchestrator) fn classify_post_review_lane_with_external_review<I>(
	snapshot: &PostReviewLaneSnapshot,
	workflow: &WorkflowDocument,
	review_state_inspector: &I,
	github_review_enabled: bool,
	runtime_state: Option<PostReviewRuntimeState<'_>>,
) -> crate::prelude::Result<PostReviewLaneClassification>
where
	I: PullRequestReviewStateInspector,
{
	let review_state = match load_post_review_lane_review_state(snapshot, review_state_inspector)? {
		PostReviewLaneStateLoad::Classification(classification) => return Ok(classification),
		PostReviewLaneStateLoad::ReviewState(review_state) => review_state,
	};
	let mut classification = initial_post_review_lane_classification(&review_state);

	if apply_pre_orchestration_post_review_classification(
		snapshot,
		workflow,
		&review_state,
		&mut classification,
	) {
		return Ok(classification);
	}
	if !github_review_enabled {
		let orchestration_marker = load_post_review_orchestration_marker(
			snapshot,
			&review_state,
			&mut classification,
			runtime_state,
		)?;

		if classification.decision == PostReviewLaneDecision::Block {
			return Ok(classification);
		}

		apply_non_github_review_post_review_classification(
			&mut classification,
			&review_state,
			orchestration_marker.as_ref(),
			OffsetDateTime::now_utc().unix_timestamp(),
		)?;
		apply_authority_boundary_landing_policy(snapshot, &mut classification, runtime_state)?;

		return Ok(classification);
	}

	let Some(orchestration_marker) = load_post_review_orchestration_marker(
		snapshot,
		&review_state,
		&mut classification,
		runtime_state,
	)?
	else {
		return Ok(classification);
	};
	let orchestration_status =
		PostReviewOrchestrationStatus::from_review_state(&review_state, &orchestration_marker)?;

	apply_review_orchestration_phase_classification(
		&mut classification,
		&review_state,
		&orchestration_marker,
		&orchestration_status,
		OffsetDateTime::now_utc().unix_timestamp(),
	);
	apply_authority_boundary_landing_policy(snapshot, &mut classification, runtime_state)?;

	Ok(classification)
}
