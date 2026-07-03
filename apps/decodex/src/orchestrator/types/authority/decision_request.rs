use crate::orchestrator::types::{Result, authority::boundary, eyre};

/// One public-safe option offered in a durable authority decision request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AuthorityDecisionOption<'a> {
	pub(crate) label: &'a str,
	pub(crate) description: &'a str,
}

/// Input for persisting the full local decision packet for an authority-boundary stop.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AuthorityDecisionRequestInput<'a> {
	pub(crate) project_id: &'a str,
	pub(crate) issue_id: &'a str,
	pub(crate) issue_identifier: &'a str,
	pub(crate) run_id: &'a str,
	pub(crate) attempt_number: i64,
	pub(crate) boundary_check_record_id: i64,
	pub(crate) decision_request_id: &'a str,
	pub(crate) reason_code: &'a str,
	pub(crate) boundary_type: &'a str,
	pub(crate) proposed_change: &'a str,
	pub(crate) why_exceeds_authority: &'a str,
	pub(crate) options: Vec<AuthorityDecisionOption<'a>>,
	pub(crate) recommendation: &'a str,
	pub(crate) resume_condition: &'a str,
	pub(crate) retained_worktree_evidence: Vec<&'a str>,
	pub(crate) retained_diff_evidence: Vec<&'a str>,
	pub(crate) recovery_attempt_context: Vec<&'a str>,
}

pub(crate) fn validate_authority_decision_request_input(
	input: &AuthorityDecisionRequestInput<'_>,
) -> Result<()> {
	boundary::authority_boundary_required("authority decision project_id", input.project_id)?;
	boundary::authority_boundary_required("authority decision issue_id", input.issue_id)?;
	boundary::authority_boundary_required(
		"authority decision issue_identifier",
		input.issue_identifier,
	)?;
	boundary::authority_boundary_required("authority decision run_id", input.run_id)?;
	boundary::authority_boundary_required(
		"authority decision decision_request_id",
		input.decision_request_id,
	)?;
	boundary::authority_boundary_required("authority decision reason_code", input.reason_code)?;
	boundary::authority_boundary_required("authority decision boundary_type", input.boundary_type)?;
	boundary::authority_boundary_required(
		"authority decision proposed_change",
		input.proposed_change,
	)?;
	boundary::authority_boundary_required(
		"authority decision why_exceeds_authority",
		input.why_exceeds_authority,
	)?;
	boundary::authority_boundary_required(
		"authority decision recommendation",
		input.recommendation,
	)?;
	boundary::authority_boundary_required(
		"authority decision resume_condition",
		input.resume_condition,
	)?;

	if input.attempt_number < 1 {
		eyre::bail!("Authority decision attempt_number must be positive.");
	}
	if input.boundary_check_record_id < 1 {
		eyre::bail!("Authority decision boundary_check_record_id must be positive.");
	}
	if input.options.is_empty() {
		eyre::bail!("Authority decision options must not be empty.");
	}

	for option in &input.options {
		boundary::authority_boundary_required("authority decision option label", option.label)?;
		boundary::authority_boundary_required(
			"authority decision option description",
			option.description,
		)?;
	}
	for evidence in &input.retained_worktree_evidence {
		boundary::authority_boundary_required(
			"authority decision retained_worktree_evidence",
			evidence,
		)?;
	}
	for evidence in &input.retained_diff_evidence {
		boundary::authority_boundary_required(
			"authority decision retained_diff_evidence",
			evidence,
		)?;
	}
	for context in &input.recovery_attempt_context {
		boundary::authority_boundary_required(
			"authority decision recovery_attempt_context",
			context,
		)?;
	}

	Ok(())
}
