use crate::{
	autonomy_objective::AutonomyObjectiveContract,
	autonomy_proposal::AutonomyProposal,
	autonomy_signal::AutonomySignal,
	execution_program::ExecutionProgram,
	loop_contract::DecisionContract,
	prelude::{Result, eyre},
};

pub(in crate::state::store) fn validate_decision_contract_record_inputs(
	project_id: &str,
	source_issue_id: Option<&str>,
	contract: &DecisionContract,
) -> Result<()> {
	validate_required_decision_contract_field("project_id", project_id)?;

	if let Some(source_issue_id) = source_issue_id {
		validate_required_decision_contract_field("source_issue_id", source_issue_id)?;
	}

	contract.validate()
}

pub(in crate::state::store) fn validate_required_decision_contract_field(
	name: &str,
	value: &str,
) -> Result<()> {
	if value.trim().is_empty() {
		eyre::bail!("Decision contract {name} must not be empty.");
	}

	Ok(())
}

pub(in crate::state::store) fn validate_autonomy_objective_record_inputs(
	project_id: &str,
	objective: &AutonomyObjectiveContract,
) -> Result<()> {
	validate_required_autonomy_objective_field("project_id", project_id)?;

	if objective.project_id() != project_id {
		eyre::bail!(
			"Autonomy objective `{}` belongs to project `{}` but was stored for `{}`.",
			objective.id(),
			objective.project_id(),
			project_id
		);
	}

	objective.validate()
}

pub(in crate::state::store) fn validate_required_autonomy_objective_field(
	name: &str,
	value: &str,
) -> Result<()> {
	if value.trim().is_empty() {
		eyre::bail!("Autonomy objective {name} must not be empty.");
	}

	Ok(())
}

pub(in crate::state::store) fn validate_autonomy_objective_version(version: u64) -> Result<()> {
	if version == 0 {
		eyre::bail!("Autonomy objective version must be greater than zero.");
	}

	Ok(())
}

pub(in crate::state::store) fn validate_autonomy_signal_record_inputs(
	project_id: &str,
	signal: &AutonomySignal,
) -> Result<()> {
	validate_required_autonomy_signal_field("project_id", project_id)?;

	if signal.project_id() != project_id {
		eyre::bail!(
			"Autonomy signal `{}` belongs to project `{}` but was stored for `{}`.",
			signal.id(),
			signal.project_id(),
			project_id
		);
	}

	signal.validate()
}

pub(in crate::state::store) fn validate_required_autonomy_signal_field(
	name: &str,
	value: &str,
) -> Result<()> {
	if value.trim().is_empty() {
		eyre::bail!("Autonomy signal {name} must not be empty.");
	}

	Ok(())
}

pub(in crate::state::store) fn validate_autonomy_proposal_record_inputs(
	project_id: &str,
	proposal: &AutonomyProposal,
) -> Result<()> {
	validate_required_autonomy_proposal_field("project_id", project_id)?;

	if proposal.project_id() != project_id {
		eyre::bail!(
			"Autonomy proposal `{}` belongs to project `{}` but was stored for `{}`.",
			proposal.id(),
			proposal.project_id(),
			project_id
		);
	}

	proposal.validate()
}

pub(in crate::state::store) fn validate_required_autonomy_proposal_field(
	name: &str,
	value: &str,
) -> Result<()> {
	if value.trim().is_empty() {
		eyre::bail!("Autonomy proposal {name} must not be empty.");
	}

	Ok(())
}

pub(in crate::state::store) fn validate_execution_program_record_inputs(
	project_id: &str,
	program: &ExecutionProgram,
) -> Result<()> {
	validate_required_execution_program_field("project_id", project_id)?;

	program.validate()
}

pub(in crate::state::store) fn validate_required_execution_program_field(
	name: &str,
	value: &str,
) -> Result<()> {
	if value.trim().is_empty() {
		eyre::bail!("Execution program {name} must not be empty.");
	}

	Ok(())
}
