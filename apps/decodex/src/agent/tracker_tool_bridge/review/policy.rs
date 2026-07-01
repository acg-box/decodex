use crate::{
	agent::tracker_tool_bridge::{
		self, ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME, ISSUE_REVIEW_HANDOFF_TOOL_NAME,
		ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME, REVIEW_POLICY_CONVERGENCE_BUDGET,
		ReviewExecutionMode, ReviewHandoffContext, ReviewPolicyPhase, ReviewPolicyState,
		ReviewPolicyStatus, ReviewPolicyStopReason, ReviewPolicyStopRequested, TrackerToolBridge,
	},
	prelude::eyre,
	state::{ReviewCheckpointArtifactLookup, ReviewPolicyCheckpoint, ReviewPolicyCheckpointInput},
};

use super::linear_events::review_policy_stop_fingerprint;

impl<'a> TrackerToolBridge<'a> {
	pub(in crate::agent::tracker_tool_bridge) fn persist_review_policy_state(
		&self,
		review_context: &ReviewHandoffContext,
		review_policy_phase: ReviewPolicyPhase,
		review_policy_status: ReviewPolicyStatus,
		head_sha: &str,
		nonclean_rounds: i64,
		details_json: &str,
	) -> Result<(), String> {
		let state_store = self.state_store.ok_or_else(|| {
			format!(
				"Runtime state store is required to persist review checkpoint state for issue `{}`.",
				self.issue.identifier
			)
		})?;

		state_store
			.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
				project_id: &review_context.service_id,
				issue_id: &self.issue.id,
				run_id: &review_context.run_id,
				attempt_number: review_context.attempt_number,
				phase: review_policy_phase.as_str(),
				review_level: review_context.review_level.as_str(),
				status: review_policy_status.as_str(),
				head_sha,
				nonclean_rounds,
				details_json,
			})
			.map_err(|error| {
				format!(
					"Failed to persist review checkpoint state for issue `{}`: {error}",
					self.issue.identifier
				)
			})?;

		Ok(())
	}

	fn review_policy_state_from_checkpoint(
		&self,
		checkpoint: ReviewPolicyCheckpoint,
	) -> crate::prelude::Result<ReviewPolicyState> {
		let phase =
			ReviewPolicyPhase::parse(checkpoint.phase()).map_err(|error| eyre::eyre!(error))?;
		let status =
			ReviewPolicyStatus::parse(checkpoint.status()).map_err(|error| eyre::eyre!(error))?;

		Ok(ReviewPolicyState {
			phase,
			status,
			head_sha: checkpoint.head_sha().to_owned(),
			nonclean_rounds: checkpoint.nonclean_rounds(),
			details_json: checkpoint.details_json().to_owned(),
		})
	}

	pub(in crate::agent::tracker_tool_bridge) fn review_policy_state_for_current_head(
		&self,
		review_context: &ReviewHandoffContext,
	) -> crate::prelude::Result<Option<ReviewPolicyState>> {
		let Some(current_phase) = ReviewPolicyPhase::for_mode(review_context.mode) else {
			return Ok(None);
		};
		let local_repo =
			self.current_local_repo_details(review_context).map_err(|error| eyre::eyre!(error))?;

		self.review_policy_artifact_for_head(review_context, current_phase, &local_repo.head_oid)
	}

	pub(in crate::agent::tracker_tool_bridge) fn review_policy_artifact_for_head(
		&self,
		review_context: &ReviewHandoffContext,
		review_policy_phase: ReviewPolicyPhase,
		head_sha: &str,
	) -> crate::prelude::Result<Option<ReviewPolicyState>> {
		let Some(state_store) = self.state_store else {
			return Ok(None);
		};
		let Some(checkpoint) =
			state_store.review_checkpoint_artifact(ReviewCheckpointArtifactLookup {
				project_id: &review_context.service_id,
				issue_id: &self.issue.id,
				phase: review_policy_phase.as_str(),
				review_level: review_context.review_level.as_str(),
				head_sha,
			})?
		else {
			return Ok(None);
		};

		self.review_policy_state_from_checkpoint(checkpoint).map(Some)
	}

	pub(in crate::agent::tracker_tool_bridge) fn require_clean_review_checkpoint(
		&self,
		review_context: &ReviewHandoffContext,
	) -> std::result::Result<(), String> {
		if !review_context.decodex_review_checkpoint_enabled() {
			return Ok(());
		}

		let local_repo = self.current_local_repo_details(review_context)?;

		if !local_repo.review_worktree_clean() {
			return Err(format!(
				"`{}` requires a clean committed lane HEAD before reusing a Decodex Review checkpoint. Commit or revert review-blocking local changes, rerun required validation, and record a fresh clean checkpoint. Review-blocking local changes: {}",
				self.required_pr_completion_tool_name(),
				tracker_tool_bridge::summarize_review_blocking_changes(
					&local_repo.review_blocking_changes
				)
			));
		}

		let Some(checkpoint) = self
			.review_policy_artifact_for_head(
				review_context,
				ReviewPolicyPhase::for_mode(review_context.mode)
					.expect("review completion should only be available during review phases"),
				&local_repo.head_oid,
			)
			.map_err(|error| error.to_string())?
		else {
			return Err(format!(
				"`{}` requires a current `{}` review checkpoint with status `clean` for the current lane HEAD.",
				self.required_pr_completion_tool_name(),
				ReviewPolicyPhase::for_mode(review_context.mode)
					.expect("review completion should only be available during review phases")
					.as_str(),
			));
		};

		if checkpoint.status != ReviewPolicyStatus::Clean {
			return Err(format!(
				"`{}` requires the latest review checkpoint to be `clean`, not `{}`.",
				self.required_pr_completion_tool_name(),
				checkpoint.status.as_str(),
			));
		}

		Ok(())
	}

	pub(in crate::agent::tracker_tool_bridge) fn review_policy_stop_requested(
		&self,
		review_context: &ReviewHandoffContext,
	) -> crate::prelude::Result<Option<ReviewPolicyStopRequested>> {
		if !review_context.decodex_review_checkpoint_enabled() {
			return Ok(None);
		}

		let Some(current_phase) = ReviewPolicyPhase::for_mode(review_context.mode) else {
			return Ok(None);
		};
		let Some(state_store) = self.state_store else {
			return Ok(None);
		};

		if !state_store.has_nonclean_review_checkpoint_artifact(
			&review_context.service_id,
			&self.issue.id,
			current_phase.as_str(),
		)? {
			return Ok(None);
		}

		let Some(checkpoint) = self.review_policy_state_for_current_head(review_context)? else {
			return Ok(None);
		};

		Ok(self.review_policy_stop_from_checkpoint(review_context, checkpoint))
	}

	fn review_policy_stop_from_checkpoint(
		&self,
		review_context: &ReviewHandoffContext,
		checkpoint: ReviewPolicyState,
	) -> Option<ReviewPolicyStopRequested> {
		let stop_reason = match checkpoint.status {
			ReviewPolicyStatus::Clean => return None,
			ReviewPolicyStatus::Findings
				if checkpoint.nonclean_rounds < REVIEW_POLICY_CONVERGENCE_BUDGET =>
			{
				return None;
			},
			ReviewPolicyStatus::Findings => ReviewPolicyStopReason::Exhausted,
			ReviewPolicyStatus::NeedsArchitectureReview =>
				ReviewPolicyStopReason::ArchitectureReviewRequired,
			ReviewPolicyStatus::Blocked => ReviewPolicyStopReason::Blocked,
		};

		Some(ReviewPolicyStopRequested {
			head_sha: checkpoint.head_sha,
			issue_identifier: self.issue.identifier.clone(),
			fingerprint: review_policy_stop_fingerprint(&checkpoint.details_json),
			nonclean_rounds: Some(checkpoint.nonclean_rounds),
			reason: stop_reason,
			run_id: review_context.run_id.clone(),
		})
	}

	pub(in crate::agent::tracker_tool_bridge) fn required_pr_completion_tool_name(
		&self,
	) -> &'static str {
		match self.review_context.as_ref().map(|context| context.mode) {
			Some(ReviewExecutionMode::Handoff) => ISSUE_REVIEW_HANDOFF_TOOL_NAME,
			Some(ReviewExecutionMode::Repair) => ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME,
			Some(ReviewExecutionMode::Closeout) => ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME,
			None => ISSUE_REVIEW_HANDOFF_TOOL_NAME,
		}
	}
}
