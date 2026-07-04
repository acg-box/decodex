//! Review handoff recovery orchestration and validation.

mod issue;
mod labels;
mod worktree;

pub(super) use self::{
	issue::load_issue_by_identifier,
	worktree::{relative_worktree_path_for_recovery, validate_retained_pr_worktree},
};
#[cfg(test)]
pub(super) use self::{
	issue::{validate_existing_handoff_refresh, validate_rebind_existing_handoff},
	labels::validate_rebind_tracker_labels_with_tracker,
	worktree::validate_adopt_existing_worktree_mapping,
};

use std::{
	env,
	path::{Path, PathBuf},
};

use crate::{
	prelude::{Result, eyre},
	pull_request::PullRequestLandingState,
	recovery::{
		context::{self, RecoveryContext},
		events,
		git_worktree::{self},
		pull_request_inspection::{self},
		reports,
		reports::ReviewHandoffRecoveryReport,
		requests::{
			ReviewHandoffAdoptRequest, ReviewHandoffDiagnoseRequest, ReviewHandoffRebindRequest,
		},
		review_handoff_apply,
		review_handoff_diagnosis::{self},
		review_handoff_policy::{self, RebindMode, RebindSuccessStateTransition},
	},
	state::WorktreeMapping,
	tracker::{self, TrackerIssue},
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
	let context = context::load_recovery_context_read_only(config_path)?;

	if let Some(message) = context::active_recovery_tracker_backoff_message(&context)? {
		println!("{message}");

		return Ok(());
	}

	let diagnostics = match match request.issue.as_deref() {
		Some(issue_identifier) =>
			review_handoff_diagnosis::diagnose_issue(&context, issue_identifier)
				.map(|diagnostic| vec![diagnostic]),
		None => review_handoff_diagnosis::diagnose_all_retained_review_worktrees(&context),
	} {
		Ok(diagnostics) => diagnostics,
		Err(error) => {
			if let Some(message) = context::remember_recovery_tracker_backoff_message(
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
		print!("{}", reports::render_review_handoff_recovery_report(&report));
	}

	Ok(())
}

/// Run an explicit retained review handoff rebind.
pub(crate) fn run_review_handoff_rebind(
	config_path: Option<&Path>,
	request: &ReviewHandoffRebindRequest,
) -> Result<()> {
	let context = context::load_recovery_context_for_dry_run(config_path, request.dry_run)?;
	let validation = validate_rebind_request(&context, request)?;

	if request.dry_run {
		let state_transition = validation
			.success_state_transition
			.as_ref()
			.map_or("none", |transition| transition.state_name.as_str());

		println!(
			"dry run: review handoff rebind validated for project={} issue={} branch={} pr={} head={} mode={} active_label_present={} would_restore_active_label={} would_clear_needs_attention_label={} state_transition={}",
			context.config.service_id(),
			validation.issue.identifier,
			validation.worktree.branch_name(),
			pull_request_inspection::landing_url(&validation.landing_state),
			validation.local_head_oid,
			validation.mode.evidence_value(),
			validation.active_label_present,
			validation.should_restore_active_label(),
			validation.clear_needs_attention_label,
			state_transition
		);

		return Ok(());
	}

	review_handoff_apply::apply_review_handoff_rebind(&context, &validation)?;

	println!(
		"rebind ok: project={} issue={} branch={} pr={} head={} mode={}",
		context.config.service_id(),
		validation.issue.identifier,
		validation.worktree.branch_name(),
		pull_request_inspection::landing_url(&validation.landing_state),
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
	let context = context::load_recovery_context_for_dry_run(config_path, request.dry_run)?;
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
			pull_request_inspection::landing_url(&validation.landing_state),
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

	review_handoff_apply::apply_review_handoff_adopt(&context, &validation)?;

	println!(
		"adopt ok: project={} issue={} branch={} pr={} head={} run_id={} attempt={}",
		context.config.service_id(),
		validation.issue.identifier,
		validation.branch_name,
		pull_request_inspection::landing_url(&validation.landing_state),
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
	let issue = issue::load_issue_by_identifier(&context.tracker, &request.issue)?;
	let worktree = issue::validate_rebind_issue_context(context, &issue)?;
	let existing_handoff = context.state_store.review_handoff_marker(
		context.config.service_id(),
		&issue.id,
		worktree.branch_name(),
	)?;
	let landing_state =
		pull_request_inspection::inspect_rebind_pull_request(context, &request.pr_url)?;
	let local_head_oid = worktree::validate_rebind_worktree(&worktree, &landing_state)?;
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
	let (run_id, attempt_number, mode) = issue::validate_rebind_existing_handoff(
		context,
		&issue,
		&worktree,
		existing_handoff.as_ref(),
		existing_orchestration.as_ref(),
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

	worktree::validate_adopt_absent_handoff_marker(
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
