use crate::{
	recovery::stale_active_labels::StaleActiveLabelSnapshot, state::ProjectRunStatus,
	tracker::TrackerIssue, workflow::WorkflowTracker,
};

pub(in crate::recovery) struct StaleActiveReleaseReentryInput<'a> {
	pub(in crate::recovery) run: Option<&'a ProjectRunStatus>,
	pub(in crate::recovery) run_lease: bool,
	pub(in crate::recovery) active_shared_claim: bool,
	pub(in crate::recovery) labels: &'a StaleActiveLabelSnapshot,
	pub(in crate::recovery) issue: &'a TrackerIssue,
	pub(in crate::recovery) tracker_policy: &'a WorkflowTracker,
	pub(in crate::recovery) worktree_state: &'a str,
	pub(in crate::recovery) control_channel: &'a str,
}

pub(in crate::recovery::stale_active_reentry) struct StaleActiveStartableStateRestoreReentryInput<
	'a,
> {
	pub(in crate::recovery::stale_active_reentry) run: Option<&'a ProjectRunStatus>,
	pub(in crate::recovery::stale_active_reentry) run_lease: bool,
	pub(in crate::recovery::stale_active_reentry) active_shared_claim: bool,
	pub(in crate::recovery::stale_active_reentry) queue_label_present: bool,
	pub(in crate::recovery::stale_active_reentry) active_label_present: bool,
	pub(in crate::recovery::stale_active_reentry) needs_attention_label_present: bool,
	pub(in crate::recovery::stale_active_reentry) issue_state: &'a str,
	pub(in crate::recovery::stale_active_reentry) in_progress_state: &'a str,
	pub(in crate::recovery::stale_active_reentry) startable_state_id_present: bool,
	pub(in crate::recovery::stale_active_reentry) worktree_state: &'a str,
	pub(in crate::recovery::stale_active_reentry) control_channel: &'a str,
}

pub(in crate::recovery::stale_active_reentry) struct StaleActiveLocalCleanupReentryInput<'a> {
	pub(in crate::recovery::stale_active_reentry) run: Option<&'a ProjectRunStatus>,
	pub(in crate::recovery::stale_active_reentry) run_lease: bool,
	pub(in crate::recovery::stale_active_reentry) active_shared_claim: bool,
	pub(in crate::recovery::stale_active_reentry) queue_label_present: bool,
	pub(in crate::recovery::stale_active_reentry) active_label_present: bool,
	pub(in crate::recovery::stale_active_reentry) needs_attention_label_present: bool,
	pub(in crate::recovery::stale_active_reentry) worktree_state: &'a str,
	pub(in crate::recovery::stale_active_reentry) control_channel: &'a str,
}
