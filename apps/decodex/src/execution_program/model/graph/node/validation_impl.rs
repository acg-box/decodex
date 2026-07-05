use crate::{
	execution_program::{ExecutionProgramNode, validation},
	prelude::Result,
};

impl ExecutionProgramNode {
	pub(in crate::execution_program::model) fn validate(&self) -> Result<()> {
		validation::validate_required("execution program node.node_id", &self.node_id)?;
		validation::validate_required("execution program node.objective", &self.objective)?;
		validation::validate_string_list(
			"execution program node.objective_lineage",
			&self.objective_lineage,
		)?;
		validation::validate_string_list(
			"execution program node.acceptance_expectations",
			&self.acceptance_expectations,
		)?;
		validation::validate_string_list(
			"execution program node.validation_expectations",
			&self.validation_expectations,
		)?;
		validation::validate_optional(
			"execution program node.contract_fingerprint",
			validation::non_empty_optional(&self.contract_fingerprint),
		)?;

		for dependency in &self.dependencies {
			dependency.validate()?;
		}
		for domain in &self.conflict_domains {
			domain.validate()?;
		}

		if let Some(issue) = &self.linear_issue {
			issue.validate()?;
		}

		Ok(())
	}
}
