mod cleanup;
mod issue;
mod ledger;
mod marker;

#[cfg(test)] pub(super) use self::issue::ensure_manual_closeout_issue_scope;
#[cfg(test)] pub(super) use self::marker::read_manual_land_closeout_marker;
pub(super) use self::{
	cleanup::{
		cleanup_manual_land_lane_checkout, ensure_manual_land_checkout_is_managed_lane,
		ensure_manual_land_left_no_merged_worktree_cleanup_debt, manual_land_cleanup_identifier,
	},
	issue::{apply_closeout, prepare_closeout},
	ledger::{
		clear_manual_closeout_issue_scope, clear_manual_closeout_runtime_state,
		write_manual_land_cleanup_complete_event,
	},
	marker::{manual_land_closeout_matches, write_manual_land_closeout_marker},
};

use crate::{
	default_branch_sync,
	manual::{ManualLandContext, ManualLandLedgerContext},
	prelude::{Result, eyre},
	runtime,
};

pub(super) fn finalize_land_closeout(
	context: &ManualLandContext,
	merge_commit: &str,
	default_branch: &str,
	landed_change_record: &str,
) -> Result<()> {
	let state_store = if context.prepared_closeout.is_some() {
		Some(runtime::open_runtime_store()?)
	} else {
		None
	};
	let worktree_path_for_event = cleanup::manual_land_relative_worktree_path(context);

	if let Some(prepared_closeout) = context.prepared_closeout.as_ref() {
		let state_store = state_store
			.as_ref()
			.ok_or_else(|| eyre::eyre!("Manual closeout state store was not opened."))?;
		let handoff = context.review_handoff.as_ref().ok_or_else(|| {
			eyre::eyre!("`decodex land` issue closeout requires a retained review handoff marker.")
		})?;
		let ledger = ManualLandLedgerContext {
			service_id: &prepared_closeout.service_id,
			issue: &prepared_closeout.issue,
			state_store,
			handoff,
			pr_url: &context.pr_url,
			merge_commit,
			branch_name: &context.current_branch,
			worktree_path: &worktree_path_for_event,
			completed_state: &prepared_closeout.completed_state,
			default_branch,
			privacy_classifier: &context.public_projection_privacy_classifier,
		};

		apply_closeout(
			&context.cwd,
			&prepared_closeout.tracker,
			&prepared_closeout.completed_state,
			&ledger,
			landed_change_record,
		)?;
	}

	default_branch_sync::sync_repo_root_default_branch(
		&context.canonical_repo_root,
		default_branch,
		Some(context.default_branch_git_credentials()),
	)?;

	if context.prepared_closeout.is_none()
		&& !manual_land_closeout_matches(
			&context.cwd,
			&context.pr_url,
			merge_commit,
			&context.current_branch,
			landed_change_record,
		)? {
		write_manual_land_closeout_marker(
			&context.cwd,
			&context.pr_url,
			merge_commit,
			&context.current_branch,
			landed_change_record,
		)?;
	}

	cleanup_manual_land_lane_checkout(context)?;

	if let Some(prepared_closeout) = context.prepared_closeout.as_ref() {
		let state_store = state_store
			.as_ref()
			.ok_or_else(|| eyre::eyre!("Manual closeout state store was not opened."))?;
		let handoff = context.review_handoff.as_ref().ok_or_else(|| {
			eyre::eyre!("`decodex land` issue cleanup requires a retained review handoff marker.")
		})?;

		clear_manual_closeout_runtime_state(
			state_store,
			&prepared_closeout.issue.id,
			handoff.run_id(),
		)?;
		clear_manual_closeout_issue_scope(
			&prepared_closeout.tracker,
			&prepared_closeout.issue,
			&prepared_closeout.service_id,
			&prepared_closeout.needs_attention_label,
		)?;

		let ledger = ManualLandLedgerContext {
			service_id: &prepared_closeout.service_id,
			issue: &prepared_closeout.issue,
			state_store,
			handoff,
			pr_url: &context.pr_url,
			merge_commit,
			branch_name: &context.current_branch,
			worktree_path: &worktree_path_for_event,
			completed_state: &prepared_closeout.completed_state,
			default_branch,
			privacy_classifier: &context.public_projection_privacy_classifier,
		};

		write_manual_land_cleanup_complete_event(&prepared_closeout.tracker, &ledger)?;
	}

	Ok(())
}
