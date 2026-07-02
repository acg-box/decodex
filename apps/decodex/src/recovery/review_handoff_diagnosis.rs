//! Diagnostic assembly for retained review-handoff recovery.

mod actions;
mod binding;

pub(super) use crate::recovery::review_handoff_diagnosis::binding::{
	HandoffDiagnosticRequest, diagnostic_binding,
};

use std::collections::{HashMap, HashSet};

use crate::{
	orchestrator,
	prelude::{Result, eyre},
	recovery::{
		self, REVIEW_HANDOFF_STALE_TERMINAL_RESIDUE_CLASSIFICATION, context::RecoveryContext,
		git_worktree, reports::ReviewHandoffDiagnostic,
	},
	state::WorktreeMapping,
	tracker::{self, IssueTracker, TrackerIssue},
};

pub(super) fn diagnose_all_retained_review_worktrees(
	context: &RecoveryContext,
) -> Result<Vec<ReviewHandoffDiagnostic>> {
	diagnose_all_retained_review_worktrees_with_tracker(context, &context.tracker)
}

pub(super) fn diagnose_all_retained_review_worktrees_with_tracker<T>(
	context: &RecoveryContext,
	tracker: &T,
) -> Result<Vec<ReviewHandoffDiagnostic>>
where
	T: IssueTracker,
{
	let mut worktrees = Vec::new();
	let mut diagnostics = Vec::new();

	for worktree in context.state_store.list_worktrees(context.config.service_id())? {
		if retained_review_worktree_is_stale_terminal_residue(context, &worktree)? {
			diagnostics.push(stale_terminal_residue_review_handoff_diagnostic(context, &worktree));
		} else {
			worktrees.push(worktree);
		}
	}

	let issues_by_id = refresh_retained_review_worktree_issues(tracker, &worktrees)?;
	let tracker_policy = context.workflow.frontmatter().tracker();
	let success_state = tracker_policy.success_state();
	let in_progress_state = tracker_policy.in_progress_state();

	for worktree in worktrees {
		let Some(issue) = issues_by_id.get(worktree.issue_id()).cloned() else {
			continue;
		};

		if issue.state.name != success_state && issue.state.name != in_progress_state {
			continue;
		}

		diagnostics.push(diagnose_issue_worktree(context, issue, worktree)?);
	}

	Ok(diagnostics)
}

pub(super) fn diagnose_issue(
	context: &RecoveryContext,
	issue_identifier: &str,
) -> Result<ReviewHandoffDiagnostic> {
	diagnose_issue_with_tracker(context, &context.tracker, issue_identifier)
}

pub(super) fn diagnose_issue_with_tracker<T>(
	context: &RecoveryContext,
	tracker: &T,
	issue_identifier: &str,
) -> Result<ReviewHandoffDiagnostic>
where
	T: IssueTracker,
{
	if let Some(worktree) = stale_terminal_residue_worktree_for_issue(context, issue_identifier)? {
		return Ok(stale_terminal_residue_review_handoff_diagnostic(context, &worktree));
	}

	let issue = recovery::load_issue_by_identifier(tracker, issue_identifier)?;
	let worktree = context.state_store.worktree_for_issue(&issue.id)?.ok_or_else(|| {
		eyre::eyre!("Issue `{}` has no retained worktree mapping.", issue.identifier)
	})?;

	diagnose_issue_worktree(context, issue, worktree)
}

fn retained_review_worktree_is_stale_terminal_residue(
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

fn stale_terminal_residue_review_handoff_diagnostic(
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
		mismatched_field: None,
		active_label_present: None,
		next_action: String::from(
			"No review-handoff recovery action is required; project reconciliation clears this stale local mapping before tracker refresh.",
		),
	}
}

fn refresh_retained_review_worktree_issues<T>(
	tracker: &T,
	worktrees: &[WorktreeMapping],
) -> Result<HashMap<String, TrackerIssue>>
where
	T: IssueTracker,
{
	if worktrees.is_empty() {
		return Ok(HashMap::new());
	}

	let issue_ids =
		worktrees.iter().map(|worktree| worktree.issue_id().to_owned()).collect::<Vec<_>>();

	Ok(tracker
		.refresh_issues(&issue_ids)?
		.into_iter()
		.map(|issue| (issue.id.clone(), issue))
		.collect())
}

fn stale_terminal_residue_worktree_for_issue(
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

fn diagnose_issue_worktree(
	context: &RecoveryContext,
	issue: TrackerIssue,
	worktree: WorktreeMapping,
) -> Result<ReviewHandoffDiagnostic> {
	let existing_handoff = context.state_store.review_handoff_marker(
		context.config.service_id(),
		&issue.id,
		worktree.branch_name(),
	)?;
	let existing_orchestration = existing_handoff
		.as_ref()
		.map(|handoff| {
			context.state_store.review_orchestration_marker(
				context.config.service_id(),
				&issue.id,
				handoff,
			)
		})
		.transpose()?
		.flatten();
	let pr_inspection = existing_handoff
		.as_ref()
		.and_then(|handoff| recovery::inspect_project_pull_request(context, handoff.pr_url()).ok())
		.map(|(landing_state, _default_branch)| landing_state);
	let local_branch_name =
		git_worktree::worktree_checkout_branch_name(worktree.worktree_path()).ok().flatten();
	let local_head_oid = git_worktree::worktree_head_oid(worktree.worktree_path()).ok().flatten();
	let worktree_clean = git_worktree::worktree_is_clean(worktree.worktree_path()).ok();
	let active_label_name = tracker::automation_active_label(context.config.service_id());
	let active_label_present = tracker::issue_has_label_with_server_confirmation(
		&context.tracker,
		&issue,
		&active_label_name,
	)
	.ok();
	let binding = diagnostic_binding(HandoffDiagnosticRequest {
		service_id: context.config.service_id(),
		issue_identifier: &issue.identifier,
		issue_state_name: &issue.state.name,
		success_state: context.workflow.frontmatter().tracker().success_state(),
		in_progress_state: context.workflow.frontmatter().tracker().in_progress_state(),
		failure_state: context.workflow.frontmatter().tracker().failure_state(),
		worktree: &worktree,
		existing_handoff: existing_handoff.as_ref(),
		existing_orchestration: existing_orchestration.as_ref(),
		local_branch_name: local_branch_name.as_deref(),
		local_head_oid: local_head_oid.as_deref(),
		worktree_clean,
		pr_inspection: pr_inspection.as_ref(),
		active_label_present,
	});

	Ok(ReviewHandoffDiagnostic {
		project_id: context.config.service_id().to_owned(),
		issue_id: issue.id.clone(),
		issue_identifier: issue.identifier.clone(),
		issue_state: issue.state.name.clone(),
		classification: binding.classification,
		reason: binding.reason,
		branch_name: worktree.branch_name().to_owned(),
		worktree_path: worktree.worktree_path().display().to_string(),
		local_branch_name,
		local_head_oid,
		worktree_clean,
		existing_pr_url: existing_handoff.as_ref().map(|handoff| handoff.pr_url().to_owned()),
		existing_lifecycle_handoff_head_oid: existing_handoff
			.as_ref()
			.map(|handoff| handoff.pr_head_oid().to_owned()),
		existing_lifecycle_phase_head_oid: existing_orchestration
			.as_ref()
			.map(|orchestration| orchestration.head_sha().to_owned()),
		pr_base_ref: binding.pr_base_ref,
		pr_head_oid: binding.pr_head_oid,
		mismatched_field: binding.mismatched_field,
		active_label_present,
		next_action: binding.next_action,
	})
}
