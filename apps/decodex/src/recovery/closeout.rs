//! Legacy and merged closeout recovery flows.

use std::{path::Path, process::Command};

use crate::{
	prelude::{Result, eyre},
	pull_request::PullRequestLandingState,
	state::WorktreeMapping,
	tracker::{
		self, IssueTracker, TrackerIssue,
		privacy_classifier::ConfiguredPublicProjectionPrivacyClassifier,
		records::{self, LinearExecutionEventIdentity, LinearExecutionEventRecord},
	},
	workflow::WorkflowTracker,
};

use super::{
	LEGACY_MANUAL_CLOSEOUT_ANCHOR, LEGACY_MANUAL_CLOSEOUT_EVENT, MERGED_CLOSEOUT_CLEANUP_ANCHOR,
	MERGED_CLOSEOUT_CLOSEOUT_ANCHOR,
	context::RecoveryContext,
	events::{current_timestamp, timestamp_after_seconds},
	git_worktree::{
		repository_relative_path, worktree_checkout_branch_name, worktree_head_oid,
		worktree_is_clean,
	},
	landing_url, load_issue_by_identifier, relative_worktree_path_for_recovery,
	requests::{LegacyCloseoutRecoveryRequest, MergedCloseoutRecoveryRequest},
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
	let validation = validate_legacy_closeout_request(&context, request)?;

	if request.dry_run {
		println!(
			"dry run: legacy closeout validated for project={} issue={} branch={} pr={} head={} merge_commit={} provenance={}",
			context.config.service_id(),
			validation.issue.identifier,
			validation.worktree.branch_name(),
			landing_url(&validation.landing_state),
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

	let event = legacy_closeout_event(&context, &validation);
	let audit_recorded = write_legacy_closeout_audit(&context, &validation, &event)?;

	println!(
		"legacy closeout audit ok: project={} issue={} branch={} pr={} head={} merge_commit={} audit_recorded={audit_recorded}",
		context.config.service_id(),
		validation.issue.identifier,
		validation.worktree.branch_name(),
		landing_url(&validation.landing_state),
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
	let validation = validate_merged_closeout_request(&context, request)?;

	if request.dry_run {
		println!(
			"dry run: merged closeout validated for project={} issue={} branch={} worktree_path={} pr={} head={} merge_commit={} run_id={} attempt={}",
			context.config.service_id(),
			validation.issue.identifier,
			validation.branch_name,
			validation.worktree_path_for_event,
			landing_url(&validation.landing_state),
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
		apply_merged_closeout_recovery(&context, &validation)?;

	println!(
		"merged closeout recovery ok: project={} issue={} branch={} worktree_path={} pr={} head={} merge_commit={} closeout_recorded={} cleanup_recorded={}",
		context.config.service_id(),
		validation.issue.identifier,
		validation.branch_name,
		validation.worktree_path_for_event,
		landing_url(&validation.landing_state),
		validation.landing_state.head_ref_oid,
		validation.merge_commit,
		closeout_recorded,
		cleanup_recorded
	);

	Ok(())
}

fn validate_legacy_closeout_request(
	context: &RecoveryContext,
	request: &LegacyCloseoutRecoveryRequest,
) -> Result<LegacyCloseoutValidation> {
	let issue = load_issue_by_identifier(&context.tracker, &request.issue)?;

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
		super::inspect_project_pull_request(context, &request.pr_url)?;

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
	let merge_commit = super::inspect_project_pull_request_merge_commit(context, &request.pr_url)?;
	let worktree_path_for_event =
		repository_relative_path(context.config.repo_root(), worktree.worktree_path());

	Ok(LegacyCloseoutValidation {
		issue,
		worktree,
		landing_state,
		local_head_oid,
		merge_commit,
		worktree_path_for_event,
	})
}

fn validate_merged_closeout_request(
	context: &RecoveryContext,
	request: &MergedCloseoutRecoveryRequest,
) -> Result<MergedCloseoutValidation> {
	let issue = load_issue_by_identifier(&context.tracker, &request.issue)?;

	validate_merged_closeout_issue_context(context, &issue)?;

	let (landing_state, default_branch) =
		super::inspect_project_pull_request(context, &request.pr_url)?;

	validate_merged_closeout_pull_request(context, &landing_state, &default_branch)?;

	let merge_commit = super::inspect_project_pull_request_merge_commit(context, &request.pr_url)?;

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
			landing_url(&landing_state),
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
		.and_then(|mapping| relative_worktree_path_for_recovery(context, mapping.worktree_path()))
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
			landing_url(landing_state),
			landing_state.base_ref_name
		);
	}
	if landing_state.state != "MERGED" {
		eyre::bail!(
			"Pull request `{}` is `{}`; merged closeout recovery requires `MERGED`.",
			landing_url(landing_state),
			landing_state.state
		);
	}
	if landing_state.head_ref_name.trim().is_empty() {
		eyre::bail!(
			"Pull request `{}` does not expose the merged head branch required for retained lane reconciliation.",
			landing_url(landing_state)
		);
	}
	if landing_state.head_ref_name == default_branch {
		eyre::bail!(
			"Pull request `{}` uses default branch `{default_branch}` as its head; merged closeout recovery cannot prove retained lane identity.",
			landing_url(landing_state)
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
	if !worktree_is_clean(worktree_path)? {
		eyre::bail!(
			"Retained worktree `{}` still has local changes; merged closeout recovery will not mark it cleanup-complete.",
			worktree_path.display()
		);
	}

	let local_branch = worktree_checkout_branch_name(worktree_path)?.ok_or_else(|| {
		eyre::eyre!("Retained worktree `{}` is detached.", worktree_path.display())
	})?;

	if local_branch != landing_state.head_ref_name {
		eyre::bail!(
			"Retained worktree `{}` is on branch `{local_branch}`, but merged PR head branch is `{}`.",
			worktree_path.display(),
			landing_state.head_ref_name
		);
	}

	let local_head = worktree_head_oid(worktree_path)?.ok_or_else(|| {
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
	super::validate_retained_pr_worktree(worktree, landing_state, "legacy closeout")
}

fn write_legacy_closeout_audit(
	context: &RecoveryContext,
	validation: &LegacyCloseoutValidation,
	event: &LinearExecutionEventRecord,
) -> Result<bool> {
	let audit_body = format!(
		"Decodex legacy manual closeout audit: verified merged PR `{}` for `{}`. Runtime provenance was `{}`, so this records the manual fallback before local cleanup.",
		landing_url(&validation.landing_state),
		validation.issue.identifier,
		validation.worktree.provenance().source()
	);
	let retry_budget_attempt_count =
		context.state_store.retry_budget_attempt_count(&validation.issue.id)?;
	let retry_budget_attempt_count =
		(retry_budget_attempt_count > 0).then_some(retry_budget_attempt_count);
	let body = format!(
		"{audit_body}\n\n{}",
		records::render_linear_execution_event_comment_body(event, retry_budget_attempt_count)
	);
	let privacy_classifier = ConfiguredPublicProjectionPrivacyClassifier::from_config(
		context.config.privacy_classifier(),
	)?;
	let projection =
		tracker::prepare_linear_execution_event_comment(&body, event, &privacy_classifier)?;
	let recorded = context.state_store.record_linear_execution_event(&projection.record)?;

	if !recorded {
		return Ok(false);
	}

	if let Err(error) = tracker::create_prepared_linear_execution_event_comment_without_remote_scan(
		&context.tracker,
		&validation.issue.id,
		&projection,
	) {
		context.state_store.forget_linear_execution_event(&projection.record.idempotency_key)?;

		return Err(error);
	}

	Ok(true)
}

fn apply_merged_closeout_recovery(
	context: &RecoveryContext,
	validation: &MergedCloseoutValidation,
) -> Result<(bool, bool)> {
	let closeout_event = merged_closeout_event(context, validation);
	let cleanup_event = merged_closeout_cleanup_event(context, validation);
	let closeout_recorded = write_merged_closeout_event(
		context,
		validation,
		&closeout_event,
		"Decodex merged closeout recovery: verified the PR was merged into the current default branch and reconciled the stale retained attention closeout ledger.",
	)?;
	let cleanup_recorded = match write_merged_closeout_event(
		context,
		validation,
		&cleanup_event,
		"Decodex merged closeout recovery: verified retained lane cleanup is already complete and recorded cleanup_complete.",
	) {
		Ok(cleanup_recorded) => cleanup_recorded,
		Err(error) => {
			if closeout_recorded {
				context
					.state_store
					.forget_linear_execution_event(&closeout_event.idempotency_key)?;
			}

			return Err(error);
		},
	};

	if validation.worktree_mapping.is_some() {
		context.state_store.clear_worktree(&validation.issue.id)?;

		if validation.issue.identifier != validation.issue.id {
			context.state_store.clear_worktree(&validation.issue.identifier)?;
		}
	}

	context.state_store.update_run_status(&validation.run_id, "succeeded")?;

	Ok((closeout_recorded, cleanup_recorded))
}

fn write_merged_closeout_event(
	context: &RecoveryContext,
	validation: &MergedCloseoutValidation,
	event: &LinearExecutionEventRecord,
	body: &str,
) -> Result<bool> {
	let privacy_classifier = ConfiguredPublicProjectionPrivacyClassifier::from_config(
		context.config.privacy_classifier(),
	)?;
	let retry_budget_attempt_count =
		context.state_store.retry_budget_attempt_count(&validation.issue.id)?;
	let retry_budget_attempt_count =
		(retry_budget_attempt_count > 0).then_some(retry_budget_attempt_count);
	let body = format!(
		"{body}\n\n{}",
		records::render_linear_execution_event_comment_body(event, retry_budget_attempt_count)
	);
	let projection =
		tracker::prepare_linear_execution_event_comment(&body, event, &privacy_classifier)?;
	let recorded = context.state_store.record_linear_execution_event(&projection.record)?;

	if !recorded {
		return Ok(false);
	}

	if let Err(error) = tracker::create_prepared_linear_execution_event_comment_without_remote_scan(
		&context.tracker,
		&validation.issue.id,
		&projection,
	) {
		context.state_store.forget_linear_execution_event(&projection.record.idempotency_key)?;

		return Err(error);
	}

	Ok(true)
}

fn legacy_closeout_event(
	context: &RecoveryContext,
	validation: &LegacyCloseoutValidation,
) -> LinearExecutionEventRecord {
	let pr_url = landing_url(&validation.landing_state);
	let stable_anchor = records::stable_event_anchor(&[
		pr_url,
		&validation.local_head_oid,
		&validation.merge_commit,
		LEGACY_MANUAL_CLOSEOUT_ANCHOR,
	]);
	let run_id = format!("legacy-closeout-{}", validation.issue.identifier.to_ascii_lowercase());
	let mut event = LinearExecutionEventRecord::new(
		LinearExecutionEventIdentity {
			service_id: context.config.service_id(),
			issue_id: &validation.issue.id,
			issue_identifier: &validation.issue.identifier,
			run_id: &run_id,
			attempt_number: 1,
		},
		LEGACY_MANUAL_CLOSEOUT_EVENT,
		current_timestamp(),
		&stable_anchor,
	);

	event.branch = Some(validation.worktree.branch_name().to_owned());
	event.worktree_path = validation.worktree_path_for_event.clone();
	event.pr_url = Some(pr_url.to_owned());
	event.pr_head_sha = Some(validation.local_head_oid.clone());
	event.pr_base_ref = Some(validation.landing_state.base_ref_name.clone());
	event.commit_sha = Some(validation.merge_commit.clone());
	event.validation_result = Some(String::from("passed"));
	event.target_state = Some(validation.issue.state.name.clone());
	event.cleanup_status = Some(String::from("manual_audit_recorded"));
	event.summary = Some(format!(
		"Legacy manual closeout audit recorded for {} after merged PR {}.",
		validation.issue.identifier, pr_url
	));
	event.evidence = Some(vec![
		format!("issue_state={}", validation.issue.state.name),
		format!("branch={}", validation.worktree.branch_name()),
		format!("pr_url={pr_url}"),
		format!("pr_head_sha={}", validation.local_head_oid),
		format!("merge_commit={}", validation.merge_commit),
		format!("worktree_provenance={}", validation.worktree.provenance().source()),
		String::from("worktree_clean=true"),
	]);
	event.next_action = Some(String::from(
		"remove the local worktree only after preserving or discarding local-only changes intentionally",
	));

	event
}

fn merged_closeout_event(
	context: &RecoveryContext,
	validation: &MergedCloseoutValidation,
) -> LinearExecutionEventRecord {
	let pr_url = landing_url(&validation.landing_state);
	let stable_anchor = records::stable_event_anchor(&[
		pr_url,
		&validation.merge_commit,
		MERGED_CLOSEOUT_CLOSEOUT_ANCHOR,
	]);
	let mut event = LinearExecutionEventRecord::new(
		LinearExecutionEventIdentity {
			service_id: context.config.service_id(),
			issue_id: &validation.issue.id,
			issue_identifier: &validation.issue.identifier,
			run_id: &validation.run_id,
			attempt_number: validation.attempt_number,
		},
		LEGACY_MANUAL_CLOSEOUT_EVENT,
		current_timestamp(),
		&stable_anchor,
	);

	event.branch = Some(validation.branch_name.clone());
	event.worktree_path = Some(validation.worktree_path_for_event.clone());
	event.pr_url = Some(pr_url.to_owned());
	event.pr_head_sha = Some(validation.landing_state.head_ref_oid.clone());
	event.pr_base_ref = Some(validation.landing_state.base_ref_name.clone());
	event.commit_sha = Some(validation.merge_commit.clone());
	event.validation_result = Some(String::from("passed"));
	event.target_state = Some(validation.issue.state.name.clone());
	event.summary = Some(format!(
		"Merged closeout recovery recorded for {} after PR {} was already merged.",
		validation.issue.identifier, pr_url
	));
	event.evidence = Some(vec![
		format!("issue_state={}", validation.issue.state.name),
		format!("branch={}", validation.branch_name),
		format!("pr_url={pr_url}"),
		format!("pr_head_sha={}", validation.landing_state.head_ref_oid),
		format!("merge_commit={}", validation.merge_commit),
		String::from("origin_default_contains_merge_commit=true"),
	]);
	event.next_action = Some(String::from(
		"Decodex will record cleanup_complete for the already-merged retained lane.",
	));

	event
}

fn merged_closeout_cleanup_event(
	context: &RecoveryContext,
	validation: &MergedCloseoutValidation,
) -> LinearExecutionEventRecord {
	let pr_url = landing_url(&validation.landing_state);
	let stable_anchor = records::stable_event_anchor(&[
		&validation.branch_name,
		&validation.worktree_path_for_event,
		&validation.merge_commit,
		MERGED_CLOSEOUT_CLEANUP_ANCHOR,
	]);
	let mut event = LinearExecutionEventRecord::new(
		LinearExecutionEventIdentity {
			service_id: context.config.service_id(),
			issue_id: &validation.issue.id,
			issue_identifier: &validation.issue.identifier,
			run_id: &validation.run_id,
			attempt_number: validation.attempt_number,
		},
		"cleanup_complete",
		timestamp_after_seconds(1),
		&stable_anchor,
	);

	event.branch = Some(validation.branch_name.clone());
	event.worktree_path = Some(validation.worktree_path_for_event.clone());
	event.pr_url = Some(pr_url.to_owned());
	event.pr_head_sha = Some(validation.landing_state.head_ref_oid.clone());
	event.pr_base_ref = Some(validation.landing_state.base_ref_name.clone());
	event.commit_sha = Some(validation.merge_commit.clone());
	event.cleanup_status = Some(String::from("merged_closeout_reconciled"));
	event.target_state = Some(validation.issue.state.name.clone());
	event.summary = Some(format!(
		"Merged closeout recovery marked stale retained lane {} cleanup complete.",
		validation.issue.identifier
	));
	event.evidence = Some(vec![
		format!("issue_state={}", validation.issue.state.name),
		format!("branch={}", validation.branch_name),
		format!("worktree_path={}", validation.worktree_path_for_event),
		String::from("linear_queue_active_attention_labels_absent=true"),
		String::from("retained_worktree_has_no_uncommitted_changes=true"),
	]);
	event.next_action = Some(String::from("No Decodex runtime action remains for this lane."));

	event
}
