use crate::agent::tracker_tool_bridge::{
	self, ExecutionProgressPhase, NormalizedProgressCheckpoint, ProgressCheckpointArgs,
	TrackerToolBridge,
};

impl<'a> TrackerToolBridge<'a> {
	pub(super) fn normalize_progress_checkpoint(
		&self,
		parsed: ProgressCheckpointArgs,
	) -> Result<NormalizedProgressCheckpoint, String> {
		let phase = ExecutionProgressPhase::parse(&parsed.phase)?;
		let focus = tracker_tool_bridge::normalize_summary(&parsed.focus);
		let next_action = tracker_tool_bridge::normalize_summary(&parsed.next_action);
		let blockers = tracker_tool_bridge::normalize_progress_list(parsed.blockers);
		let evidence = tracker_tool_bridge::normalize_progress_list(parsed.evidence);
		let verification = tracker_tool_bridge::normalize_progress_list(parsed.verification);
		let head_sha = self.resolve_progress_checkpoint_head_sha(parsed.head_sha)?;
		let branch = tracker_tool_bridge::normalize_optional_progress_field(parsed.branch);
		let pr_url = tracker_tool_bridge::normalize_optional_progress_field(parsed.pr_url);

		if focus.is_empty() {
			return Err(String::from("`issue_progress_checkpoint` requires a non-empty `focus`."));
		}
		if next_action.is_empty() {
			return Err(String::from(
				"`issue_progress_checkpoint` requires a non-empty `next_action`.",
			));
		}
		if phase == ExecutionProgressPhase::Blocked && blockers.is_empty() {
			return Err(String::from(
				"`issue_progress_checkpoint` phase `blocked` requires at least one blocker.",
			));
		}

		Ok(NormalizedProgressCheckpoint {
			phase,
			focus,
			next_action,
			blockers,
			evidence,
			verification,
			head_sha,
			branch,
			pr_url,
		})
	}
}
