use std::{path::Path, process::Command};

use crate::{
	prelude::{Result, eyre},
	pull_request::PullRequestLandingState,
	recovery::{
		closeout::{
			LegacyCloseoutValidation, MergedCloseoutRetainedContext, MergedCloseoutValidation,
		},
		context::RecoveryContext,
		git_worktree::{self},
		pull_request_inspection::{self},
		requests::{LegacyCloseoutRecoveryRequest, MergedCloseoutRecoveryRequest},
		review_handoff::{self},
	},
	state::WorktreeMapping,
	tracker::{
		self, IssueTracker, TrackerIssue,
		records::{self, LinearExecutionEventRecord},
	},
	workflow::WorkflowTracker,
};

pub(super) fn validate_legacy_closeout_request(
	context: &RecoveryContext,
	request: &LegacyCloseoutRecoveryRequest,
) -> Result<LegacyCloseoutValidation> {
	let issue = review_handoff::load_issue_by_identifier(&context.tracker, &request.issue)?;

	validate_legacy_closeout_issue_state(context.workflow.frontmatter().tracker(), &issue)?;

	let worktree = legacy_closeout_worktree(context, &issue)?;

	if !worktree.provenance().is_legacy_unknown() {
		eyre::bail!(
			"Issue `{}` worktree provenance is `{}`; legacy closeout requires `legacy_unknown` cleanup-only provenance.",
			issue.identifier,
			worktree.provenance().source()
		);
	}

	let (landing_state, default_branch) =
		pull_request_inspection::inspect_project_pull_request(context, &request.pr_url)?;

	if landing_state.base_ref_name != default_branch {
		eyre::bail!(
			"Pull request `{}` targets `{}`, but configured default branch is `{}`.",
			request.pr_url,
			landing_state.base_ref_name,
			default_branch
		);
	}
	if landing_state.state != "MERGED" {
		eyre::bail!(
			"Pull request `{}` is `{}`; legacy closeout requires `MERGED`.",
			request.pr_url,
			landing_state.state
		);
	}

	let local_head_oid = validate_legacy_closeout_worktree(&worktree, &landing_state)?;
	let merge_commit = pull_request_inspection::inspect_project_pull_request_merge_commit(
		context,
		&request.pr_url,
	)?;
	let worktree_path_for_event = git_worktree::repository_relative_path(
		context.config.repo_root(),
		worktree.worktree_path(),
	);

	Ok(LegacyCloseoutValidation {
		issue,
		worktree,
		landing_state,
		local_head_oid,
		merge_commit,
		worktree_path_for_event,
	})
}

pub(super) fn validate_merged_closeout_request(
	context: &RecoveryContext,
	request: &MergedCloseoutRecoveryRequest,
) -> Result<MergedCloseoutValidation> {
	let issue = review_handoff::load_issue_by_identifier(&context.tracker, &request.issue)?;

	validate_merged_closeout_issue_context(context, &issue)?;

	let (landing_state, default_branch) =
		pull_request_inspection::inspect_project_pull_request(context, &request.pr_url)?;

	validate_merged_closeout_pull_request(context, &landing_state, &default_branch)?;

	let merge_commit = pull_request_inspection::inspect_project_pull_request_merge_commit(
		context,
		&request.pr_url,
	)?;

	ensure_merge_commit_reachable_from_remote_default_branch(
		context.config.repo_root(),
		&request.pr_url,
		&merge_commit,
		&default_branch,
	)?;

	let worktree_mapping = retained_worktree_mapping_for_issue(context, &issue)?;
	let retained_context =
		merged_closeout_retained_context(context, &issue, worktree_mapping.as_ref())?;

	if landing_state.head_ref_name != retained_context.branch_name {
		eyre::bail!(
			"Pull request `{}` points at branch `{}`, but retained lane branch is `{}`.",
			pull_request_inspection::landing_url(&landing_state),
			landing_state.head_ref_name,
			retained_context.branch_name
		);
	}

	validate_merged_closeout_worktree_mapping(
		context,
		&issue,
		worktree_mapping.as_ref(),
		&landing_state,
	)?;

	Ok(MergedCloseoutValidation {
		issue,
		branch_name: retained_context.branch_name,
		worktree_path_for_event: retained_context.worktree_path,
		run_id: retained_context.run_id,
		attempt_number: retained_context.attempt_number,
		landing_state,
		merge_commit,
		worktree_mapping,
	})
}

fn validate_legacy_closeout_issue_state(
	tracker_policy: &WorkflowTracker,
	issue: &TrackerIssue,
) -> Result<()> {
	if tracker_policy.terminal_states().iter().any(|state| state == &issue.state.name) {
		return Ok(());
	}

	eyre::bail!(
		"Issue `{}` is in `{}`, but legacy closeout requires a terminal state: {}.",
		issue.identifier,
		issue.state.name,
		tracker_policy.terminal_states().join(", ")
	)
}

