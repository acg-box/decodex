use std::{
	error::Error,
	fmt::{Display, Formatter},
	path::PathBuf,
};

use crate::config::ReviewLevel;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ReviewHandoffContext {
	pub(crate) attempt_number: i64,
	pub(crate) branch_name: String,
	pub(crate) run_id: String,
	pub(crate) service_id: String,
	pub(crate) worktree_path: String,
	pub(crate) cwd: PathBuf,
	pub(crate) github_token_env_var: Option<String>,
	pub(crate) github_command_path: Option<PathBuf>,
	pub(crate) review_level: ReviewLevel,
	pub(crate) mode: ReviewExecutionMode,
	pub(crate) recorded_pr_url: Option<String>,
}
impl ReviewHandoffContext {
	pub(crate) fn decodex_review_checkpoint_enabled(&self) -> bool {
		self.review_level.requires_review_checkpoint()
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ReviewHandoffWritebackFailed {
	pub(crate) issue_identifier: String,
	pub(crate) run_id: String,
	pub(crate) pr_url: String,
	pub(crate) success_state: String,
	pub(crate) source: String,
}
impl Display for ReviewHandoffWritebackFailed {
	fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
		write!(
			f,
			"Run `{}` failed to finalize the review handoff for issue `{}` around target state `{}` and PR `{}`: {}",
			self.run_id, self.issue_identifier, self.success_state, self.pr_url, self.source
		)
	}
}

impl Error for ReviewHandoffWritebackFailed {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PullRequestDetails {
	pub(in crate::agent::tracker_tool_bridge) base_ref_name: String,
	pub(in crate::agent::tracker_tool_bridge) head_ref_name: String,
	pub(in crate::agent::tracker_tool_bridge) head_ref_oid: String,
	pub(in crate::agent::tracker_tool_bridge) head_repository_name: String,
	pub(in crate::agent::tracker_tool_bridge) head_repository_owner: String,
	pub(in crate::agent::tracker_tool_bridge) is_draft: bool,
	pub(in crate::agent::tracker_tool_bridge) state: String,
	pub(in crate::agent::tracker_tool_bridge) url: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LocalRepoDetails {
	pub(in crate::agent::tracker_tool_bridge) default_branch: String,
	pub(in crate::agent::tracker_tool_bridge) head_oid: String,
	pub(in crate::agent::tracker_tool_bridge) head_tree_oid: String,
	pub(in crate::agent::tracker_tool_bridge) repository_name: String,
	pub(in crate::agent::tracker_tool_bridge) repository_owner: String,
	pub(in crate::agent::tracker_tool_bridge) review_blocking_changes: Vec<String>,
}
impl LocalRepoDetails {
	pub(in crate::agent::tracker_tool_bridge) fn review_worktree_clean(&self) -> bool {
		self.review_blocking_changes.is_empty()
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReviewExecutionMode {
	Handoff,
	Repair,
	Closeout,
}
impl ReviewExecutionMode {
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::Handoff => "handoff",
			Self::Repair => "repair",
			Self::Closeout => "closeout",
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TurnCompletionStatus {
	Continue,
	Complete,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RunCompletionDisposition {
	ManualAttention,
	ReviewHandoff,
	ReviewRepair,
	Closeout,
}
impl RunCompletionDisposition {
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::ManualAttention => "manual_attention",
			Self::ReviewHandoff => "review_handoff",
			Self::ReviewRepair => "review_repair",
			Self::Closeout => "closeout",
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::agent::tracker_tool_bridge) struct PendingReviewAction {
	pub(in crate::agent::tracker_tool_bridge) pr_url: String,
	pub(in crate::agent::tracker_tool_bridge) summary: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::agent::tracker_tool_bridge) enum PendingReviewCompletion {
	Handoff(PendingReviewAction),
	Repair(PendingReviewAction),
	Closeout(PendingReviewAction),
}
