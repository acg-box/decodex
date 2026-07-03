#[cfg(test)] use crate::orchestrator::ReviewLevel;
use crate::{
	orchestrator::status::{
		post_review,
		post_review::{
			OffsetDateTime, PostReviewLaneClassification, PostReviewLaneDecision,
			PostReviewLaneKernelInput, PostReviewLaneSnapshot, PostReviewLaneStateLoad,
			PostReviewOrchestrationStatus, PostReviewRuntimeState, PullRequestReviewStateInspector,
			ReviewHandoffMarker, ServiceConfig, StateStore, WorkflowDocument,
		},
	},
	prelude::Result,
};

#[cfg_attr(not(test), allow(dead_code))]
#[cfg(test)]
pub(crate) fn classify_post_review_lane<I>(
	snapshot: &PostReviewLaneSnapshot,
	state_store: &StateStore,
	workflow: &WorkflowDocument,
	review_state_inspector: &I,
) -> Result<PostReviewLaneClassification>
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

pub(crate) fn classify_post_review_lane_with_project<I>(
	snapshot: &PostReviewLaneSnapshot,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	review_state_inspector: &I,
) -> Result<PostReviewLaneClassification>
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

	post_review::confirm_status_visible_merged_closeout(snapshot, project, &mut classification);

	Ok(classification)
}

pub(crate) fn classify_post_review_lane_with_external_review<I>(
	snapshot: &PostReviewLaneSnapshot,
	workflow: &WorkflowDocument,
	review_state_inspector: &I,
	github_review_enabled: bool,
	runtime_state: Option<PostReviewRuntimeState<'_>>,
) -> Result<PostReviewLaneClassification>
where
	I: PullRequestReviewStateInspector,
{
	let review_state =
		match post_review::load_post_review_lane_review_state(snapshot, review_state_inspector)? {
			PostReviewLaneStateLoad::Classification(classification) => {
				return Ok(finalize_post_review_lane_classification(snapshot, classification));
			},
			PostReviewLaneStateLoad::ReviewState(review_state) => review_state,
		};
	let mut classification = post_review::initial_post_review_lane_classification(&review_state);

	if post_review::apply_pre_orchestration_post_review_classification(
		snapshot,
		workflow,
		&review_state,
		&mut classification,
	) {
		return Ok(finalize_post_review_lane_classification(snapshot, classification));
	}
	if !github_review_enabled {
		let orchestration_marker = post_review::load_post_review_orchestration_marker(
			snapshot,
			&review_state,
			&mut classification,
			runtime_state,
		)?;

		if classification.decision == PostReviewLaneDecision::Block {
			return Ok(finalize_post_review_lane_classification(snapshot, classification));
		}

		post_review::apply_non_github_review_post_review_classification(
			&mut classification,
			&review_state,
			orchestration_marker.as_ref(),
			OffsetDateTime::now_utc().unix_timestamp(),
		)?;
		post_review::apply_authority_boundary_landing_policy(
			snapshot,
			&mut classification,
			runtime_state,
		)?;

		return Ok(finalize_post_review_lane_classification(snapshot, classification));
	}

	let Some(orchestration_marker) = post_review::load_post_review_orchestration_marker(
		snapshot,
		&review_state,
		&mut classification,
		runtime_state,
	)?
	else {
		return Ok(finalize_post_review_lane_classification(snapshot, classification));
	};
	let orchestration_status =
		PostReviewOrchestrationStatus::from_review_state(&review_state, &orchestration_marker)?;

	post_review::apply_review_orchestration_phase_classification(
		&mut classification,
		&review_state,
		&orchestration_marker,
		&orchestration_status,
		OffsetDateTime::now_utc().unix_timestamp(),
	);
	post_review::apply_authority_boundary_landing_policy(
		snapshot,
		&mut classification,
		runtime_state,
	)?;

	Ok(finalize_post_review_lane_classification(snapshot, classification))
}

pub(crate) fn finalize_post_review_lane_classification(
	snapshot: &PostReviewLaneSnapshot,
	classification: PostReviewLaneClassification,
) -> PostReviewLaneClassification {
	finalize_post_review_lane_classification_with_retry_budget(snapshot, classification, false)
}

pub(crate) fn finalize_post_review_lane_classification_with_retry_budget(
	snapshot: &PostReviewLaneSnapshot,
	mut classification: PostReviewLaneClassification,
	retry_budget_exhausted: bool,
) -> PostReviewLaneClassification {
	let run_id = snapshot.review_handoff.as_ref().map(ReviewHandoffMarker::run_id);
	let input = PostReviewLaneKernelInput {
		issue_id: &snapshot.issue.id,
		run_id,
		lifecycle_present: snapshot.review_handoff.is_some(),
		proposed_decision: classification.decision,
		reason: classification.reason.as_str(),
		retry_budget_exhausted,
	};
	let decision = post_review::decide_post_review_lane(&input);

	classification.decision = post_review::project_post_review_lane_decision(&input, &decision);

	classification
}
