//! Diagnostic assembly for retained review-handoff recovery.

use std::collections::{HashMap, HashSet};

use crate::{
	orchestrator,
	prelude::{Result, eyre},
	pull_request::PullRequestLandingState,
	state::{ReviewHandoffMarker, ReviewOrchestrationMarker, WorktreeMapping},
	tracker::{self, IssueTracker, TrackerIssue},
};

use super::{
	MISSING_HANDOFF_REASON, ORPHANED_REVIEW_HANDOFF_CLASSIFICATION,
	REVIEW_HANDOFF_BOUND_CLASSIFICATION, REVIEW_HANDOFF_MISMATCH_CLASSIFICATION,
	REVIEW_HANDOFF_OWNERSHIP_DRIFT_CLASSIFICATION, REVIEW_HANDOFF_REBIND_REQUIRED_CLASSIFICATION,
	REVIEW_HANDOFF_STALE_TERMINAL_RESIDUE_CLASSIFICATION, REVIEW_HANDOFF_UNVERIFIED_CLASSIFICATION,
	context::RecoveryContext,
	git_worktree::{
		ReviewHandoffLineage, worktree_checkout_branch_name,
		worktree_head_descends_from_review_handoff, worktree_head_oid, worktree_is_clean,
	},
	reports::ReviewHandoffDiagnostic,
};

pub(super) struct HandoffBindingDiagnostic {
	pub(super) classification: String,
	pub(super) reason: String,
	pub(super) pr_base_ref: Option<String>,
	pub(super) pr_head_oid: Option<String>,
	pub(super) mismatched_field: Option<String>,
	pub(super) next_action: String,
}

pub(super) struct HandoffDiagnosticRequest<'a> {
	pub(super) service_id: &'a str,
	pub(super) issue_identifier: &'a str,
	pub(super) issue_state_name: &'a str,
	pub(super) success_state: &'a str,
	pub(super) in_progress_state: &'a str,
	pub(super) failure_state: &'a str,
	pub(super) worktree: &'a WorktreeMapping,
	pub(super) existing_handoff: Option<&'a ReviewHandoffMarker>,
	pub(super) existing_orchestration: Option<&'a ReviewOrchestrationMarker>,
	pub(super) local_branch_name: Option<&'a str>,
	pub(super) local_head_oid: Option<&'a str>,
	pub(super) worktree_clean: Option<bool>,
	pub(super) pr_inspection: Option<&'a PullRequestLandingState>,
	pub(super) active_label_present: Option<bool>,
}

struct HandoffDiagnosticContext<'a> {
	issue_identifier: &'a str,
	worktree: &'a WorktreeMapping,
	existing_handoff: &'a ReviewHandoffMarker,
	existing_orchestration: Option<&'a ReviewOrchestrationMarker>,
	local_branch_name: Option<&'a str>,
	local_head_oid: Option<&'a str>,
	worktree_clean: Option<bool>,
}

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

	let issue = super::load_issue_by_identifier(tracker, issue_identifier)?;
	let worktree = context.state_store.worktree_for_issue(&issue.id)?.ok_or_else(|| {
		eyre::eyre!("Issue `{}` has no retained worktree mapping.", issue.identifier)
	})?;

	diagnose_issue_worktree(context, issue, worktree)
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
		.and_then(|handoff| super::inspect_project_pull_request(context, handoff.pr_url()).ok())
		.map(|(landing_state, _default_branch)| landing_state);
	let local_branch_name = worktree_checkout_branch_name(worktree.worktree_path()).ok().flatten();
	let local_head_oid = worktree_head_oid(worktree.worktree_path()).ok().flatten();
	let worktree_clean = worktree_is_clean(worktree.worktree_path()).ok();
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

