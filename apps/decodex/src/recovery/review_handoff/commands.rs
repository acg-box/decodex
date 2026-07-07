use std::path::Path;

use crate::{
	prelude::Result,
	recovery::{
		context, pull_request_inspection,
		reports::{self, ReviewHandoffRecoveryReport},
		requests::{
			ReviewHandoffAdoptRequest, ReviewHandoffDiagnoseRequest, ReviewHandoffRebindRequest,
		},
		review_handoff_apply, review_handoff_diagnosis,
	},
	tracker,
};

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
		Some(issue_identifier) => {
			review_handoff_diagnosis::diagnose_issue(&context, issue_identifier)
				.map(|diagnostic| vec![diagnostic])
		},
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
	let validation = super::validate_rebind_request(&context, request)?;

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
	let validation = super::validate_adopt_request(&context, request)?;

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
