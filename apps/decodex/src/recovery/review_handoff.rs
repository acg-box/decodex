//! Review handoff recovery orchestration and validation.

use super::{
	RebindMode, RebindSuccessStateTransition, RecoveryContext, ReviewHandoffAdoptRequest,
	ReviewHandoffDiagnoseRequest, ReviewHandoffRebindRequest, ReviewHandoffRecoveryReport,
	WorkflowTracker, active_recovery_tracker_backoff_message, apply_review_handoff_adopt,
	apply_review_handoff_rebind, diagnose_all_retained_review_worktrees, diagnose_issue,
	git_toplevel_path, inspect_rebind_pull_request, landing_url, load_recovery_context_for_dry_run,
	load_recovery_context_read_only, manual_adopt_run_id,
	remember_recovery_tracker_backoff_message, render_review_handoff_recovery_report,
	repository_relative_path, validate_adopt_issue_state_for_policy, validate_adopt_landing_state,
	validate_rebind_issue_state_for_policy, worktree_checkout_branch_name, worktree_head_oid,
	worktree_is_clean,
};
use crate::{
	prelude::{Result, eyre},
	pull_request::PullRequestLandingState,
	state::{ReviewHandoffMarker, ReviewOrchestrationMarker, WorktreeMapping},
	tracker::{self, IssueTracker, TrackerIssue},
};
use color_eyre::eyre::WrapErr;
use std::{
	env, fs,
	path::{Path, PathBuf},
};

pub(super) struct RebindValidation {
	pub(super) issue: TrackerIssue,
	pub(super) worktree: WorktreeMapping,
	pub(super) run_id: String,
	pub(super) attempt_number: i64,
	pub(super) landing_state: PullRequestLandingState,
	pub(super) local_head_oid: String,
	pub(super) worktree_path_for_event: Option<String>,
	pub(super) active_label_present: bool,
	pub(super) restore_active_label: bool,
	pub(super) mode: RebindMode,
	pub(super) success_state_transition: Option<RebindSuccessStateTransition>,
	pub(super) clear_needs_attention_label: bool,
}
impl RebindValidation {
	pub(super) fn should_restore_active_label(&self) -> bool {
		self.restore_active_label
	}
}

pub(super) struct AdoptValidation {
	pub(super) issue: TrackerIssue,
	pub(super) branch_name: String,
	pub(super) worktree_path: PathBuf,
	pub(super) run_id: String,
	pub(super) attempt_number: i64,
	pub(super) landing_state: PullRequestLandingState,
	pub(super) local_head_oid: String,
	pub(super) worktree_path_for_event: Option<String>,
	pub(super) active_label_present: bool,
	pub(super) success_state_transition: Option<RebindSuccessStateTransition>,
	pub(super) previous_worktree_mapping: Option<WorktreeMapping>,
}
impl AdoptValidation {
	pub(super) fn should_restore_active_label(&self) -> bool {
		!self.active_label_present
	}
}

pub(super) struct RebindLabelValidation {
	pub(super) active_label_present: bool,
	pub(super) restore_active_label: bool,
	pub(super) clear_needs_attention_label: bool,
}

/// Run a read-only retained review handoff diagnostic.
pub(crate) fn run_review_handoff_diagnose(
	config_path: Option<&Path>,
	request: &ReviewHandoffDiagnoseRequest,
) -> Result<()> {
	let context = load_recovery_context_read_only(config_path)?;

	if let Some(message) = active_recovery_tracker_backoff_message(&context)? {
		println!("{message}");

		return Ok(());
	}

	let diagnostics = match match request.issue.as_deref() {
		Some(issue_identifier) =>
			diagnose_issue(&context, issue_identifier).map(|diagnostic| vec![diagnostic]),
		None => diagnose_all_retained_review_worktrees(&context),
	} {
		Ok(diagnostics) => diagnostics,
		Err(error) => {
			if let Some(message) = remember_recovery_tracker_backoff_message(
				&context,
				&error,
				"review_handoff_recovery",
			) {
				println!("{message}");

				return Ok(());
			}

			return Err(error);
		},
	};
	let report = ReviewHandoffRecoveryReport {
		project_id: context.config.service_id().to_owned(),
		diagnostics,
	};

	if request.json {
		println!("{}", serde_json::to_string_pretty(&report)?);
	} else {
		print!("{}", render_review_handoff_recovery_report(&report));
	}

	Ok(())
}

