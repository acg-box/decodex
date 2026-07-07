use crate::{
	pull_request::PullRequestLandingState,
	state::{ReviewLifecycleRecord, WorktreeMapping},
};

pub(in crate::recovery) struct HandoffBindingDiagnostic {
	pub(in crate::recovery) classification: String,
	pub(in crate::recovery) reason: String,
	pub(in crate::recovery) pr_base_ref: Option<String>,
	pub(in crate::recovery) pr_head_oid: Option<String>,
	pub(in crate::recovery) mismatched_field: Option<String>,
	pub(in crate::recovery) next_action: String,
}

pub(in crate::recovery) struct HandoffDiagnosticRequest<'a> {
	pub(in crate::recovery) service_id: &'a str,
	pub(in crate::recovery) issue_identifier: &'a str,
	pub(in crate::recovery) issue_state_name: &'a str,
	pub(in crate::recovery) success_state: &'a str,
	pub(in crate::recovery) in_progress_state: &'a str,
	pub(in crate::recovery) failure_state: &'a str,
	pub(in crate::recovery) worktree: &'a WorktreeMapping,
	pub(in crate::recovery) existing_lifecycle: Option<&'a ReviewLifecycleRecord>,
	pub(in crate::recovery) local_branch_name: Option<&'a str>,
	pub(in crate::recovery) local_head_oid: Option<&'a str>,
	pub(in crate::recovery) worktree_clean: Option<bool>,
	pub(in crate::recovery) pr_inspection: Option<&'a PullRequestLandingState>,
	pub(in crate::recovery) active_label_present: Option<bool>,
}

pub(in crate::recovery) struct HandoffDiagnosticContext<'a> {
	pub(in crate::recovery) issue_identifier: &'a str,
	pub(in crate::recovery) worktree: &'a WorktreeMapping,
	pub(in crate::recovery) existing_lifecycle: &'a ReviewLifecycleRecord,
	pub(in crate::recovery) local_branch_name: Option<&'a str>,
	pub(in crate::recovery) local_head_oid: Option<&'a str>,
	pub(in crate::recovery) worktree_clean: Option<bool>,
}