fn legacy_closeout_worktree(
	context: &RecoveryContext,
	issue: &TrackerIssue,
) -> Result<WorktreeMapping> {
	if let Some(worktree) = context.state_store.worktree_for_issue(&issue.id)? {
		return Ok(worktree);
	}
	if let Some(worktree) = context.state_store.worktree_for_issue(&issue.identifier)? {
		return Ok(worktree);
	}

	eyre::bail!("Issue `{}` has no retained worktree mapping.", issue.identifier)
}

fn validate_merged_closeout_issue_context(
	context: &RecoveryContext,
	issue: &TrackerIssue,
) -> Result<()> {
	let tracker_policy = context.workflow.frontmatter().tracker();
	let completed_state = tracker_policy.resolved_completed_state();

	if issue.state.name != completed_state {
		eyre::bail!(
			"Issue `{}` is in `{}`, but merged closeout recovery requires `{completed_state}`.",
			issue.identifier,
			issue.state.name
		);
	}
	if issue.has_label(tracker_policy.opt_out_label()) {
		eyre::bail!(
			"Issue `{}` has opt-out label `{}`.",
			issue.identifier,
			tracker_policy.opt_out_label()
		);
	}

	for label in [
		tracker::automation_queue_label(context.config.service_id()),
		tracker::automation_active_label(context.config.service_id()),
		tracker_policy.needs_attention_label().to_owned(),
	] {
		if tracker::issue_has_label_with_server_confirmation(&context.tracker, issue, &label)? {
			eyre::bail!(
				"Issue `{}` still has Linear label `{label}`; merged closeout recovery requires queue, active, and needs-attention labels to be absent.",
				issue.identifier
			);
		}
	}

	Ok(())
}

fn retained_worktree_mapping_for_issue(
	context: &RecoveryContext,
	issue: &TrackerIssue,
) -> Result<Option<WorktreeMapping>> {
	if let Some(worktree) = context.state_store.worktree_for_issue(&issue.id)? {
		return Ok(Some(worktree));
	}

	context.state_store.worktree_for_issue(&issue.identifier)
}

fn merged_closeout_retained_context(
	context: &RecoveryContext,
	issue: &TrackerIssue,
	worktree_mapping: Option<&WorktreeMapping>,
) -> Result<MergedCloseoutRetainedContext> {
	let latest_record = latest_merged_closeout_source_record(context, issue)?;
	let branch_name = worktree_mapping
		.map(|mapping| mapping.branch_name().to_owned())
		.or_else(|| latest_record.as_ref().and_then(|record| record.branch.clone()))
		.ok_or_else(|| {
			eyre::eyre!(
				"Issue `{}` has no retained branch in runtime state or execution ledger.",
				issue.identifier
			)
		})?;
	let worktree_path = worktree_mapping
		.and_then(|mapping| {
			review_handoff::relative_worktree_path_for_recovery(context, mapping.worktree_path())
		})
		.or_else(|| latest_record.as_ref().and_then(|record| record.worktree_path.clone()))
		.unwrap_or_else(|| format!(".worktrees/{}", issue.identifier));
	let (run_id, attempt_number) = if let Some(record) = latest_record
		.as_ref()
		.filter(|record| !record.run_id.trim().is_empty() && record.attempt_number >= 1)
	{
		(record.run_id.clone(), record.attempt_number)
	} else if let Some(attempt) = context.state_store.latest_run_attempt_for_issue(&issue.id)? {
		(attempt.run_id().to_owned(), attempt.attempt_number())
	} else {
		(format!("merged-closeout-{}", issue.identifier.to_ascii_lowercase()), 1)
	};

	Ok(MergedCloseoutRetainedContext { branch_name, worktree_path, run_id, attempt_number })
}

fn latest_merged_closeout_source_record(
	context: &RecoveryContext,
	issue: &TrackerIssue,
) -> Result<Option<LinearExecutionEventRecord>> {
	let mut records =
		context.state_store.list_linear_execution_events(context.config.service_id(), &issue.id)?;

	if issue.identifier != issue.id {
		records.extend(
			context
				.state_store
				.list_linear_execution_events(context.config.service_id(), &issue.identifier)?,
		);
	}

	let comments = context.tracker.list_comments(&issue.id)?;

	records.extend(
		comments
			.iter()
			.filter_map(|comment| records::parse_linear_execution_event_record(&comment.body))
			.filter(|record| {
				record.service_id == context.config.service_id()
					&& (record.issue_id == issue.id || record.issue_identifier == issue.identifier)
			}),
	);

	Ok(records
		.into_iter()
		.filter(|record| record.branch.as_ref().is_some_and(|branch| !branch.trim().is_empty()))
		.max_by(|left, right| {
			left.event_timestamp
				.cmp(&right.event_timestamp)
				.then_with(|| left.idempotency_key.cmp(&right.idempotency_key))
		}))
}