pub(super) fn diagnostic_binding(
	request: HandoffDiagnosticRequest<'_>,
) -> HandoffBindingDiagnostic {
	let Some(existing_handoff) = request.existing_handoff else {
		return HandoffBindingDiagnostic {
			classification: String::from(ORPHANED_REVIEW_HANDOFF_CLASSIFICATION),
			reason: String::from(MISSING_HANDOFF_REASON),
			pr_base_ref: None,
			pr_head_oid: None,
			mismatched_field: None,
			next_action: missing_handoff_next_action(request.service_id, request.issue_identifier),
		};
	};
	let context = HandoffDiagnosticContext {
		issue_identifier: request.issue_identifier,
		worktree: request.worktree,
		existing_handoff,
		existing_orchestration: request.existing_orchestration,
		local_branch_name: request.local_branch_name,
		local_head_oid: request.local_head_oid,
		worktree_clean: request.worktree_clean,
	};
	let pr_base_ref = request.pr_inspection.map(|pr| pr.base_ref_name.clone());
	let pr_head_oid = request.pr_inspection.map(|pr| pr.head_ref_oid.clone());

	if let Some(diagnostic) = worktree_binding_mismatch(&context, &pr_base_ref, &pr_head_oid) {
		return diagnostic;
	}

	let Some(local_head_oid) = request.local_head_oid else {
		return mismatched_handoff_diagnostic(
			"worktree_head_missing",
			"worktree.local_head",
			pr_base_ref,
			pr_head_oid,
			inspect_handoff_next_action(request.issue_identifier, existing_handoff.pr_url()),
		);
	};
	let Some(pr_inspection) = request.pr_inspection else {
		return HandoffBindingDiagnostic {
			classification: String::from(REVIEW_HANDOFF_UNVERIFIED_CLASSIFICATION),
			reason: String::from("pull_request_state_read_failed"),
			pr_base_ref,
			pr_head_oid,
			mismatched_field: Some(String::from("pr_url")),
			next_action: inspect_handoff_next_action(
				request.issue_identifier,
				existing_handoff.pr_url(),
			),
		};
	};

	if let Some(diagnostic) =
		pull_request_binding_mismatch(&context, pr_inspection, &pr_base_ref, &pr_head_oid)
	{
		return diagnostic;
	}
	if let Some(diagnostic) =
		marker_head_binding_mismatch(&context, local_head_oid, &pr_base_ref, &pr_head_oid)
	{
		return diagnostic;
	}

	if let Some(diagnostic) = handoff_issue_state_drift_diagnostic(
		&request,
		existing_handoff,
		pr_base_ref.clone(),
		pr_head_oid.clone(),
	) {
		return diagnostic;
	}

	HandoffBindingDiagnostic {
		classification: String::from(REVIEW_HANDOFF_BOUND_CLASSIFICATION),
		reason: String::from("review_handoff_record_present"),
		pr_base_ref,
		pr_head_oid,
		mismatched_field: None,
		next_action: bound_handoff_next_action(request.service_id, request.active_label_present),
	}
}

fn handoff_issue_state_drift_diagnostic(
	request: &HandoffDiagnosticRequest<'_>,
	existing_handoff: &ReviewHandoffMarker,
	pr_base_ref: Option<String>,
	pr_head_oid: Option<String>,
) -> Option<HandoffBindingDiagnostic> {
	if request.active_label_present == Some(false) {
		let next_action = if request.issue_state_name == request.in_progress_state
			|| request.issue_state_name == request.failure_state
		{
			rebind_state_transition_next_action(request.issue_identifier, existing_handoff.pr_url())
		} else if request.issue_state_name == request.success_state {
			bound_handoff_next_action(request.service_id, request.active_label_present)
		} else {
			issue_state_mismatch_next_action(request.success_state, request.in_progress_state)
		};

		return Some(HandoffBindingDiagnostic {
			classification: String::from(REVIEW_HANDOFF_OWNERSHIP_DRIFT_CLASSIFICATION),
			reason: String::from("active_ownership_label_missing"),
			pr_base_ref,
			pr_head_oid,
			mismatched_field: Some(String::from("issue.labels")),
			next_action,
		});
	}

	if request.issue_state_name == request.in_progress_state {
		return Some(HandoffBindingDiagnostic {
			classification: String::from(REVIEW_HANDOFF_REBIND_REQUIRED_CLASSIFICATION),
			reason: String::from("review_handoff_state_transition_pending"),
			pr_base_ref,
			pr_head_oid,
			mismatched_field: Some(String::from("issue.state")),
			next_action: rebind_state_transition_next_action(
				request.issue_identifier,
				existing_handoff.pr_url(),
			),
		});
	}

	if request.issue_state_name == request.failure_state {
		return Some(HandoffBindingDiagnostic {
			classification: String::from(REVIEW_HANDOFF_REBIND_REQUIRED_CLASSIFICATION),
			reason: String::from("review_handoff_failure_state_drift"),
			pr_base_ref,
			pr_head_oid,
			mismatched_field: Some(String::from("issue.state")),
			next_action: rebind_state_transition_next_action(
				request.issue_identifier,
				existing_handoff.pr_url(),
			),
		});
	}

	(request.issue_state_name != request.success_state).then(|| HandoffBindingDiagnostic {
		classification: String::from(REVIEW_HANDOFF_MISMATCH_CLASSIFICATION),
		reason: String::from("review_handoff_issue_state_mismatch"),
		pr_base_ref,
		pr_head_oid,
		mismatched_field: Some(String::from("issue.state")),
		next_action: issue_state_mismatch_next_action(
			request.success_state,
			request.in_progress_state,
		),
	})
}