/// Run an explicit retained review handoff rebind.
pub(crate) fn run_review_handoff_rebind(
	config_path: Option<&Path>,
	request: &ReviewHandoffRebindRequest,
) -> Result<()> {
	let context = load_recovery_context_for_dry_run(config_path, request.dry_run)?;
	let validation = validate_rebind_request(&context, request)?;

	if request.dry_run {
		let state_transition = validation
			.success_state_transition
			.as_ref()
			.map_or("none", |transition| transition.state_name.as_str());

		println!(
			"dry run: review handoff rebind validated for project={} issue={} branch={} pr={} head={} mode={} active_label_present={} would_restore_active_label={} state_transition={}",
			context.config.service_id(),
			validation.issue.identifier,
			validation.worktree.branch_name(),
			landing_url(&validation.landing_state),
			validation.local_head_oid,
			validation.mode.evidence_value(),
			validation.active_label_present,
			validation.should_restore_active_label(),
			state_transition
		);

		return Ok(());
	}

	apply_review_handoff_rebind(&context, &validation)?;

	println!(
		"rebind ok: project={} issue={} branch={} pr={} head={} mode={}",
		context.config.service_id(),
		validation.issue.identifier,
		validation.worktree.branch_name(),
		landing_url(&validation.landing_state),
		validation.local_head_oid,
		validation.mode.evidence_value()
	);

	Ok(())
}

/// Run an explicit manual PR takeover into retained review handoff state.
pub(crate) fn run_review_handoff_adopt(
	config_path: Option<&Path>,
	request: &ReviewHandoffAdoptRequest,
) -> Result<()> {
	let context = load_recovery_context_for_dry_run(config_path, request.dry_run)?;
	let validation = validate_adopt_request(&context, request)?;

	if request.dry_run {
		let state_transition = validation
			.success_state_transition
			.as_ref()
			.map_or("none", |transition| transition.state_name.as_str());
		let active_label = tracker::automation_active_label(context.config.service_id());

		println!(
			"dry run: review handoff adopt validated for project={} issue={} branch={} pr={} head={} run_id={} attempt={} active_label={} active_label_present={} would_restore_active_label={} state_transition={}",
			context.config.service_id(),
			validation.issue.identifier,
			validation.branch_name,
			landing_url(&validation.landing_state),
			validation.local_head_oid,
			validation.run_id,
			validation.attempt_number,
			active_label,
			validation.active_label_present,
			validation.should_restore_active_label(),
			state_transition
		);

		return Ok(());
	}

	apply_review_handoff_adopt(&context, &validation)?;

	println!(
		"adopt ok: project={} issue={} branch={} pr={} head={} run_id={} attempt={}",
		context.config.service_id(),
		validation.issue.identifier,
		validation.branch_name,
		landing_url(&validation.landing_state),
		validation.local_head_oid,
		validation.run_id,
		validation.attempt_number
	);

	Ok(())
}

