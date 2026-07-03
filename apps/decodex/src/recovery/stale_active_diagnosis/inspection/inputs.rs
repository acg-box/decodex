use std::path::Path;

use crate::{
	recovery::{
		process_liveness::StaleActiveProcessLiveness, stale_active_labels::StaleActiveLabelSnapshot,
	},
	state::{ProjectRunStatus, RunActivityMarker, StateStore},
	tracker::{IssueTracker, TrackerIssue},
	workflow::WorkflowDocument,
};

pub(super) struct StaleActiveDiagnosticParts<'a> {
	pub(super) project_id: &'a str,
	pub(super) issue: TrackerIssue,
	pub(super) labels: StaleActiveLabelSnapshot,
	pub(super) latest_run: Option<&'a ProjectRunStatus>,
	pub(super) run_lease: bool,
	pub(super) active_shared_claim: bool,
	pub(super) control_channel: String,
	pub(super) worktree_path: &'a Path,
	pub(super) worktree_state: String,
	pub(super) evidence: Vec<String>,
	pub(super) blockers: Vec<String>,
}

pub(super) struct StaleActiveDeadOwnershipInput<'a> {
	pub(super) project_id: &'a str,
	pub(super) state_store: &'a StateStore,
	pub(super) issue_keys: &'a [String],
	pub(super) marker: Option<&'a RunActivityMarker>,
	pub(super) marker_liveness: StaleActiveProcessLiveness,
	pub(super) latest_run: Option<&'a ProjectRunStatus>,
	pub(super) run_lease: bool,
	pub(super) active_shared_claim: bool,
}

pub(super) struct StaleActiveAuthorityEvidenceInspection<'a, T>
where
	T: IssueTracker + ?Sized,
{
	pub(super) project_id: &'a str,
	pub(super) state_store: &'a StateStore,
	pub(super) tracker: &'a T,
	pub(super) issue: &'a TrackerIssue,
	pub(super) issue_keys: &'a [String],
	pub(super) latest_run: Option<&'a ProjectRunStatus>,
	pub(super) marker_liveness: StaleActiveProcessLiveness,
}

pub(super) struct StaleActiveReleaseReentryInspection<'a> {
	pub(super) latest_run: Option<&'a ProjectRunStatus>,
	pub(super) run_lease: bool,
	pub(super) active_shared_claim: bool,
	pub(super) labels: &'a StaleActiveLabelSnapshot,
	pub(super) issue: &'a TrackerIssue,
	pub(super) workflow: &'a WorkflowDocument,
	pub(super) worktree_state: &'a str,
	pub(super) control_channel: &'a str,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct StaleActiveDeadLocalClaims {
	pub(super) matching_claim_count: usize,
	pub(super) incompatible_claim_present: bool,
}
