mod cleanup;
mod issue;
mod ledger;
mod receipt;

use std::path::Path;

use time::{OffsetDateTime, format_description::well_known::Rfc3339};

#[cfg(test)]
pub(super) use self::issue::ensure_manual_closeout_issue_scope;
#[cfg(test)]
pub(super) use self::receipt::read_manual_land_closeout_receipt;
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
	receipt::{manual_land_closeout_receipt_matches, write_manual_land_closeout_receipt},
};

use crate::{
	config::ReviewLevel,
	default_branch_sync,
	manual::{ManualLandContext, ManualLandLedgerContext},
	orchestrator::{
		PostReviewLifecycleFactsInput, PullRequestReviewState, build_post_review_lifecycle_facts,
		kernel::lifecycle::{
			LifecycleDecisionInput, LifecycleEvidenceKind, LifecycleOutcome,
			PreviousLifecycleAuthority, decide_lifecycle_transition,
		},
		runtime_review_checkpoint_status_for_head,
	},
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
		let lifecycle_record = context.review_lifecycle.as_ref().ok_or_else(|| {
			eyre::eyre!(
				"`decodex land` issue closeout requires retained review lifecycle authority."
			)
		})?;
		let ledger = ManualLandLedgerContext {
			service_id: &prepared_closeout.service_id,
			issue: &prepared_closeout.issue,
			state_store,
			lifecycle_record,
			pr_url: &context.pr_url,
			merge_commit,
			branch_name: &context.current_branch,
			worktree_path: &worktree_path_for_event,
			completed_state: &prepared_closeout.completed_state,
			default_branch,
			privacy_classifier: &context.public_projection_privacy_classifier,
		};

		record_manual_land_lifecycle_decision(
			&ledger,
			prepared_closeout.review_level,
			LifecycleEvidenceKind::LandingReadback,
			LifecycleOutcome::Succeeded,
			Some(merge_commit),
			"landed",
			"not_started",
			"not_started",
			"manual_land_readback",
		)?;
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
		&& !manual_land_closeout_receipt_matches(
			&context.cwd,
			&context.pr_url,
			merge_commit,
			&context.current_branch,
			landed_change_record,
		)? {
		write_manual_land_closeout_receipt(
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
		let lifecycle_record = context.review_lifecycle.as_ref().ok_or_else(|| {
			eyre::eyre!(
				"`decodex land` issue cleanup requires retained review lifecycle authority."
			)
		})?;

		clear_manual_closeout_runtime_state(
			state_store,
			&prepared_closeout.issue.id,
			lifecycle_record.run_id(),
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
			lifecycle_record,
			pr_url: &context.pr_url,
			merge_commit,
			branch_name: &context.current_branch,
			worktree_path: &worktree_path_for_event,
			completed_state: &prepared_closeout.completed_state,
			default_branch,
			privacy_classifier: &context.public_projection_privacy_classifier,
		};

		write_manual_land_cleanup_complete_event(&prepared_closeout.tracker, &ledger)?;
		record_manual_land_lifecycle_decision(
			&ledger,
			prepared_closeout.review_level,
			LifecycleEvidenceKind::CloseoutCompletion,
			LifecycleOutcome::Succeeded,
			Some(merge_commit),
			"landed",
			"completed",
			"completed",
			"manual_land_closeout_complete",
		)?;
	}

	Ok(())
}

fn record_manual_land_lifecycle_decision(
	ledger: &ManualLandLedgerContext<'_>,
	review_level: ReviewLevel,
	evidence_kind: LifecycleEvidenceKind,
	outcome: LifecycleOutcome,
	merge_commit: Option<&str>,
	landing_state: &str,
	closeout_state: &str,
	cleanup_state: &str,
	causation_id: &str,
) -> Result<()> {
	let checkpoint = runtime_review_checkpoint_status_for_head(
		ledger.state_store,
		ledger.service_id,
		&ledger.issue.id,
		review_level,
		ledger.lifecycle_record.pr_head_oid(),
	)?;
	let review_state = manual_land_review_state(ledger, merge_commit);
	let previous_record = ledger.state_store.review_lifecycle_record(
		ledger.service_id,
		&ledger.issue.id,
		ledger.branch_name,
	)?;
	let facts = build_post_review_lifecycle_facts(PostReviewLifecycleFactsInput {
		project_id: ledger.service_id,
		issue_id: &ledger.issue.id,
		review_lifecycle: previous_record.as_ref(),
		review_state: &review_state,
		worktree_path: Path::new(ledger.worktree_path),
		review_level,
		phase: "manual_land",
		landing_state: Some(landing_state),
		closeout_state: Some(closeout_state),
		validated_head_sha: Some(ledger.lifecycle_record.pr_head_oid()),
		review_checkpoint_phase: checkpoint.as_ref().map(|checkpoint| checkpoint.phase),
		review_checkpoint_status: checkpoint.as_ref().map(|checkpoint| checkpoint.status.as_str()),
	});
	let previous = previous_record.as_ref().map(|record| PreviousLifecycleAuthority {
		sequence: record.sequence(),
		next_state: record.next_state(),
	});
	let idempotency_key = format!(
		"{}:{}:{}:{}:{}",
		ledger.service_id,
		ledger.issue.id,
		ledger.lifecycle_record.pr_head_oid(),
		evidence_kind.as_str(),
		causation_id
	);
	let decided_at = current_timestamp();
	let decision = decide_lifecycle_transition(LifecycleDecisionInput {
		facts: &facts,
		previous,
		evidence_kind,
		outcome,
		merge_commit,
		cleanup_state: Some(cleanup_state),
		authority: "issue_authority",
		actor: "manual_land",
		idempotency_key: &idempotency_key,
		correlation_id: ledger.lifecycle_record.run_id(),
		causation_id: Some(causation_id),
		decided_at: &decided_at,
	});

	ledger.state_store.record_lifecycle_decision(
		ledger.lifecycle_record.run_id(),
		ledger.lifecycle_record.attempt_number(),
		&decision,
	)?;

	Ok(())
}

fn manual_land_review_state(
	ledger: &ManualLandLedgerContext<'_>,
	merge_commit: Option<&str>,
) -> PullRequestReviewState {
	PullRequestReviewState {
		url: ledger.pr_url.to_owned(),
		state: String::from("MERGED"),
		is_draft: false,
		review_decision: Some(String::from("APPROVED")),
		merge_commit_allowed: false,
		pending_review_requests: 0,
		mergeable: String::from("MERGEABLE"),
		merge_state_status: String::from("CLEAN"),
		head_ref_name: ledger.lifecycle_record.pr_head_ref_name().to_owned(),
		head_ref_oid: ledger.lifecycle_record.pr_head_oid().to_owned(),
		merge_commit_oid: merge_commit.map(str::to_owned),
		head_repository_name: None,
		head_repository_owner: None,
		status_check_rollup_state: Some(String::from("SUCCESS")),
		unresolved_review_threads: 0,
		issue_description_external_review_thumbs_up_count: 0,
		issue_comments: Vec::new(),
		reviews: Vec::new(),
	}
}

fn current_timestamp() -> String {
	OffsetDateTime::now_utc().format(&Rfc3339).expect("timestamp formatting should succeed")
}
