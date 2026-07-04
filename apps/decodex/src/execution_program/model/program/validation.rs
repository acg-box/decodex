use std::collections::HashSet;

use crate::{
	execution_program::{
		intake::ProgramIntakeKind,
		model::{EXECUTION_PROGRAM_RECORD_VERSION, EXECUTION_PROGRAM_SCHEMA, ExecutionProgram},
		validation,
	},
	prelude::{Result, eyre},
};

impl ExecutionProgram {
	/// Validate the serialized program payload.
	pub(crate) fn validate(&self) -> Result<()> {
		validation::validate_required("execution program schema", &self.schema)?;
		validation::validate_required("execution program program_id", &self.program_id)?;
		validation::validate_required("execution program service_id", &self.service_id)?;
		validation::validate_optional(
			"execution program source_contract_id",
			self.source_contract_id.as_deref(),
		)?;
		validation::validate_required(
			"execution program accepted_contract_fingerprint",
			&self.accepted_contract_fingerprint,
		)?;

		if self.schema != EXECUTION_PROGRAM_SCHEMA {
			eyre::bail!(
				"Execution program `{}` has unsupported schema `{}`.",
				self.program_id,
				self.schema
			);
		}
		if self.record_version != EXECUTION_PROGRAM_RECORD_VERSION {
			eyre::bail!(
				"Execution program `{}` has unsupported record_version `{}`.",
				self.program_id,
				self.record_version
			);
		}

		if let Some(plan) = &self.program_intake_plan {
			self.validate_intake_plan(plan)?;
		}

		let mut node_ids = HashSet::new();

		for node in &self.nodes {
			node.validate()?;

			if !node_ids.insert(node.node_id.as_str()) {
				eyre::bail!(
					"Execution program `{}` contains duplicate node `{}`.",
					self.program_id,
					node.node_id
				);
			}
		}

		if self.program_intake_plan.is_none() && self.source_contract_id.as_deref().is_none() {
			eyre::bail!(
				"Execution program `{}` without a source contract must carry an issue-batch intake plan.",
				self.program_id
			);
		}

		Ok(())
	}

	fn validate_intake_plan(
		&self,
		plan: &crate::execution_program::intake::ProgramIntakePlan,
	) -> Result<()> {
		plan.validate()?;

		if plan.service_id != self.service_id {
			eyre::bail!(
				"Execution program `{}` belongs to service `{}` but intake plan belongs to `{}`.",
				self.program_id,
				self.service_id,
				plan.service_id
			);
		}

		if let Some(source_contract_id) = plan.source_contract_id()
			&& Some(source_contract_id) != self.source_contract_id.as_deref()
		{
			eyre::bail!(
				"Execution program `{}` belongs to source contract `{}` but intake plan belongs to `{}`.",
				self.program_id,
				self.source_contract_id.as_deref().unwrap_or("none"),
				source_contract_id
			);
		}

		if plan.intake_kind == ProgramIntakeKind::GoalIntake
			&& self.source_contract_id.as_deref().is_none_or(str::is_empty)
		{
			eyre::bail!(
				"Goal intake execution program `{}` must reference a source contract.",
				self.program_id
			);
		}
		if plan.intake_kind == ProgramIntakeKind::IssueBatchIntake
			&& self.source_contract_id.as_deref().is_some_and(|id| !id.is_empty())
		{
			eyre::bail!(
				"Issue-batch execution program `{}` must not reference a source contract.",
				self.program_id
			);
		}
		if plan.accepted_contract_fingerprint != self.accepted_contract_fingerprint {
			eyre::bail!(
				"Execution program `{}` has an intake plan fingerprint mismatch.",
				self.program_id
			);
		}

		Ok(())
	}
}
