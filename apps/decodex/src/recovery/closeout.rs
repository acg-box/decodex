//! Legacy and merged closeout recovery flows.

mod apply;
mod events;
mod validation;

use std::path::Path;

use crate::{
	prelude::{Result, eyre},
	pull_request::PullRequestLandingState,
	recovery::{
		pull_request_inspection,
		requests::{LegacyCloseoutRecoveryRequest, MergedCloseoutRecoveryRequest},
	},
	state::WorktreeMapping,
	tracker::TrackerIssue,
};

struct LegacyCloseoutValidation {
	issue: TrackerIssue,
	worktree: WorktreeMapping,
	landing_state: PullRequestLandingState,
	local_head_oid: String,
	merge_commit: String,
	worktree_path_for_event: Option<String>,
}

struct MergedCloseoutValidation {
	issue: TrackerIssue,
	branch_name: String,
	worktree_path_for_event: String,
	run_id: String,
	attempt_number: i64,
	landing_state: PullRequestLandingState,
	merge_commit: String,
	worktree_mapping: Option<WorktreeMapping>,
}

struct MergedCloseoutRetainedContext {
	branch_name: String,
	worktree_path: String,
	run_id: String,
	attempt_number: i64,
}

/// Run an explicit audited legacy closeout fallback.
pub(crate) fn run_legacy_closeout(
	config_path: Option<&Path>,
	request: &LegacyCloseoutRecoveryRequest,
) -> Result<()> {
	let context = super::load_recovery_context_for_dry_run(config_path, request.dry_run)?;
	let validation = validation::validate_legacy_closeout_request(&context, request)?;

	if request.dry_run {
		println!(
			"dry run: legacy closeout validated for project={} issue={} branch={} pr={} head={} merge_commit={} provenance={}",
			context.config.service_id(),
			validation.issue.identifier,
			validation.worktree.branch_name(),
			pull_request_inspection::landing_url(&validation.landing_state),
			validation.local_head_oid,
			validation.merge_commit,
			validation.worktree.provenance().source()
		);

		return Ok(());
	}
	if !request.manual_authority {
		eyre::bail!(
			"`recover legacy-closeout` writes a closeout audit and requires --manual-authority outside dry-run mode."
		);
	}

	let event = events::legacy_closeout_event(&context, &validation);
	let audit_recorded = apply::write_legacy_closeout_audit(&context, &validation, &event)?;

	println!(
		"legacy closeout audit ok: project={} issue={} branch={} pr={} head={} merge_commit={} audit_recorded={audit_recorded}",
		context.config.service_id(),
		validation.issue.identifier,
		validation.worktree.branch_name(),
		pull_request_inspection::landing_url(&validation.landing_state),
		validation.local_head_oid,
		validation.merge_commit,
	);

	Ok(())
}

/// Run an explicit merged PR closeout reconciliation for stale retained attention.
pub(crate) fn run_merged_closeout(
	config_path: Option<&Path>,
	request: &MergedCloseoutRecoveryRequest,
) -> Result<()> {
	let context = super::load_recovery_context_for_dry_run(config_path, request.dry_run)?;
	let validation = validation::validate_merged_closeout_request(&context, request)?;

	if request.dry_run {
		println!(
			"dry run: merged closeout validated for project={} issue={} branch={} worktree_path={} pr={} head={} merge_commit={} run_id={} attempt={}",
			context.config.service_id(),
			validation.issue.identifier,
			validation.branch_name,
			validation.worktree_path_for_event,
			pull_request_inspection::landing_url(&validation.landing_state),
			validation.landing_state.head_ref_oid,
			validation.merge_commit,
			validation.run_id,
			validation.attempt_number
		);

		return Ok(());
	}
	if !request.manual_authority {
		eyre::bail!(
			"`recover merged-closeout` writes closeout and cleanup ledger records and requires --manual-authority outside dry-run mode."
		);
	}

	let (closeout_recorded, cleanup_recorded) =
		apply::apply_merged_closeout_recovery(&context, &validation)?;

	println!(
		"merged closeout recovery ok: project={} issue={} branch={} worktree_path={} pr={} head={} merge_commit={} closeout_recorded={} cleanup_recorded={}",
		context.config.service_id(),
		validation.issue.identifier,
		validation.branch_name,
		validation.worktree_path_for_event,
		pull_request_inspection::landing_url(&validation.landing_state),
		validation.landing_state.head_ref_oid,
		validation.merge_commit,
		closeout_recorded,
		cleanup_recorded
	);

	Ok(())
}
