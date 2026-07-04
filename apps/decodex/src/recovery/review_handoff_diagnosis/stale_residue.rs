use std::collections::HashSet;

use crate::{
	orchestrator,
	prelude::Result,
	recovery::{
		REVIEW_HANDOFF_STALE_TERMINAL_RESIDUE_CLASSIFICATION, context::RecoveryContext,
		reports::ReviewHandoffDiagnostic,
	},
	state::WorktreeMapping,
};

pub(in crate::recovery::review_handoff_diagnosis) fn retained_review_worktree_is_stale_terminal_residue(
	context: &RecoveryContext,
	worktree: &WorktreeMapping,
) -> Result<bool> {
	let active_issue_ids = context
		.state_store
		.list_active_shared_leases(context.config.service_id())?
		.into_iter()
		.map(|lease| lease.issue_id().to_owned())
		.collect::<HashSet<_>>();

	orchestrator::worktree_mapping_is_stale_terminal_local_residue(
		&context.config,
		&context.state_store,
		worktree,
		&active_issue_ids,
	)
}

pub(in crate::recovery::review_handoff_diagnosis) fn stale_terminal_residue_review_handoff_diagnostic(
	context: &RecoveryContext,
	worktree: &WorktreeMapping,
) -> ReviewHandoffDiagnostic {
	ReviewHandoffDiagnostic {
		project_id: context.config.service_id().to_owned(),
		issue_id: worktree.issue_id().to_owned(),
		issue_identifier: worktree.issue_id().to_owned(),
		issue_state: String::from("local_terminal_residue"),
		classification: String::from(REVIEW_HANDOFF_STALE_TERMINAL_RESIDUE_CLASSIFICATION),
		reason: String::from(
			"terminal_unleased_runtime_recorded_identifier_mapping_with_missing_path",
		),
		branch_name: worktree.branch_name().to_owned(),
		worktree_path: worktree.worktree_path().display().to_string(),
		local_branch_name: None,
		local_head_oid: None,
		worktree_clean: None,
		existing_pr_url: None,
		existing_lifecycle_handoff_head_oid: None,
		existing_lifecycle_phase_head_oid: None,
		pr_base_ref: None,
		pr_head_oid: None,
		pr_read_error: None,
		mismatched_field: None,
		active_label_present: None,
		next_action: String::from(
			"No review-handoff recovery action is required; project reconciliation clears this stale local mapping before tracker refresh.",
		),
	}
}

pub(in crate::recovery::review_handoff_diagnosis) fn stale_terminal_residue_worktree_for_issue(
	context: &RecoveryContext,
	issue_identifier: &str,
) -> Result<Option<WorktreeMapping>> {
	let Some(worktree) = context.state_store.worktree_for_issue(issue_identifier)? else {
		return Ok(None);
	};

	if retained_review_worktree_is_stale_terminal_residue(context, &worktree)? {
		Ok(Some(worktree))
	} else {
		Ok(None)
	}
}