fn worktree_binding_mismatch(
	context: &HandoffDiagnosticContext<'_>,
	pr_base_ref: &Option<String>,
	pr_head_oid: &Option<String>,
) -> Option<HandoffBindingDiagnostic> {
	let mismatch = if context.existing_handoff.branch_name() != context.worktree.branch_name() {
		Some(("review_handoff_branch_mismatch", "review_handoff.branch_name"))
	} else if context.local_branch_name.is_none() {
		Some(("worktree_checkout_branch_missing", "worktree.local_branch"))
	} else if context.local_branch_name != Some(context.worktree.branch_name()) {
		Some(("worktree_checkout_branch_mismatch", "worktree.local_branch"))
	} else if context.worktree_clean == Some(false) {
		Some(("worktree_dirty", "worktree.clean"))
	} else if context.local_head_oid.is_none() {
		Some(("worktree_head_missing", "worktree.local_head"))
	} else {
		None
	};

	mismatch.map(|(reason, field)| {
		mismatched_handoff_diagnostic(
			reason,
			field,
			pr_base_ref.clone(),
			pr_head_oid.clone(),
			inspect_handoff_next_action(
				context.issue_identifier,
				context.existing_handoff.pr_url(),
			),
		)
	})
}

fn pull_request_binding_mismatch(
	context: &HandoffDiagnosticContext<'_>,
	pr_inspection: &PullRequestLandingState,
	pr_base_ref: &Option<String>,
	pr_head_oid: &Option<String>,
) -> Option<HandoffBindingDiagnostic> {
	if context
		.existing_handoff
		.target_base_ref_name()
		.is_some_and(|base_ref| base_ref != pr_inspection.base_ref_name.as_str())
	{
		return Some(rebind_required_diagnostic(
			"review_handoff_base_mismatch",
			"review_handoff.target_base_ref_name",
			pr_base_ref.clone(),
			pr_head_oid.clone(),
			context.issue_identifier,
			context.existing_handoff.pr_url(),
		));
	}

	let mismatch = if pr_inspection.head_ref_name != context.worktree.branch_name() {
		Some(("pull_request_branch_mismatch", "pull_request.head_ref_name"))
	} else if context.local_head_oid != Some(pr_inspection.head_ref_oid.as_str()) {
		Some(("pull_request_head_mismatch", "pull_request.head_ref_oid"))
	} else {
		None
	};

	mismatch.map(|(reason, field)| {
		mismatched_handoff_diagnostic(
			reason,
			field,
			pr_base_ref.clone(),
			pr_head_oid.clone(),
			inspect_handoff_next_action(
				context.issue_identifier,
				context.existing_handoff.pr_url(),
			),
		)
	})
}

fn marker_head_binding_mismatch(
	context: &HandoffDiagnosticContext<'_>,
	local_head_oid: &str,
	pr_base_ref: &Option<String>,
	pr_head_oid: &Option<String>,
) -> Option<HandoffBindingDiagnostic> {
	let mismatch = if context.existing_handoff.pr_head_oid() != local_head_oid {
		match worktree_head_descends_from_review_handoff(
			context.worktree.worktree_path(),
			context.existing_handoff.pr_head_oid(),
			local_head_oid,
		) {
			ReviewHandoffLineage::Descends => None,
			ReviewHandoffLineage::Diverged =>
				Some(("review_handoff_lineage_mismatch", "review_handoff.pr_head_oid")),
			ReviewHandoffLineage::Unknown =>
				Some(("review_handoff_lineage_check_failed", "review_handoff.pr_head_oid")),
		}
	} else if let Some(orchestration) = context.existing_orchestration {
		orchestration_binding_mismatch(context, orchestration, local_head_oid)
	} else {
		None
	};

	mismatch.map(|(reason, field)| {
		rebind_required_diagnostic(
			reason,
			field,
			pr_base_ref.clone(),
			pr_head_oid.clone(),
			context.issue_identifier,
			context.existing_handoff.pr_url(),
		)
	})
}