fn validate_merged_closeout_pull_request(
	context: &RecoveryContext,
	landing_state: &PullRequestLandingState,
	default_branch: &str,
) -> Result<()> {
	if landing_state.base_ref_name != default_branch {
		eyre::bail!(
			"Pull request `{}` targets `{}`, but configured default branch is `{default_branch}`.",
			pull_request_inspection::landing_url(landing_state),
			landing_state.base_ref_name
		);
	}
	if landing_state.state != "MERGED" {
		eyre::bail!(
			"Pull request `{}` is `{}`; merged closeout recovery requires `MERGED`.",
			pull_request_inspection::landing_url(landing_state),
			landing_state.state
		);
	}
	if landing_state.head_ref_name.trim().is_empty() {
		eyre::bail!(
			"Pull request `{}` does not expose the merged head branch required for retained lane reconciliation.",
			pull_request_inspection::landing_url(landing_state)
		);
	}
	if landing_state.head_ref_name == default_branch {
		eyre::bail!(
			"Pull request `{}` uses default branch `{default_branch}` as its head; merged closeout recovery cannot prove retained lane identity.",
			pull_request_inspection::landing_url(landing_state)
		);
	}

	let remote_ref = format!("refs/remotes/origin/{default_branch}");
	let output = Command::new("git")
		.arg("-C")
		.arg(context.config.repo_root())
		.args(["rev-parse", "--verify", remote_ref.as_str()])
		.output()?;

	if !output.status.success() {
		let stderr = String::from_utf8_lossy(&output.stderr);

		eyre::bail!(
			"Configured repo root `{}` does not expose `{remote_ref}`; sync the default branch before merged closeout recovery: {}",
			context.config.repo_root().display(),
			stderr.trim()
		);
	}

	Ok(())
}

fn ensure_merge_commit_reachable_from_remote_default_branch(
	repo_root: &Path,
	pr_url: &str,
	merge_commit: &str,
	default_branch: &str,
) -> Result<()> {
	let remote_ref = format!("refs/remotes/origin/{default_branch}");
	let status = Command::new("git")
		.arg("-C")
		.arg(repo_root)
		.args(["merge-base", "--is-ancestor", merge_commit, remote_ref.as_str()])
		.status()?;

	if status.success() {
		return Ok(());
	}
	if status.code() == Some(1) {
		eyre::bail!(
			"Configured repo root `{}` remote `{remote_ref}` does not contain merge commit `{merge_commit}` for `{pr_url}`.",
			repo_root.display()
		);
	}

	eyre::bail!(
		"`git merge-base --is-ancestor {merge_commit} {remote_ref}` failed in `{}` with status `{status}`.",
		repo_root.display()
	)
}

fn validate_merged_closeout_worktree_mapping(
	context: &RecoveryContext,
	issue: &TrackerIssue,
	worktree_mapping: Option<&WorktreeMapping>,
	landing_state: &PullRequestLandingState,
) -> Result<()> {
	if let Some(mapping) = worktree_mapping {
		if mapping.branch_name() != landing_state.head_ref_name {
			eyre::bail!(
				"Issue `{}` retained worktree branch is `{}`, but merged PR head branch is `{}`.",
				issue.identifier,
				mapping.branch_name(),
				landing_state.head_ref_name
			);
		}

		return validate_merged_closeout_worktree_path(mapping.worktree_path(), landing_state);
	}

	let Some(relative_path) = latest_merged_closeout_source_record(context, issue)?
		.and_then(|record| record.worktree_path)
	else {
		return Ok(());
	};
	let worktree_path = context.config.repo_root().join(relative_path);

	validate_merged_closeout_worktree_path(&worktree_path, landing_state)
}

fn validate_merged_closeout_worktree_path(
	worktree_path: &Path,
	landing_state: &PullRequestLandingState,
) -> Result<()> {
	if !worktree_path.exists() {
		return Ok(());
	}
	if !git_worktree::worktree_is_clean(worktree_path)? {
		eyre::bail!(
			"Retained worktree `{}` still has local changes; merged closeout recovery will not mark it cleanup-complete.",
			worktree_path.display()
		);
	}

	let local_branch =
		git_worktree::worktree_checkout_branch_name(worktree_path)?.ok_or_else(|| {
			eyre::eyre!("Retained worktree `{}` is detached.", worktree_path.display())
		})?;

	if local_branch != landing_state.head_ref_name {
		eyre::bail!(
			"Retained worktree `{}` is on branch `{local_branch}`, but merged PR head branch is `{}`.",
			worktree_path.display(),
			landing_state.head_ref_name
		);
	}

	let local_head = git_worktree::worktree_head_oid(worktree_path)?.ok_or_else(|| {
		eyre::eyre!("Retained worktree `{}` has no readable HEAD.", worktree_path.display())
	})?;

	if local_head != landing_state.head_ref_oid {
		eyre::bail!(
			"Retained worktree `{}` HEAD is `{local_head}`, but merged PR head is `{}`.",
			worktree_path.display(),
			landing_state.head_ref_oid
		);
	}

	Ok(())
}

fn validate_legacy_closeout_worktree(
	worktree: &WorktreeMapping,
	landing_state: &PullRequestLandingState,
) -> Result<String> {
	review_handoff::validate_retained_pr_worktree(worktree, landing_state, "legacy closeout")
}
