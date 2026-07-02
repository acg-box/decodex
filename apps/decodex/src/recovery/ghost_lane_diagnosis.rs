//! Diagnostic assembly for missing-issue ghost-lane recovery.

mod inspection;

use std::path::Path;

use crate::{
	prelude::{Result, eyre},
	recovery::{context::RecoveryRuntimeMutationPolicy, identifiers, reports::GhostLaneDiagnostic},
	state::StateStore,
	tracker::IssueTracker,
};

pub(super) fn diagnose_ghost_lanes<T>(
	project_id: &str,
	worktree_root: &Path,
	state_store: &StateStore,
	tracker: &T,
	selector: Option<&str>,
) -> Result<Vec<GhostLaneDiagnostic>>
where
	T: IssueTracker + ?Sized,
{
	diagnose_ghost_lanes_with_listing_mode(
		project_id,
		worktree_root,
		state_store,
		tracker,
		selector,
		RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
	)
}

pub(super) fn diagnose_ghost_lanes_read_only<T>(
	project_id: &str,
	worktree_root: &Path,
	state_store: &StateStore,
	tracker: &T,
	selector: Option<&str>,
) -> Result<Vec<GhostLaneDiagnostic>>
where
	T: IssueTracker + ?Sized,
{
	diagnose_ghost_lanes_with_listing_mode(
		project_id,
		worktree_root,
		state_store,
		tracker,
		selector,
		RecoveryRuntimeMutationPolicy::ReadOnly,
	)
}

fn diagnose_ghost_lanes_with_listing_mode<T>(
	project_id: &str,
	worktree_root: &Path,
	state_store: &StateStore,
	tracker: &T,
	selector: Option<&str>,
	listing_mode: RecoveryRuntimeMutationPolicy,
) -> Result<Vec<GhostLaneDiagnostic>>
where
	T: IssueTracker + ?Sized,
{
	let (mut runs, _) = if listing_mode.allows_runtime_writes() {
		state_store.list_project_runs(project_id, 0)?
	} else {
		state_store.list_project_runs_read_only(project_id, 0)?
	};

	if let Some(selector) = selector {
		let selector = selector.trim();

		runs.retain(|run| identifiers::ghost_lane_run_matches_selector(run, selector));

		if runs.is_empty() {
			eyre::bail!("No leased lane matched `{selector}`.");
		}
		if runs.len() > 1 {
			eyre::bail!(
				"`{selector}` matched multiple leased lanes; pass the exact local issue id."
			);
		}
	}

	runs.into_iter()
		.map(|run| {
			inspection::inspect_ghost_lane(
				project_id,
				worktree_root,
				state_store,
				tracker,
				&run,
				selector,
			)
		})
		.collect()
}
