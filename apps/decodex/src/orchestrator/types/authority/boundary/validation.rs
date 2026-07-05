use crate::orchestrator::types::{
	AuthorityBoundaryCheckInput, AuthorityBoundaryPolicyDecision, Result, eyre,
};

pub(crate) fn validate_authority_boundary_check_input(
	input: &AuthorityBoundaryCheckInput<'_>,
) -> Result<()> {
	authority_boundary_required("authority boundary project_id", input.project_id)?;
	authority_boundary_required("authority boundary issue_id", input.issue_id)?;
	authority_boundary_required("authority boundary issue_identifier", input.issue_identifier)?;
	authority_boundary_required("authority boundary run_id", input.run_id)?;
	authority_boundary_required(
		"authority boundary attempted_recovery_reason",
		input.attempted_recovery_reason,
	)?;
	authority_boundary_required(
		"authority boundary final_disposition_reason",
		input.final_disposition_reason,
	)?;

	if input.attempt_number < 1 {
		eyre::bail!("Authority boundary attempt_number must be positive.");
	}
	if input.changed_surfaces.is_empty() {
		eyre::bail!("Authority boundary changed_surfaces must not be empty.");
	}

	let mut expected_policy_decision = AuthorityBoundaryPolicyDecision::AutoContinue;

	for surface in &input.changed_surfaces {
		let surface_policy_decision = surface.surface.policy_decision();

		if surface.policy_decision != surface_policy_decision {
			eyre::bail!(
				"Authority boundary surface `{}` must use policy decision `{}`.",
				surface.surface.as_str(),
				surface_policy_decision.as_str()
			);
		}
		if surface.legacy_disposition != surface_policy_decision.disposition() {
			eyre::bail!(
				"Authority boundary surface `{}` must use legacy disposition `{}`.",
				surface.surface.as_str(),
				surface_policy_decision.disposition().as_str()
			);
		}

		expected_policy_decision =
			AuthorityBoundaryPolicyDecision::max(expected_policy_decision, surface_policy_decision);
	}

	if input.policy_decision != expected_policy_decision {
		eyre::bail!(
			"Authority boundary policy_decision must be `{}` for the changed surfaces.",
			expected_policy_decision.as_str()
		);
	}
	if input.disposition != input.policy_decision.disposition() {
		eyre::bail!(
			"Authority boundary disposition must be `{}` for policy decision `{}`.",
			input.policy_decision.disposition().as_str(),
			input.policy_decision.as_str()
		);
	}

	for contract_id in &input.decision_contract_ids {
		authority_boundary_required("authority boundary decision_contract_id", contract_id)?;
	}
	for surface in &input.changed_surfaces {
		authority_boundary_required(
			"authority boundary changed surface summary",
			surface.change_summary,
		)?;
	}
	for signal in &input.improvement_signals {
		authority_boundary_required("authority boundary improvement kind", signal.kind)?;
		authority_boundary_required(
			"authority boundary improvement reason_code",
			signal.reason_code,
		)?;
		authority_boundary_required("authority boundary improvement target", signal.target)?;
		authority_boundary_required(
			"authority boundary improvement recommendation",
			signal.recommendation,
		)?;
	}

	Ok(())
}

pub(crate) fn authority_boundary_required(name: &str, value: &str) -> Result<()> {
	if value.trim().is_empty() {
		eyre::bail!("{name} must not be empty.");
	}

	Ok(())
}
