use crate::agent::tracker_tool_bridge::{
	self, ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME, LocalRepoDetails, ReviewHandoffContext,
	TrackerToolBridge,
};

impl<'a> TrackerToolBridge<'a> {
	pub(in crate::agent::tracker_tool_bridge) fn canonicalize_current_lane_head_sha(
		&self,
		tool_name: &str,
		head_sha: &str,
		current_head_sha: &str,
	) -> std::result::Result<String, String> {
		let head_sha = head_sha.trim();

		if head_sha.is_empty() {
			return Err(format!("`{tool_name}` requires a non-empty `head_sha`."));
		}
		if head_sha == current_head_sha {
			return Ok(current_head_sha.to_owned());
		}
		if head_sha.len() >= 7 && current_head_sha.starts_with(head_sha) {
			return Ok(current_head_sha.to_owned());
		}

		Err(format!(
			"`{tool_name}` head `{head_sha}` does not match the current lane HEAD `{current_head_sha}`."
		))
	}

	pub(in crate::agent::tracker_tool_bridge) fn current_local_repo_details(
		&self,
		review_context: &ReviewHandoffContext,
	) -> std::result::Result<LocalRepoDetails, String> {
		self.local_repo_inspector.inspect_local_repo(&review_context.cwd)
	}

	pub(in crate::agent::tracker_tool_bridge) fn resolve_progress_checkpoint_head_sha(
		&self,
		head_sha: Option<String>,
	) -> std::result::Result<Option<String>, String> {
		let normalized_head_sha = tracker_tool_bridge::normalize_optional_progress_field(head_sha);
		let Some(review_context) = self.review_context.as_ref() else {
			return Ok(normalized_head_sha);
		};
		let local_repo = self.current_local_repo_details(review_context)?;

		match normalized_head_sha {
			Some(head_sha) => self
				.canonicalize_current_lane_head_sha(
					ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
					&head_sha,
					&local_repo.head_oid,
				)
				.map(Some),
			None => Ok(Some(local_repo.head_oid)),
		}
	}
}
