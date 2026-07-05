use crate::agent::tracker_tool_bridge::{
	self, ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME, ISSUE_REVIEW_HANDOFF_TOOL_NAME,
	ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME, ReviewExecutionMode, ReviewHandoffContext,
	ReviewPolicyPhase, ReviewPolicyStatus, TrackerToolBridge,
};

impl<'a> TrackerToolBridge<'a> {
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