fn orchestration_binding_mismatch(
	context: &HandoffDiagnosticContext<'_>,
	orchestration: &ReviewOrchestrationMarker,
	local_head_oid: &str,
) -> Option<(&'static str, &'static str)> {
	if orchestration.branch_name() != context.worktree.branch_name() {
		Some(("review_orchestration_branch_mismatch", "review_orchestration.branch_name"))
	} else if orchestration.pr_url() != context.existing_handoff.pr_url() {
		Some(("review_orchestration_pr_mismatch", "review_orchestration.pr_url"))
	} else if orchestration.head_sha() != local_head_oid {
		Some(("review_orchestration_head_mismatch", "review_orchestration.head_sha"))
	} else {
		None
	}
}

fn mismatched_handoff_diagnostic(
	reason: &str,
	mismatched_field: &str,
	pr_base_ref: Option<String>,
	pr_head_oid: Option<String>,
	next_action: String,
) -> HandoffBindingDiagnostic {
	HandoffBindingDiagnostic {
		classification: String::from(REVIEW_HANDOFF_MISMATCH_CLASSIFICATION),
		reason: reason.to_owned(),
		pr_base_ref,
		pr_head_oid,
		mismatched_field: Some(mismatched_field.to_owned()),
		next_action,
	}
}

fn rebind_required_diagnostic(
	reason: &str,
	mismatched_field: &str,
	pr_base_ref: Option<String>,
	pr_head_oid: Option<String>,
	issue_identifier: &str,
	pr_url: &str,
) -> HandoffBindingDiagnostic {
	HandoffBindingDiagnostic {
		classification: String::from(REVIEW_HANDOFF_REBIND_REQUIRED_CLASSIFICATION),
		reason: reason.to_owned(),
		pr_base_ref,
		pr_head_oid,
		mismatched_field: Some(mismatched_field.to_owned()),
		next_action: rebind_refresh_next_action(issue_identifier, pr_url),
	}
}

fn missing_handoff_next_action(service_id: &str, issue_identifier: &str) -> String {
	format!(
		"Inspect PR lineage and ensure label `{}` is present. Use `decodex recover review-handoff rebind {} --pr <URL>` for a retained lane PR, or `decodex recover review-handoff adopt {} --pr <URL>` from the managed worktree for a human-owned PR takeover.",
		tracker::automation_active_label(service_id),
		issue_identifier,
		issue_identifier
	)
}

fn bound_handoff_next_action(service_id: &str, active_label_present: Option<bool>) -> String {
	if active_label_present == Some(false) {
		return format!(
			"Restore explicit lane ownership with label `{}`, then rerun `decodex recover review-handoff diagnose <ISSUE>` and continue the existing post-review lifecycle.",
			tracker::automation_active_label(service_id)
		);
	}

	String::from("Continue the existing post-review lifecycle; no rebind is needed.")
}

fn inspect_handoff_next_action(issue_identifier: &str, pr_url: &str) -> String {
	format!(
		"Inspect the retained worktree and PR `{pr_url}`; run `decodex recover review-handoff rebind {issue_identifier} --pr {pr_url}` only after the mismatch is repaired."
	)
}

fn rebind_refresh_next_action(issue_identifier: &str, pr_url: &str) -> String {
	format!(
		"Run `decodex recover review-handoff rebind {issue_identifier} --pr {pr_url} --dry-run`, then rerun without `--dry-run` to refresh the retained lifecycle record if validation passes."
	)
}

fn rebind_state_transition_next_action(issue_identifier: &str, pr_url: &str) -> String {
	format!(
		"Run `decodex recover review-handoff rebind {issue_identifier} --pr {pr_url} --dry-run`, then rerun without `--dry-run` to complete the pending issue-state transition if validation passes."
	)
}

fn issue_state_mismatch_next_action(success_state: &str, in_progress_state: &str) -> String {
	format!(
		"Move the issue to `{success_state}` or `{in_progress_state}` only after confirming the retained handoff lineage still belongs to the current lane."
	)
}