fn validate_rebind_request(
	context: &RecoveryContext,
	request: &ReviewHandoffRebindRequest,
) -> Result<RebindValidation> {
	let issue = load_issue_by_identifier(&context.tracker, &request.issue)?;
	let worktree = validate_rebind_issue_context(context, &issue)?;
	let existing_handoff = context.state_store.review_handoff_marker(
		context.config.service_id(),
		&issue.id,
		worktree.branch_name(),
	)?;
	let landing_state = inspect_rebind_pull_request(context, &request.pr_url)?;
	let local_head_oid = validate_rebind_worktree(&worktree, &landing_state)?;
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
	let (run_id, attempt_number, mode) = validate_rebind_existing_handoff(
		context,
		&issue,
		&worktree,
		existing_handoff.as_ref(),
		existing_orchestration.as_ref(),
		&landing_state,
		&local_head_oid,
	)?;
	let success_state_transition = validate_rebind_issue_state(context, &issue, mode)?;
	let label_validation = validate_rebind_tracker_labels(context, &issue, mode)?;
	let worktree_path_for_event =
		repository_relative_path(context.config.repo_root(), worktree.worktree_path());

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
	let issue = load_issue_by_identifier(&context.tracker, &request.issue)?;
	let label_validation = validate_adopt_issue_context(context, &issue)?;
	let landing_state = inspect_rebind_pull_request(context, &request.pr_url)?;
	let existing_worktree_mapping = context.state_store.worktree_for_issue(&issue.id)?;

	validate_adopt_landing_state(&landing_state)?;

	let cwd = env::current_dir()?;
	let worktree_path = validate_adopt_current_worktree(
		context,
		&issue,
		&landing_state,
		&cwd,
		existing_worktree_mapping.as_ref(),
	)?;
	let branch_name = worktree_checkout_branch_name(&worktree_path)?
		.ok_or_else(|| eyre::eyre!("Manual takeover worktree is detached."))?;
	let local_head_oid = worktree_head_oid(&worktree_path)?
		.ok_or_else(|| eyre::eyre!("Manual takeover worktree has no readable HEAD."))?;

	validate_adopt_absent_handoff_marker(
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
	let run_id = manual_adopt_run_id(&issue.identifier, attempt_number, &local_head_oid);
	let worktree_path_for_event =
		repository_relative_path(context.config.repo_root(), &worktree_path);

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

pub(super) fn load_issue_by_identifier<T>(
	tracker: &T,
	issue_identifier: &str,
) -> Result<TrackerIssue>
where
	T: IssueTracker + ?Sized,
{
	tracker
		.get_issue_by_identifier(issue_identifier)?
		.ok_or_else(|| eyre::eyre!("Tracker issue `{issue_identifier}` was not found."))
}

fn validate_rebind_issue_context(
	context: &RecoveryContext,
	issue: &TrackerIssue,
) -> Result<WorktreeMapping> {
	let tracker_policy = context.workflow.frontmatter().tracker();

	if issue.has_label(tracker_policy.opt_out_label()) {
		eyre::bail!(
			"Issue `{}` has opt-out label `{}`.",
			issue.identifier,
			tracker_policy.opt_out_label()
		);
	}

	let worktree = context.state_store.worktree_for_issue(&issue.id)?.ok_or_else(|| {
		eyre::eyre!("Issue `{}` has no retained worktree mapping.", issue.identifier)
	})?;

	Ok(worktree)
}

fn validate_rebind_issue_state(
	context: &RecoveryContext,
	issue: &TrackerIssue,
	mode: RebindMode,
) -> Result<Option<RebindSuccessStateTransition>> {
	validate_rebind_issue_state_for_policy(context.workflow.frontmatter().tracker(), issue, mode)
}

fn validate_rebind_existing_handoff(
	context: &RecoveryContext,
	issue: &TrackerIssue,
	worktree: &WorktreeMapping,
	existing_handoff: Option<&ReviewHandoffMarker>,
	existing_orchestration: Option<&ReviewOrchestrationMarker>,
	landing_state: &PullRequestLandingState,
	local_head_oid: &str,
) -> Result<(String, i64, RebindMode)> {
	let Some(existing_handoff) = existing_handoff else {
		let attempt =
			context.state_store.latest_run_attempt_for_issue(&issue.id)?.ok_or_else(|| {
				eyre::eyre!("Issue `{}` has no recorded run attempt to rebind.", issue.identifier)
			})?;

		return Ok((
			attempt.run_id().to_owned(),
			attempt.attempt_number(),
			RebindMode::RestoreMissingHandoff,
		));
	};

	validate_existing_handoff_refresh(
		context.workflow.frontmatter().tracker(),
		issue,
		worktree,
		existing_handoff,
		existing_orchestration,
		landing_state,
		local_head_oid,
	)
}

pub(super) fn validate_existing_handoff_refresh(
	tracker_policy: &WorkflowTracker,
	issue: &TrackerIssue,
	worktree: &WorktreeMapping,
	existing_handoff: &ReviewHandoffMarker,
	existing_orchestration: Option<&ReviewOrchestrationMarker>,
	landing_state: &PullRequestLandingState,
	local_head_oid: &str,
) -> Result<(String, i64, RebindMode)> {
	if existing_handoff.pr_url() != landing_url(landing_state) {
		eyre::bail!(
			"Issue `{}` already has a review lifecycle record for branch `{}` and PR `{}`; refusing to rebind it to `{}`.",
			issue.identifier,
			worktree.branch_name(),
			existing_handoff.pr_url(),
			landing_url(landing_state)
		);
	}

	let orchestration_is_current = existing_orchestration.is_none_or(|marker| {
		marker.branch_name() == worktree.branch_name()
			&& marker.pr_url() == landing_url(landing_state)
			&& marker.head_sha() == local_head_oid
	});

	if existing_handoff.pr_head_oid() == local_head_oid && orchestration_is_current {
		if issue.state.name == tracker_policy.in_progress_state()
			|| issue.state.name == tracker_policy.failure_state()
		{
			return Ok((
				existing_handoff.run_id().to_owned(),
				existing_handoff.attempt_number(),
				RebindMode::CompleteExistingHandoffState,
			));
		}

		eyre::bail!(
			"Issue `{}` already has a review lifecycle record for branch `{}` and PR `{}` at head `{local_head_oid}`; no rebind is needed.",
			issue.identifier,
			worktree.branch_name(),
			existing_handoff.pr_url()
		);
	}

	Ok((
		existing_handoff.run_id().to_owned(),
		existing_handoff.attempt_number(),
		RebindMode::RefreshExistingHandoff,
	))
}

fn validate_rebind_worktree(
	worktree: &WorktreeMapping,
	landing_state: &PullRequestLandingState,
) -> Result<String> {
	validate_retained_pr_worktree(worktree, landing_state, "rebind")
}

pub(super) fn validate_retained_pr_worktree(
	worktree: &WorktreeMapping,
	landing_state: &PullRequestLandingState,
	action_label: &str,
) -> Result<String> {
	let local_branch = worktree_checkout_branch_name(worktree.worktree_path())?
		.ok_or_else(|| eyre::eyre!("Retained worktree is detached."))?;

	if local_branch != worktree.branch_name() {
		eyre::bail!(
			"Retained worktree branch is `{local_branch}`, but runtime mapping expects `{}`.",
			worktree.branch_name()
		);
	}
	if landing_state.head_ref_name != worktree.branch_name() {
		eyre::bail!(
			"Pull request `{}` points at branch `{}`, but retained lane branch is `{}`.",
			landing_url(landing_state),
			landing_state.head_ref_name,
			worktree.branch_name()
		);
	}
	if !worktree_is_clean(worktree.worktree_path())? {
		eyre::bail!(
			"Retained worktree `{}` has local changes; {action_label} requires a clean lane checkout.",
			worktree.worktree_path().display(),
		);
	}

	let local_head = worktree_head_oid(worktree.worktree_path())?
		.ok_or_else(|| eyre::eyre!("Retained worktree has no readable HEAD."))?;

	if landing_state.head_ref_oid != local_head {
		eyre::bail!(
			"Pull request `{}` points at head `{}`, but retained worktree HEAD is `{local_head}`.",
			landing_url(landing_state),
			landing_state.head_ref_oid
		);
	}

	Ok(local_head)
}

fn validate_rebind_tracker_labels(
	context: &RecoveryContext,
	issue: &TrackerIssue,
	mode: RebindMode,
) -> Result<RebindLabelValidation> {
	let active_label = tracker::automation_active_label(context.config.service_id());
	let active_label_present =
		tracker::issue_has_label_with_server_confirmation(&context.tracker, issue, &active_label)?;
	let tracker_policy = context.workflow.frontmatter().tracker();

	if !active_label_present {
		if !mode.allows_failure_state_drift_repair()
			|| issue.state.name != tracker_policy.failure_state()
		{
			eyre::bail!(
				"Issue `{}` is missing active automation label `{active_label}`. Restore explicit lane ownership before rebind.",
				issue.identifier
			);
		}

		if tracker::issue_team_label_id_with_server_confirmation(
			&context.tracker,
			issue,
			&active_label,
		)?
		.is_none()
		{
			eyre::bail!(
				"Issue `{}` is missing active automation label `{active_label}`, but that label was not found on the team.",
				issue.identifier
			);
		}
	}

	let needs_attention_label = tracker_policy.needs_attention_label();
	let needs_attention_present = tracker::issue_has_label_with_server_confirmation(
		&context.tracker,
		issue,
		needs_attention_label,
	)?;

	if !needs_attention_present {
		return Ok(RebindLabelValidation {
			active_label_present,
			restore_active_label: !active_label_present,
			clear_needs_attention_label: false,
		});
	}
	if mode.allows_failure_state_drift_repair()
		&& issue.state.name == tracker_policy.failure_state()
	{
		if tracker::issue_team_label_id_with_server_confirmation(
			&context.tracker,
			issue,
			needs_attention_label,
		)?
		.is_none()
		{
			eyre::bail!(
				"Issue `{}` has needs-attention label `{needs_attention_label}`, but that label was not found on the team.",
				issue.identifier
			);
		}

		return Ok(RebindLabelValidation {
			active_label_present,
			restore_active_label: !active_label_present,
			clear_needs_attention_label: true,
		});
	}

	eyre::bail!(
		"Issue `{}` has needs-attention label `{}`.",
		issue.identifier,
		needs_attention_label
	)
}

fn validate_adopt_issue_context(
	context: &RecoveryContext,
	issue: &TrackerIssue,
) -> Result<RebindLabelValidation> {
	let tracker_policy = context.workflow.frontmatter().tracker();

	if issue.has_label(tracker_policy.opt_out_label()) {
		eyre::bail!(
			"Issue `{}` has opt-out label `{}`.",
			issue.identifier,
			tracker_policy.opt_out_label()
		);
	}

	let active_label = tracker::automation_active_label(context.config.service_id());
	let active_label_present =
		tracker::issue_has_label_with_server_confirmation(&context.tracker, issue, &active_label)?;

	if !active_label_present
		&& tracker::issue_team_label_id_with_server_confirmation(
			&context.tracker,
			issue,
			&active_label,
		)?
		.is_none()
	{
		eyre::bail!(
			"Issue `{}` is missing active automation label `{active_label}`, and that label was not found on the team.",
			issue.identifier
		);
	}

	let needs_attention_label = tracker_policy.needs_attention_label();
	let needs_attention_present = tracker::issue_has_label_with_server_confirmation(
		&context.tracker,
		issue,
		needs_attention_label,
	)?;

	if needs_attention_present {
		eyre::bail!(
			"Issue `{}` has needs-attention label `{needs_attention_label}`; manual takeover adopt will not bypass a human-required stop.",
			issue.identifier
		);
	}

	Ok(RebindLabelValidation {
		active_label_present,
		restore_active_label: false,
		clear_needs_attention_label: false,
	})
}

fn validate_adopt_issue_state(
	context: &RecoveryContext,
	issue: &TrackerIssue,
) -> Result<Option<RebindSuccessStateTransition>> {
	validate_adopt_issue_state_for_policy(context.workflow.frontmatter().tracker(), issue)
}

fn validate_adopt_current_worktree(
	context: &RecoveryContext,
	issue: &TrackerIssue,
	landing_state: &PullRequestLandingState,
	cwd: &Path,
	existing_worktree_mapping: Option<&WorktreeMapping>,
) -> Result<PathBuf> {
	let worktree_path = git_toplevel_path(cwd)?;
	let canonical_worktree = fs::canonicalize(&worktree_path).wrap_err_with(|| {
		format!("Failed to canonicalize current worktree `{}`.", worktree_path.display())
	})?;
	let canonical_root = fs::canonicalize(context.config.worktree_root()).wrap_err_with(|| {
		format!(
			"Failed to canonicalize configured worktree root `{}`.",
			context.config.worktree_root().display()
		)
	})?;

	if !canonical_worktree.starts_with(&canonical_root) || canonical_worktree == canonical_root {
		eyre::bail!(
			"Manual takeover adopt for issue `{}` must run from a managed lane under worktree_root `{}`.",
			issue.identifier,
			context.config.worktree_root().display()
		);
	}

	let local_branch = worktree_checkout_branch_name(&canonical_worktree)?
		.ok_or_else(|| eyre::eyre!("Manual takeover worktree is detached."))?;

	if let Some(mapping) = existing_worktree_mapping {
		validate_adopt_existing_worktree_mapping(
			context.config.service_id(),
			issue,
			mapping,
			&canonical_worktree,
		)?;
	}

	if local_branch != landing_state.head_ref_name {
		eyre::bail!(
			"Pull request `{}` points at branch `{}`, but current worktree branch is `{local_branch}`.",
			landing_url(landing_state),
			landing_state.head_ref_name
		);
	}
	if !worktree_is_clean(&canonical_worktree)? {
		eyre::bail!(
			"Manual takeover worktree `{}` has local changes; adopt requires a clean lane checkout.",
			canonical_worktree.display()
		);
	}

	let local_head = worktree_head_oid(&canonical_worktree)?
		.ok_or_else(|| eyre::eyre!("Manual takeover worktree has no readable HEAD."))?;

	if landing_state.head_ref_oid != local_head {
		eyre::bail!(
			"Pull request `{}` points at head `{}`, but current worktree HEAD is `{local_head}`.",
			landing_url(landing_state),
			landing_state.head_ref_oid
		);
	}

	Ok(canonical_worktree)
}

pub(super) fn validate_adopt_existing_worktree_mapping(
	service_id: &str,
	issue: &TrackerIssue,
	mapping: &WorktreeMapping,
	canonical_worktree: &Path,
) -> Result<()> {
	if mapping.project_id() != service_id {
		eyre::bail!(
			"Issue `{}` already has a retained worktree mapping for project `{}`, not `{}`.",
			issue.identifier,
			mapping.project_id(),
			service_id
		);
	}

	let canonical_mapping = fs::canonicalize(mapping.worktree_path()).wrap_err_with(|| {
		format!(
			"Failed to canonicalize retained worktree mapping `{}` for issue `{}`.",
			mapping.worktree_path().display(),
			issue.identifier
		)
	})?;

	if canonical_mapping != canonical_worktree {
		eyre::bail!(
			"Issue `{}` already has a retained worktree mapping at `{}`, but manual takeover adopt is running from `{}`.",
			issue.identifier,
			mapping.worktree_path().display(),
			canonical_worktree.display()
		);
	}

	Ok(())
}

fn validate_adopt_absent_handoff_marker(
	context: &RecoveryContext,
	issue: &TrackerIssue,
	branch_name: &str,
	existing_worktree_mapping: Option<&WorktreeMapping>,
) -> Result<()> {
	let mut branches = vec![branch_name.to_owned()];

	if let Some(mapping) = existing_worktree_mapping
		&& mapping.branch_name() != branch_name
	{
		branches.push(mapping.branch_name().to_owned());
	}

	for branch in branches {
		if context
			.state_store
			.review_handoff_marker(context.config.service_id(), &issue.id, &branch)?
			.is_some()
		{
			eyre::bail!(
				"Issue `{}` already has a retained review lifecycle record for branch `{branch}`; use `decodex land` or `decodex recover review-handoff rebind` instead.",
				issue.identifier
			);
		}
	}

	Ok(())
}

pub(super) fn relative_worktree_path_for_recovery(
	context: &RecoveryContext,
	worktree_path: &Path,
) -> Option<String> {
	repository_relative_path(context.config.repo_root(), worktree_path).or_else(|| {
		worktree_path
			.strip_prefix(context.config.repo_root())
			.ok()
			.map(|relative| relative.to_string_lossy().to_string())
	})
}
