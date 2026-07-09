#[cfg(test)] use crate::orchestrator::ReviewLevel;
use crate::{
	orchestrator::{
		self, PostReviewLifecycleFactsInput, RuntimeReviewGateState,
		status::{
			PullRequestReviewState, post_review,
			post_review::{
				OffsetDateTime, PostReviewLaneClassification, PostReviewLaneDecision,
				PostReviewLaneKernelInput, PostReviewLaneSnapshot, PostReviewLaneStateLoad,
				PostReviewOrchestrationStatus, PostReviewRuntimeState,
				PullRequestReviewStateInspector, ServiceConfig, StateStore, WorkflowDocument,
			},
			review_state,
		},
	},
	prelude::Result,
	state::ReviewLifecycleRecord,
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
		let lifecycle_record = post_review::load_post_review_lifecycle_record(
			snapshot,
			&review_state,
			&mut classification,
			runtime_state,
		)?;

		if classification.decision == PostReviewLaneDecision::Block {
			return Ok(finalize_post_review_lane_classification(snapshot, classification));
		}
		if apply_runtime_standard_review_gate(
			snapshot,
			&review_state,
			&mut classification,
			runtime_state,
			lifecycle_record.as_ref(),
		)? {
			return Ok(finalize_post_review_lane_classification(snapshot, classification));
		}

		post_review::apply_non_github_review_post_review_classification(
			&mut classification,
			&review_state,
			lifecycle_record.as_ref(),
			OffsetDateTime::now_utc().unix_timestamp(),
		)?;
		post_review::apply_authority_boundary_landing_policy(
			snapshot,
			&mut classification,
			runtime_state,
		)?;

		return Ok(finalize_post_review_lane_classification(snapshot, classification));
	}

	let Some(lifecycle_record) = post_review::load_post_review_lifecycle_record(
		snapshot,
		&review_state,
		&mut classification,
		runtime_state,
	)?
	else {
		return Ok(finalize_post_review_lane_classification(snapshot, classification));
	};
	let orchestration_status =
		PostReviewOrchestrationStatus::from_review_state(&review_state, &lifecycle_record)?;

	post_review::apply_review_lifecycle_action_classification(
		&mut classification,
		&review_state,
		&lifecycle_record,
		&orchestration_status,
		OffsetDateTime::now_utc().unix_timestamp(),
	);

	if classification.decision == PostReviewLaneDecision::ReadyToLand
		&& classification.reason == "external_review_passed_strict"
		&& apply_runtime_standard_review_gate(
			snapshot,
			&review_state,
			&mut classification,
			runtime_state,
			Some(&lifecycle_record),
		)? {
		return Ok(finalize_post_review_lane_classification(snapshot, classification));
	}

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
	let run_id = snapshot.lifecycle_record.as_ref().map(ReviewLifecycleRecord::run_id);
	let input = PostReviewLaneKernelInput {
		issue_id: &snapshot.issue.id,
		run_id,
		lifecycle_present: snapshot.lifecycle_record.is_some(),
		proposed_decision: classification.decision,
		reason: classification.reason.as_str(),
		retry_budget_exhausted,
	};
	let decision = post_review::decide_post_review_lane(&input);

	classification.decision = post_review::project_post_review_lane_decision(&input, &decision);

	classification
}

fn apply_runtime_standard_review_gate(
	snapshot: &PostReviewLaneSnapshot,
	review_state: &PullRequestReviewState,
	classification: &mut PostReviewLaneClassification,
	runtime_state: Option<PostReviewRuntimeState<'_>>,
	lifecycle_record: Option<&ReviewLifecycleRecord>,
) -> Result<bool> {
	let Some(runtime_state) = runtime_state else {
		return Ok(false);
	};

	if !runtime_state.review_level.requires_review_checkpoint() {
		return Ok(false);
	}

	let local_head_oid = snapshot.local_head_oid.as_deref();
	let checkpoint = if let Some(local_head_oid) = local_head_oid {
		orchestrator::runtime_review_checkpoint_status_for_head(
			runtime_state.state_store,
			runtime_state.project_id,
			&snapshot.issue.id,
			runtime_state.review_level,
			local_head_oid,
		)?
	} else {
		None
	};
	let facts = orchestrator::build_post_review_lifecycle_facts(PostReviewLifecycleFactsInput {
		project_id: runtime_state.project_id,
		issue_id: &snapshot.issue.id,
		review_lifecycle: lifecycle_record,
		review_state,
		worktree_path: snapshot.worktree.worktree_path(),
		review_level: runtime_state.review_level,
		phase: "handoff",
		landing_state: None,
		closeout_state: None,
		validated_head_sha: local_head_oid,
		review_checkpoint_phase: checkpoint.as_ref().map(|checkpoint| checkpoint.phase),
		review_checkpoint_status: checkpoint.as_ref().map(|checkpoint| checkpoint.status.as_str()),
	});

	match facts.review_gate_state {
		RuntimeReviewGateState::NotRequired => Ok(false),
		RuntimeReviewGateState::Clean => {
			if orchestrator::worktree_has_review_blocking_changes(
				snapshot.worktree.worktree_path(),
			)? {
				*classification = review_state::blocked_post_review_lane_from_state(
					review_state,
					"runtime_standard_review_clean_checkpoint_worktree_dirty",
				);

				return Ok(true);
			}

			Ok(false)
		},
		RuntimeReviewGateState::WorktreeHeadMissing => {
			*classification = review_state::blocked_post_review_lane_from_state(
				review_state,
				"worktree_head_missing",
			);

			Ok(true)
		},
		RuntimeReviewGateState::Pending => {
			classification.decision = PostReviewLaneDecision::WaitForReview;
			classification.reason = String::from("runtime_standard_review_checkpoint_pending");

			Ok(true)
		},
		RuntimeReviewGateState::Findings => {
			classification.decision = PostReviewLaneDecision::NeedsReviewRepair;
			classification.reason = String::from("runtime_standard_review_repair_required");

			Ok(true)
		},
		RuntimeReviewGateState::NeedsArchitectureReview => {
			*classification = review_state::blocked_post_review_lane_from_state(
				review_state,
				"runtime_standard_review_needs_architecture_review",
			);

			Ok(true)
		},
		RuntimeReviewGateState::Blocked => {
			*classification = review_state::blocked_post_review_lane_from_state(
				review_state,
				"runtime_standard_review_blocked",
			);

			Ok(true)
		},
		RuntimeReviewGateState::Unknown(_) => {
			*classification = review_state::blocked_post_review_lane_from_state(
				review_state,
				"runtime_standard_review_unknown_checkpoint_status",
			);

			Ok(true)
		},
	}
}
