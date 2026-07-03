use crate::{
	agent::tracker_tool_bridge::{
		NormalizedProgressCheckpoint, ReviewHandoffContext, TrackerToolBridge,
	},
	state::StateStore,
};

impl<'a> TrackerToolBridge<'a> {
	pub(super) fn progress_checkpoint_context(
		&self,
	) -> Result<(&ReviewHandoffContext, &StateStore), String> {
		let review_context = self.review_context.as_ref().ok_or_else(|| {
			String::from("`issue_progress_checkpoint` requires an active Decodex run context.")
		})?;
		let state_store = self.state_store.ok_or_else(|| {
			format!(
				"`issue_progress_checkpoint` requires the Decodex runtime state store for issue `{}`.",
				self.issue.identifier
			)
		})?;

		Ok((review_context, state_store))
	}

	pub(super) fn append_private_progress_checkpoint(
		&self,
		review_context: &ReviewHandoffContext,
		state_store: &StateStore,
		checkpoint: &NormalizedProgressCheckpoint,
	) -> Result<(), String> {
		let branch = checkpoint.public_branch(review_context);
		let private_payload = serde_json::json!({
				"phase": checkpoint.phase.as_str(),
				"docs_impact": checkpoint.docs_impact.as_str(),
				"focus": checkpoint.focus.as_str(),
			"next_action": checkpoint.next_action.as_str(),
			"blockers": &checkpoint.blockers,
			"evidence": &checkpoint.evidence,
			"verification": &checkpoint.verification,
			"head_sha": checkpoint.head_sha.as_deref(),
			"branch": branch.as_str(),
			"worktree_path": review_context.worktree_path.as_str(),
			"pr_url": checkpoint.pr_url.as_deref(),
		});

		state_store
			.append_private_execution_event(
				&review_context.service_id,
				&self.issue.id,
				&review_context.run_id,
				review_context.attempt_number,
				"progress_checkpoint",
				private_payload,
			)
			.map(|_| ())
			.map_err(|error| {
				format!(
					"Failed to persist the private execution-state checkpoint for issue `{}`: {error}",
					self.issue.identifier
				)
			})
	}
}
