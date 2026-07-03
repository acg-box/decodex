//! Apply path for stale-active release recovery.

use std::path::{Path, PathBuf};

use color_eyre::eyre::WrapErr;

use crate::{
	config::ServiceConfig,
	prelude::{Result, eyre},
	recovery::{
		GHOST_LANE_TERMINAL_STATUS, STALE_ACTIVE_RECOVERY_SCHEMA, STALE_ACTIVE_RELEASE_EVENT,
		context::{RecoveryContext, RecoveryRuntimeMutationPolicy},
		git_worktree,
		reports::StaleActiveDiagnostic,
		stale_active_authority,
		stale_active_diagnosis::{self},
		stale_active_labels::{self},
		stale_active_worktree,
	},
	state::{RUN_CONTROL_CHANNEL_STATUS_FAILED, StateStore, WorktreeMapping},
	tracker::{self, IssueTracker, TrackerIssue},
	workflow::WorkflowDocument,
	worktree::WorktreeManager,
};

#[derive(Clone, Debug)]
enum StaleActiveWorktreeCleanup {
	None,
	UnmappedPath(PathBuf),
	Mapped(WorktreeMapping),
}

pub(super) fn apply_stale_active_release(
	context: &RecoveryContext,
	diagnostic: &StaleActiveDiagnostic,
) -> Result<()> {
	apply_stale_active_release_with_tracker(
		&context.tracker,
		&context.config,
		&context.workflow,
		&context.state_store,
		diagnostic,
	)
}

pub(super) fn apply_stale_active_release_with_tracker<T>(
	tracker: &T,
	config: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	diagnostic: &StaleActiveDiagnostic,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	let diagnostic = refreshed_stale_active_release_diagnostic(
		tracker,
		config,
		workflow,
		state_store,
		diagnostic,
	)?;
	let worktree_cleanup = preflight_stale_active_worktree_cleanup_plan(state_store, &diagnostic)?;

	stale_active_authority::ensure_stale_active_review_authority_missing(
		tracker,
		state_store,
		&diagnostic,
	)?;

	let cleared_run_lease =
		clear_stale_active_dead_run_claims_before_release(state_store, &diagnostic)?;

	ensure_stale_active_run_claim_guard(config, state_store, &diagnostic)?;

	let active_label = tracker::automation_active_label(config.service_id());

	if let Some(run_id) = diagnostic.latest_run_id.as_deref()
		&& let Some(attempt_number) = diagnostic.latest_attempt_number
	{
		if diagnostic
			.latest_attempt_status
			.as_deref()
			.is_some_and(stale_active_attempt_status_needs_terminal_guard)
		{
			state_store.update_run_status(run_id, GHOST_LANE_TERMINAL_STATUS)?;
		}

		state_store.retire_run_control_channel_for_attempt(
			run_id,
			attempt_number,
			RUN_CONTROL_CHANNEL_STATUS_FAILED,
		)?;
	}

	stale_active_authority::ensure_stale_active_review_authority_missing(
		tracker,
		state_store,
		&diagnostic,
	)?;

	cleanup_stale_active_worktree_mapping(
		config,
		workflow,
		state_store,
		&diagnostic,
		worktree_cleanup,
	)?;

	if let Some(run_id) = diagnostic.latest_run_id.as_deref()
		&& let Some(attempt_number) = diagnostic.latest_attempt_number
	{
		state_store
			.append_private_execution_event(
				&diagnostic.project_id,
				&diagnostic.issue_id,
				run_id,
				attempt_number,
				STALE_ACTIVE_RELEASE_EVENT,
				serde_json::json!({
					"schema": STALE_ACTIVE_RECOVERY_SCHEMA,
					"event": STALE_ACTIVE_RELEASE_EVENT,
					"phase": "local_cleanup_complete_before_active_label_release",
					"classification": &diagnostic.classification,
					"reason": &diagnostic.reason,
					"issue_identifier": &diagnostic.issue_identifier,
					"terminal_status": GHOST_LANE_TERMINAL_STATUS,
					"active_label_release": "pending_final_mutation",
					"queue_label_preserved": diagnostic.queue_label_present,
					"cleared_run_lease": cleared_run_lease,
					"worktree_state": &diagnostic.worktree_state,
					"evidence": &diagnostic.evidence,
					"blockers": &diagnostic.blockers,
					"next_action": "ordinary automation may continue after status readback confirms no current attention lane",
				}),
			)
			.map(|_| ())?;
	}

	stale_active_authority::ensure_stale_active_review_authority_missing(
		tracker,
		state_store,
		&diagnostic,
	)?;

	ensure_stale_active_run_claim_guard(config, state_store, &diagnostic)?;

	let final_diagnostic = refreshed_stale_active_release_diagnostic(
		tracker,
		config,
		workflow,
		state_store,
		&diagnostic,
	)?;

	stale_active_authority::ensure_stale_active_review_authority_missing(
		tracker,
		state_store,
		&final_diagnostic,
	)?;

	ensure_stale_active_run_claim_guard(config, state_store, &final_diagnostic)?;

	let issue =
		stale_active_diagnosis::lookup_stale_active_issue(tracker, &diagnostic.issue_identifier)?;

	restore_stale_active_startable_state_if_queued(tracker, workflow, &issue, &final_diagnostic)?;

	tracker::set_issue_label_presence(tracker, &issue, &active_label, false)?;

	Ok(())
}

