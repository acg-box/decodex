use crate::{
	execution_program::{
		intake::{
			PROGRAM_INTAKE_PLAN_RECORD_VERSION, PROGRAM_INTAKE_PLAN_SCHEMA, ProgramIntakeKind,
			ProgramIntakePlan,
		},
		validation::{self},
	},
	prelude::{Result, eyre},
};

pub(in crate::execution_program) fn validate_program_intake_plan(
	plan: &ProgramIntakePlan,
) -> Result<()> {
	validation::validate_required("program intake plan schema", plan.schema())?;
	validation::validate_required("program intake plan plan_id", plan.plan_id())?;
	validation::validate_required("program intake plan service_id", plan.service_id())?;
	validation::validate_required(
		"program intake plan accepted_contract_fingerprint",
		plan.accepted_contract_fingerprint(),
	)?;
	validation::validate_required("program intake plan public_summary", plan.public_summary())?;

	if plan.schema() != PROGRAM_INTAKE_PLAN_SCHEMA {
		eyre::bail!(
			"Program intake plan `{}` has unsupported schema `{}`.",
			plan.plan_id(),
			plan.schema()
		);
	}
	if plan.record_version() != PROGRAM_INTAKE_PLAN_RECORD_VERSION {
		eyre::bail!(
			"Program intake plan `{}` has unsupported record_version `{}`.",
			plan.plan_id(),
			plan.record_version()
		);
	}
	if plan.intake_kind() == ProgramIntakeKind::GoalIntake
		&& plan.source_contract_id().is_none_or(str::is_empty)
	{
		eyre::bail!("Goal intake plan `{}` must reference a source contract.", plan.plan_id());
	}

	validate_issue_batch_lineage(plan)?;

	validation::validate_optional(
		"program intake plan source_objective_ref",
		plan.source_objective_ref(),
	)?;
	validation::validate_optional(
		"program intake plan source_proposal_id",
		plan.source_proposal_id(),
	)?;
	validation::validate_string_list(
		"program intake plan source_signal_refs",
		plan.source_signal_refs(),
	)?;

	Ok(())
}

fn validate_issue_batch_lineage(plan: &ProgramIntakePlan) -> Result<()> {
	if plan.intake_kind() != ProgramIntakeKind::IssueBatchIntake {
		return Ok(());
	}
	if plan.source_contract_id().is_some_and(|id| !id.is_empty()) {
		eyre::bail!(
			"Issue-batch intake plan `{}` must not reference a source contract.",
			plan.plan_id()
		);
	}
	if plan.source_objective_ref().is_some_and(|id| !id.is_empty()) {
		eyre::bail!(
			"Issue-batch intake plan `{}` must not reference autonomy objective lineage.",
			plan.plan_id()
		);
	}
	if plan.source_proposal_id().is_some_and(|id| !id.is_empty()) {
		eyre::bail!(
			"Issue-batch intake plan `{}` must not reference autonomy proposal lineage.",
			plan.plan_id()
		);
	}
	if !plan.source_signal_refs().is_empty() {
		eyre::bail!(
			"Issue-batch intake plan `{}` must not reference autonomy signal lineage.",
			plan.plan_id()
		);
	}

	Ok(())
}
