use crate::{
	execution_program::{
		ExecutionConflictDomain, ExecutionLinearIssueMapping, ExecutionProgramDependency,
		ExecutionProgramNode, ExecutionProgramNodeStage, ExecutionQueueIntent,
	},
	prelude::Result,
};

impl ExecutionProgramNode {
	/// Build a program node.
	pub(crate) fn new(
		node_id: impl Into<String>,
		stage: ExecutionProgramNodeStage,
		objective: impl Into<String>,
		queue_intent: ExecutionQueueIntent,
	) -> Result<Self> {
		let node = Self {
			node_id: node_id.into(),
			stage,
			objective: objective.into(),
			objective_lineage: Vec::new(),
			dependencies: Vec::new(),
			conflict_domains: Vec::new(),
			acceptance_expectations: Vec::new(),
			validation_expectations: Vec::new(),
			queue_intent,
			linear_issue: None,
			contract_fingerprint: String::new(),
		};

		node.validate()?;

		Ok(node)
	}

	/// Add objective-lineage text from the accepted contract.
	pub(crate) fn with_objective_lineage(
		mut self,
		lineage: impl IntoIterator<Item = impl Into<String>>,
	) -> Result<Self> {
		self.objective_lineage = lineage.into_iter().map(Into::into).collect();

		self.validate()?;

		Ok(self)
	}

	/// Add dependencies.
	pub(crate) fn with_dependencies(
		mut self,
		dependencies: impl IntoIterator<Item = ExecutionProgramDependency>,
	) -> Result<Self> {
		self.dependencies = dependencies.into_iter().collect();

		self.validate()?;

		Ok(self)
	}

	/// Add conflict domains.
	pub(crate) fn with_conflict_domains(
		mut self,
		conflict_domains: impl IntoIterator<Item = ExecutionConflictDomain>,
	) -> Result<Self> {
		self.conflict_domains = conflict_domains.into_iter().collect();

		self.validate()?;

		Ok(self)
	}

	/// Add acceptance expectations.
	pub(crate) fn with_acceptance_expectations(
		mut self,
		expectations: impl IntoIterator<Item = impl Into<String>>,
	) -> Result<Self> {
		self.acceptance_expectations = expectations.into_iter().map(Into::into).collect();

		self.validate()?;

		Ok(self)
	}

	/// Add validation expectations.
	pub(crate) fn with_validation_expectations(
		mut self,
		expectations: impl IntoIterator<Item = impl Into<String>>,
	) -> Result<Self> {
		self.validation_expectations = expectations.into_iter().map(Into::into).collect();

		self.validate()?;

		Ok(self)
	}

	/// Link the node to a normal Linear issue.
	pub(crate) fn with_linear_issue(mut self, issue: ExecutionLinearIssueMapping) -> Result<Self> {
		self.linear_issue = Some(issue);

		self.validate()?;

		Ok(self)
	}

	/// Refresh dispatch intent from the owning intake authority.
	pub(crate) fn with_queue_intent(mut self, queue_intent: ExecutionQueueIntent) -> Result<Self> {
		self.queue_intent = queue_intent;

		self.validate()?;

		Ok(self)
	}

	/// Override the accepted-contract fingerprint used for drift detection.
	#[cfg(test)]
	pub(crate) fn with_contract_fingerprint(
		mut self,
		fingerprint: impl Into<String>,
	) -> Result<Self> {
		self.contract_fingerprint = fingerprint.into();

		self.validate()?;

		Ok(self)
	}
}