pub(super) fn clear_stale_active_dead_run_claims_before_release(
	state_store: &StateStore,
	diagnostic: &StaleActiveDiagnostic,
) -> Result<bool> {
	if !diagnostic.run_lease
		|| !diagnostic.evidence.iter().any(|evidence| evidence == "stale_run_lease_present")
	{
		return Ok(false);
	}

	let Some(run_id) = diagnostic.latest_run_id.as_deref() else {
		return Ok(false);
	};
	let mut cleared = false;

	for issue_key in stale_active_labels::stale_active_diagnostic_issue_keys(diagnostic) {
		let Some(lease) = state_store.lease_for_issue(&issue_key)? else {
			continue;
		};

		if lease.project_id() == diagnostic.project_id && lease.run_id() == run_id {
			state_store.clear_lease(&issue_key)?;

			cleared = true;
		}
	}

	Ok(cleared)
}

pub(super) fn ensure_stale_active_run_claim_guard(
	config: &ServiceConfig,
	state_store: &StateStore,
	diagnostic: &StaleActiveDiagnostic,
) -> Result<()> {
	let issue_keys = stale_active_labels::stale_active_diagnostic_issue_keys(diagnostic);

	match stale_active_labels::stale_active_issue_has_active_shared_claim(
		config.service_id(),
		state_store,
		&issue_keys,
	) {
		Ok(false) => Ok(()),
		Ok(true) => eyre::bail!(
			"`recover stale-active release` refused `{}` because a run lease or shared claim appeared before active-label release.",
			diagnostic.issue_identifier
		),
		Err(error) => eyre::bail!(
			"`recover stale-active release` refused `{}` because run lease/shared claim state could not be inspected before active-label release: {}",
			diagnostic.issue_identifier,
			error
		),
	}
}

pub(super) fn preflight_stale_active_worktree_cleanup(
	state_store: &StateStore,
	diagnostic: &StaleActiveDiagnostic,
) -> Result<()> {
	preflight_stale_active_worktree_cleanup_plan(state_store, diagnostic).map(|_| ())
}

fn refreshed_stale_active_release_diagnostic<T>(
	tracker: &T,
	config: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	original: &StaleActiveDiagnostic,
) -> Result<StaleActiveDiagnostic>
where
	T: IssueTracker + ?Sized,
{
	let mut diagnostics = stale_active_diagnosis::diagnose_stale_active_issues(
		config.service_id(),
		workflow,
		config.worktree_root(),
		state_store,
		tracker,
		Some(&original.issue_identifier),
		RecoveryRuntimeMutationPolicy::ReadOnly,
	)?;
	let diagnostic = diagnostics.pop().ok_or_else(|| {
		eyre::eyre!("No stale active issue matched `{}`.", original.issue_identifier)
	})?;

	if !diagnostic.recoverable() {
		eyre::bail!(
			"`recover stale-active release` refused `{}` because safety inspection changed before apply: {}",
			original.issue_identifier,
			diagnostic.blockers.join(", ")
		);
	}
	if diagnostic.issue_id != original.issue_id
		|| diagnostic.latest_run_id != original.latest_run_id
		|| diagnostic.latest_attempt_number != original.latest_attempt_number
	{
		eyre::bail!(
			"`recover stale-active release` refused `{}` because the stale ownership target changed before apply.",
			original.issue_identifier
		);
	}

	Ok(diagnostic)
}

