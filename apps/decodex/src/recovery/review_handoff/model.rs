use std::path::PathBuf;

use crate::{
	pull_request::PullRequestLandingState,
	recovery::review_handoff_policy::{RebindMode, RebindSuccessStateTransition},
	state::WorktreeMapping,
	tracker::TrackerIssue,
};

pub(in crate::recovery) struct RebindValidation {
	pub(in crate::recovery) issue: TrackerIssue,
	pub(in crate::recovery) worktree: WorktreeMapping,
	pub(in crate::recovery) run_id: String,
	pub(in crate::recovery) attempt_number: i64,
	pub(in crate::recovery) landing_state: PullRequestLandingState,
	pub(in crate::recovery) local_head_oid: String,
	pub(in crate::recovery) worktree_path_for_event: Option<String>,
	pub(in crate::recovery) active_label_present: bool,
	pub(in crate::recovery) restore_active_label: bool,
	pub(in crate::recovery) mode: RebindMode,
	pub(in crate::recovery) success_state_transition: Option<RebindSuccessStateTransition>,
	pub(in crate::recovery) clear_needs_attention_label: bool,
}
impl RebindValidation {
	pub(in crate::recovery) fn should_restore_active_label(&self) -> bool {
		self.restore_active_label
	}
}

pub(in crate::recovery) struct AdoptValidation {
	pub(in crate::recovery) issue: TrackerIssue,
	pub(in crate::recovery) branch_name: String,
	pub(in crate::recovery) worktree_path: PathBuf,
	pub(in crate::recovery) run_id: String,
	pub(in crate::recovery) attempt_number: i64,
	pub(in crate::recovery) landing_state: PullRequestLandingState,
	pub(in crate::recovery) local_head_oid: String,
	pub(in crate::recovery) worktree_path_for_event: Option<String>,
	pub(in crate::recovery) active_label_present: bool,
	pub(in crate::recovery) success_state_transition: Option<RebindSuccessStateTransition>,
	pub(in crate::recovery) previous_worktree_mapping: Option<WorktreeMapping>,
}
impl AdoptValidation {
	pub(in crate::recovery) fn should_restore_active_label(&self) -> bool {
		!self.active_label_present
	}
}

pub(in crate::recovery) struct RebindLabelValidation {
	pub(in crate::recovery) active_label_present: bool,
	pub(in crate::recovery) restore_active_label: bool,
	pub(in crate::recovery) clear_needs_attention_label: bool,
}
