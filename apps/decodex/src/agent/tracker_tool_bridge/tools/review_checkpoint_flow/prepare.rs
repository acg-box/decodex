use crate::agent::tracker_tool_bridge::{
	self, ISSUE_REVIEW_CHECKPOINT_TOOL_NAME, LocalRepoDetails, ReviewCheckpointArgs,
	ReviewHandoffContext, ReviewPolicyPhase, ReviewPolicyStatus, TrackerToolBridge,
	tools::{review_checkpoint, review_checkpoint_flow::PreparedReviewCheckpoint},
};

impl<'a> TrackerToolBridge<'a> {
	pub(in crate::agent::tracker_tool_bridge::tools::review_checkpoint_flow) fn prepare_review_checkpoint(
		&self,
		parsed: ReviewCheckpointArgs,
		review_context: &ReviewHandoffContext,
	) -> Result<PreparedReviewCheckpoint, String> {
		let Some(review_policy_phase) = ReviewPolicyPhase::for_mode(review_context.mode) else {
			return Err(String::from(
				"`issue_review_checkpoint` is unavailable for retained closeout runs.",
			));
		};
		let review_policy_status = ReviewPolicyStatus::parse(&parsed.status)?;
		let local_repo = self.current_local_repo_details(review_context)?;
		let head_sha = self.canonicalize_current_lane_head_sha(
			ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
			parsed.head_sha.as_str(),
			&local_repo.head_oid,
		)?;

		self.ensure_review_checkpoint_committed_head(&local_repo)?;

		let mut checkpoint_payload = review_checkpoint::normalize_review_checkpoint_payload(
			parsed,
			review_policy_phase,
			review_policy_status,
			&head_sha,
			&local_repo,
		)?;
		let policy_update = self.review_checkpoint_finding_policy_update(
			review_context,
			review_policy_phase,
			review_policy_status,
			&head_sha,
			&checkpoint_payload,
		)?;

		review_checkpoint::validate_review_cost_control_policy_state(
			&checkpoint_payload.review_cost_control,
			&policy_update,
		)?;

		checkpoint_payload.finding_policy = policy_update.finding_policy;

		Ok(PreparedReviewCheckpoint {
			review_policy_phase,
			review_policy_status,
			head_sha,
			checkpoint_payload,
			nonclean_rounds: policy_update.nonclean_rounds,
		})
	}

	fn ensure_review_checkpoint_committed_head(
		&self,
		local_repo: &LocalRepoDetails,
	) -> Result<(), String> {
		if local_repo.review_worktree_clean() {
			return Ok(());
		}

		Err(format!(
			"`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires a clean committed lane HEAD before recording formal Decodex Review evidence. Commit or revert review-blocking local changes, rerun required validation, then request review for the committed HEAD. Review-blocking local changes: {}",
			tracker_tool_bridge::summarize_review_blocking_changes(
				&local_repo.review_blocking_changes
			)
		))
	}
}