fn restore_stale_active_startable_state_if_queued<T>(
	tracker: &T,
	workflow: &WorkflowDocument,
	issue: &TrackerIssue,
	diagnostic: &StaleActiveDiagnostic,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	if !diagnostic.queue_label_present {
		return Ok(());
	}

	let tracker_policy = workflow.frontmatter().tracker();

	if tracker_policy.startable_states().iter().any(|state| state == &issue.state.name) {
		return Ok(());
	}
	if issue.state.name != tracker_policy.in_progress_state() {
		eyre::bail!(
			"`recover stale-active release` refused `{}` because queued issue state `{}` is not `{}` or a configured startable state.",
			diagnostic.issue_identifier,
			issue.state.name,
			tracker_policy.in_progress_state()
		);
	}

	let startable_state = tracker_policy.startable_states().first().ok_or_else(|| {
		eyre::eyre!("Workflow tracker startable_states must contain at least one state.")
	})?;
	let state_id = issue.state_id_for_name(startable_state).ok_or_else(|| {
		eyre::eyre!(
			"Issue `{}` team does not expose configured startable state `{}`.",
			issue.identifier,
			startable_state
		)
	})?;

	tracker.update_issue_state(&issue.id, state_id)
}

fn stale_active_attempt_status_needs_terminal_guard(status: &str) -> bool {
	matches!(
		status,
		"starting" | "running" | "continuation_pending" | "stalled" | "failed" | "interrupted"
	)
}

fn preflight_stale_active_worktree_cleanup_plan(
	state_store: &StateStore,
	diagnostic: &StaleActiveDiagnostic,
) -> Result<StaleActiveWorktreeCleanup> {
	let issue_keys = stale_active_labels::stale_active_diagnostic_issue_keys(diagnostic);
	let Some(mapping) =
		stale_active_worktree::stale_active_worktree_mapping_for_keys(state_store, &issue_keys)?
	else {
		if let Some(worktree_path) = diagnostic.worktree_path.as_deref().map(PathBuf::from)
			&& stale_active_worktree_path_exists_for_cleanup(
				&diagnostic.issue_identifier,
				&worktree_path,
			)? {
			ensure_stale_active_worktree_clean(&diagnostic.issue_identifier, &worktree_path)?;

			return Ok(StaleActiveWorktreeCleanup::UnmappedPath(worktree_path));
		}

		return Ok(StaleActiveWorktreeCleanup::None);
	};

	if stale_active_worktree_path_exists_for_cleanup(
		&diagnostic.issue_identifier,
		mapping.worktree_path(),
	)? {
		ensure_stale_active_worktree_clean(&diagnostic.issue_identifier, mapping.worktree_path())?;

		return Ok(StaleActiveWorktreeCleanup::Mapped(mapping));
	}

	Ok(StaleActiveWorktreeCleanup::None)
}

fn stale_active_worktree_path_exists_for_cleanup(
	issue_identifier: &str,
	worktree_path: &Path,
) -> Result<bool> {
	worktree_path.try_exists().wrap_err_with(|| {
		format!(
			"`recover stale-active release` refused `{}` because retained worktree `{}` could not be inspected before cleanup.",
			issue_identifier,
			worktree_path.display()
		)
	})
}

fn ensure_stale_active_worktree_clean(issue_identifier: &str, worktree_path: &Path) -> Result<()> {
	if git_worktree::worktree_has_tracked_changes_for_recovery(worktree_path)? {
		eyre::bail!(
			"`recover stale-active release` refused `{}` because retained worktree changes appeared before cleanup.",
			issue_identifier
		);
	}

	Ok(())
}

fn cleanup_stale_active_worktree_mapping(
	config: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	diagnostic: &StaleActiveDiagnostic,
	cleanup: StaleActiveWorktreeCleanup,
) -> Result<()> {
	match cleanup {
		StaleActiveWorktreeCleanup::None => {},
		StaleActiveWorktreeCleanup::UnmappedPath(worktree_path) => {
			let worktree_manager = WorktreeManager::new(
				config.service_id(),
				config.repo_root(),
				config.worktree_root(),
			);

			worktree_manager.remove_worktree_path(&worktree_path)?;
		},
		StaleActiveWorktreeCleanup::Mapped(mapping) => {
			let worktree_manager = WorktreeManager::new(
				config.service_id(),
				config.repo_root(),
				config.worktree_root(),
			);

			worktree_manager.remove_worktree_path_with_hooks(
				&diagnostic.issue_identifier,
				mapping.branch_name(),
				mapping.worktree_path(),
				workflow.frontmatter().execution().workspace_hooks(),
			)?;
		},
	};

	state_store.clear_worktree_mapping(&diagnostic.issue_id)?;

	if diagnostic.issue_identifier != diagnostic.issue_id {
		state_store.clear_worktree_mapping(&diagnostic.issue_identifier)?;
	}

	Ok(())
}
