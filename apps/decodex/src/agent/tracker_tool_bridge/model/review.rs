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
	pub(crate) base_ref_name: String,
	pub(crate) head_ref_name: String,
	pub(crate) head_ref_oid: String,
	pub(crate) head_repository_name: String,
	pub(crate) head_repository_owner: String,
	pub(crate) is_draft: bool,
	pub(crate) state: String,
	pub(crate) url: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LocalRepoDetails {
	pub(crate) default_branch: String,
	pub(crate) head_oid: String,
	pub(crate) head_tree_oid: String,
	pub(crate) repository_name: String,
	pub(crate) repository_owner: String,
	pub(crate) review_blocking_changes: Vec<String>,
}
impl LocalRepoDetails {
	pub(crate) fn review_worktree_clean(&self) -> bool {
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
pub(crate) struct PendingReviewAction {
	pub(crate) pr_url: String,
	pub(crate) summary: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PendingReviewCompletion {
	Handoff(PendingReviewAction),
	Repair(PendingReviewAction),
	Closeout(PendingReviewAction),
}
