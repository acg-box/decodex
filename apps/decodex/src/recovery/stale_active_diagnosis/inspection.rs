mod authority;
mod dead_ownership;
mod diagnostic;
mod inputs;
mod reentry;
mod sources;

use std::path::Path;

use crate::{
	prelude::Result,
	recovery::{
		context::RecoveryRuntimeMutationPolicy,
		process_liveness::{self, StaleActiveProcessLiveness},
		reports::StaleActiveDiagnostic,
		stale_active_labels::{self},
		stale_active_runtime, stale_active_worktree,
	},
	state::{ProjectRunStatus, StateStore},
	tracker::{IssueTracker, TrackerIssue},
	workflow::WorkflowDocument,
};
use inputs::{
	StaleActiveAuthorityEvidenceInspection, StaleActiveDeadOwnershipInput,
	StaleActiveDiagnosticParts, StaleActiveReleaseReentryInspection,
};

pub(super) fn inspect_stale_active_issue<T>(
	project_id: &str,
	workflow: &WorkflowDocument,
	worktree_root: &Path,
	state_store: &StateStore,
	tracker: &T,
	issue: TrackerIssue,
	listing_mode: RecoveryRuntimeMutationPolicy,
) -> Result<StaleActiveDiagnostic>
where
	T: IssueTracker + ?Sized,
{
	let mut evidence = vec![String::from("tracker_issue_present")];
	let mut blockers = Vec::new();
	let issue_keys = stale_active_labels::stale_active_tracker_issue_keys(&issue);
	let labels = stale_active_labels::inspect_stale_active_labels(
		project_id,
		workflow,
		tracker,
		&issue,
		&mut evidence,
		&mut blockers,
	)?;
	let active_shared_claim = stale_active_labels::inspect_stale_active_shared_claim(
		project_id,
		state_store,
		&issue_keys,
		&mut evidence,
		&mut blockers,
	);
	let runs = stale_active_runtime::stale_active_runs(
		project_id,
		state_store,
		&issue_keys,
		listing_mode,
	)?;
	let latest_run = stale_active_runtime::latest_stale_active_run(&runs);
	let run_lease = runs.iter().any(ProjectRunStatus::run_lease);

	sources::record_stale_active_run_lease_evidence(run_lease, &mut evidence, &mut blockers);

	let mapping = sources::read_stale_active_worktree_mapping(
		state_store,
		&issue_keys,
		&mut evidence,
		&mut blockers,
	);
	let worktree_path = mapping
		.as_ref()
		.map(|mapping| mapping.worktree_path().to_path_buf())
		.unwrap_or_else(|| worktree_root.join(&issue.identifier));
	let marker =
		sources::read_stale_active_activity_marker(&worktree_path, &mut evidence, &mut blockers);
	let marker_liveness =
		process_liveness::stale_active_optional_marker_process_liveness(marker.as_ref());

	inspect_stale_active_dead_ownership_and_runs(
		StaleActiveDeadOwnershipInput {
			project_id,
			state_store,
			issue_keys: &issue_keys,
			marker: marker.as_ref(),
			marker_liveness,
			latest_run,
			run_lease,
			active_shared_claim,
		},
		&runs,
		marker_liveness,
		&mut evidence,
		&mut blockers,
	);

	let worktree_state = stale_active_worktree::inspect_stale_active_worktree(
		&worktree_path,
		mapping.as_ref(),
		marker.as_ref(),
		marker_liveness,
		&mut evidence,
		&mut blockers,
	);
	let control_channel = stale_active_runtime::inspect_stale_active_control_channel(
		latest_run,
		&runs,
		marker_liveness,
		&mut evidence,
		&mut blockers,
	);

	authority::inspect_stale_active_authority_evidence(
		StaleActiveAuthorityEvidenceInspection {
			project_id,
			state_store,
			tracker,
			issue: &issue,
			issue_keys: &issue_keys,
			latest_run,
			marker_liveness,
		},
		&mut evidence,
		&mut blockers,
	)?;
	reentry::apply_stale_active_release_reentry(
		StaleActiveReleaseReentryInspection {
			latest_run,
			run_lease,
			active_shared_claim,
			labels: &labels,
			issue: &issue,
			workflow,
			worktree_state: &worktree_state,
			control_channel: &control_channel,
		},
		&mut evidence,
		&mut blockers,
	);

	Ok(diagnostic::stale_active_diagnostic_from_parts(StaleActiveDiagnosticParts {
		project_id,
		issue,
		labels,
		latest_run,
		run_lease,
		active_shared_claim,
		control_channel,
		worktree_path: &worktree_path,
		worktree_state,
		evidence,
		blockers,
	}))
}

fn inspect_stale_active_dead_ownership_and_runs(
	dead_ownership_input: StaleActiveDeadOwnershipInput<'_>,
	runs: &[ProjectRunStatus],
	marker_liveness: StaleActiveProcessLiveness,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) {
	dead_ownership::record_recoverable_dead_leased_ownership(
		dead_ownership_input,
		evidence,
		blockers,
	);
	stale_active_runtime::inspect_stale_active_run_evidence(
		runs,
		marker_liveness,
		evidence,
		blockers,
	);
}
