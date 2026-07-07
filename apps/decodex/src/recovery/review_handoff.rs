//! Review handoff recovery orchestration and validation.

pub(in crate::recovery) mod commands;

mod issue;
mod labels;
mod model;
mod worktree;

pub(super) use self::{
	issue::load_issue_by_identifier,
	model::{AdoptValidation, RebindLabelValidation, RebindValidation},
	worktree::{relative_worktree_path_for_recovery, validate_retained_pr_worktree},
};
#[cfg(test)]
pub(super) use self::{
	issue::{validate_existing_handoff_refresh, validate_rebind_existing_handoff},
	labels::validate_rebind_tracker_labels_with_tracker,
	worktree::validate_adopt_existing_worktree_mapping,
};

use std::env;

use crate::{
	prelude::{Result, eyre},
	recovery::{
		context::RecoveryContext,
		events,
		git_worktree::{self},
		pull_request_inspection::{self},
		requests::{ReviewHandoffAdoptRequest, ReviewHandoffRebindRequest},
		review_handoff_policy::{self, RebindSuccessStateTransition},
	},
	tracker::TrackerIssue,
};

fn validate_rebind_request(
	context: &RecoveryContext,
	request: &ReviewHandoffRebindRequest,
) -> Result<RebindValidation> {
	let issue = issue::load_issue_by_identifier(&context.tracker, &request.issue)?;
	let worktree = issue::validate_rebind_issue_context(context, &issue)?;
	let existing_lifecycle = context.state_store.review_lifecycle_record(
		context.config.service_id(),
		&issue.id,
		worktree.branch_name(),
	)?;
	let landing_state =
		pull_request_inspection::inspect_rebind_pull_request(context, &request.pr_url)?;
	let local_head_oid = worktree::validate_rebind_worktree(&worktree, &landing_state)?;
	let (run_id, attempt_number, mode) = issue::validate_rebind_existing_handoff(
		context,
		&issue,
		&worktree,
		existing_lifecycle.as_ref(),
		&landing_state,
		&local_head_oid,
	)?;
	let success_state_transition = issue::validate_rebind_issue_state(context, &issue, mode)?;
	let label_validation = labels::validate_rebind_tracker_labels(context, &issue, mode)?;
	let worktree_path_for_event =
		worktree::relative_worktree_path_for_recovery(context, worktree.worktree_path());

	Ok(RebindValidation {
		issue,
		worktree,
		run_id,
		attempt_number,
		landing_state,
		local_head_oid,
		worktree_path_for_event,
		active_label_present: label_validation.active_label_present,
		restore_active_label: label_validation.restore_active_label,
		mode,
		success_state_transition,
		clear_needs_attention_label: label_validation.clear_needs_attention_label,
	})
}

fn validate_adopt_request(
	context: &RecoveryContext,
	request: &ReviewHandoffAdoptRequest,
) -> Result<AdoptValidation> {
	let issue = issue::load_issue_by_identifier(&context.tracker, &request.issue)?;
	let label_validation = labels::validate_adopt_issue_context(context, &issue)?;
	let landing_state =
		pull_request_inspection::inspect_rebind_pull_request(context, &request.pr_url)?;
	let existing_worktree_mapping = context.state_store.worktree_for_issue(&issue.id)?;

	review_handoff_policy::validate_adopt_landing_state(&landing_state)?;

	let cwd = env::current_dir()?;
	let worktree_path = worktree::validate_adopt_current_worktree(
		context,
		&issue,
		&landing_state,
		&cwd,
		existing_worktree_mapping.as_ref(),
	)?;
	let branch_name = git_worktree::worktree_checkout_branch_name(&worktree_path)?
		.ok_or_else(|| eyre::eyre!("Manual takeover worktree is detached."))?;
	let local_head_oid = git_worktree::worktree_head_oid(&worktree_path)?
		.ok_or_else(|| eyre::eyre!("Manual takeover worktree has no readable HEAD."))?;

	worktree::validate_adopt_absent_lifecycle_record(
		context,
		&issue,
		&branch_name,
		existing_worktree_mapping.as_ref(),
	)?;

	let success_state_transition = validate_adopt_issue_state(context, &issue)?;
	let attempt_number = context
		.state_store
		.latest_run_attempt_for_issue(&issue.id)?
		.map_or(1, |attempt| attempt.attempt_number().saturating_add(1));
	let run_id = events::manual_adopt_run_id(&issue.identifier, attempt_number, &local_head_oid);
	let worktree_path_for_event =
		worktree::relative_worktree_path_for_recovery(context, &worktree_path);

	Ok(AdoptValidation {
		issue,
		branch_name,
		worktree_path,
		run_id,
		attempt_number,
		landing_state,
		local_head_oid,
		worktree_path_for_event,
		active_label_present: label_validation.active_label_present,
		success_state_transition,
		previous_worktree_mapping: existing_worktree_mapping,
	})
}

fn validate_adopt_issue_state(
	context: &RecoveryContext,
	issue: &TrackerIssue,
) -> Result<Option<RebindSuccessStateTransition>> {
	review_handoff_policy::validate_adopt_issue_state_for_policy(
		context.workflow.frontmatter().tracker(),
		issue,
	)
}
