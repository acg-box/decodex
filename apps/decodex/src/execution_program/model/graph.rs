//! Execution Program graph node and edge types.

use serde::{Deserialize, Serialize};

use super::{
	super::validation::{
		non_empty_optional, validate_optional, validate_required, validate_string_list,
	},
	ExecutionConflictDomainKind, ExecutionLinearIssueMapping, ExecutionProgramNodeStage,
	ExecutionQueueIntent,
};
use crate::prelude::Result;

/// Conflict-domain key for one program node.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Deserialize, Serialize)]
pub(crate) struct ExecutionConflictDomain {
	pub(in crate::execution_program) kind: ExecutionConflictDomainKind,
	key: String,
}
impl ExecutionConflictDomain {
	/// Build a conflict-domain key.
	pub(crate) fn new(kind: ExecutionConflictDomainKind, key: impl Into<String>) -> Result<Self> {
		let domain = Self { kind, key: key.into() };

		domain.validate()?;

		Ok(domain)
	}

	/// Stable conflict-domain key.
	pub(crate) fn key(&self) -> &str {
		&self.key
	}

	/// Stable conflict-domain kind.
	pub(crate) fn kind(&self) -> ExecutionConflictDomainKind {
		self.kind
	}

	pub(super) fn validate(&self) -> Result<()> {
		validate_required("execution program conflict_domain.key", &self.key)
	}
}

/// Dependency edge for one program node.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct ExecutionProgramDependency {
	pub(in crate::execution_program) dependency_id: String,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(in crate::execution_program) required_terminal_states: Vec<String>,
}
impl ExecutionProgramDependency {
	/// Build a dependency edge using the registered workflow terminal states.
	pub(crate) fn new(dependency_id: impl Into<String>) -> Result<Self> {
		let dependency =
			Self { dependency_id: dependency_id.into(), required_terminal_states: Vec::new() };

		dependency.validate()?;

		Ok(dependency)
	}

	/// Override the terminal tracker states that satisfy this dependency.
	pub(crate) fn with_required_terminal_states(
		mut self,
		states: impl IntoIterator<Item = impl Into<String>>,
	) -> Result<Self> {
		self.required_terminal_states = states.into_iter().map(Into::into).collect();

		self.validate()?;

		Ok(self)
	}

	/// Dependency node or issue identifier.
	pub(crate) fn dependency_id(&self) -> &str {
		&self.dependency_id
	}

	pub(super) fn validate(&self) -> Result<()> {
		validate_required("execution program dependency.dependency_id", &self.dependency_id)?;

		validate_string_list(
			"execution program dependency.required_terminal_states",
			&self.required_terminal_states,
		)
	}
}

/// Internal node in an Execution Program.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct ExecutionProgramNode {
	pub(in crate::execution_program) node_id: String,
	pub(in crate::execution_program) stage: ExecutionProgramNodeStage,
	objective: String,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	objective_lineage: Vec<String>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(in crate::execution_program) dependencies: Vec<ExecutionProgramDependency>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(in crate::execution_program) conflict_domains: Vec<ExecutionConflictDomain>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(in crate::execution_program) acceptance_expectations: Vec<String>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(in crate::execution_program) validation_expectations: Vec<String>,
	pub(in crate::execution_program) queue_intent: ExecutionQueueIntent,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::execution_program) linear_issue: Option<ExecutionLinearIssueMapping>,
	pub(in crate::execution_program) contract_fingerprint: String,
}
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

	/// Override the accepted-contract fingerprint used for drift detection.
	pub(crate) fn with_contract_fingerprint(
		mut self,
		fingerprint: impl Into<String>,
	) -> Result<Self> {
		self.contract_fingerprint = fingerprint.into();

		self.validate()?;

		Ok(self)
	}

	/// Stable internal node id.
	pub(crate) fn node_id(&self) -> &str {
		&self.node_id
	}

	/// Node execution stage.
	pub(crate) fn stage(&self) -> ExecutionProgramNodeStage {
		self.stage
	}

	/// Node queue intent.
	pub(crate) fn queue_intent(&self) -> ExecutionQueueIntent {
		self.queue_intent
	}

	/// Conflict domains occupied by this node.
	pub(crate) fn conflict_domains(&self) -> &[ExecutionConflictDomain] {
		&self.conflict_domains
	}

	/// Linked normal Linear issue, when the node is executable.
	pub(crate) fn linear_issue(&self) -> Option<&ExecutionLinearIssueMapping> {
		self.linear_issue.as_ref()
	}

	pub(super) fn bind_contract_fingerprint(&mut self, fingerprint: &str) {
		if self.contract_fingerprint.is_empty() {
			self.contract_fingerprint = fingerprint.to_owned();
		}
	}

	pub(super) fn validate(&self) -> Result<()> {
		validate_required("execution program node.node_id", &self.node_id)?;
		validate_required("execution program node.objective", &self.objective)?;
		validate_string_list("execution program node.objective_lineage", &self.objective_lineage)?;
		validate_string_list(
			"execution program node.acceptance_expectations",
			&self.acceptance_expectations,
		)?;
		validate_string_list(
			"execution program node.validation_expectations",
			&self.validation_expectations,
		)?;
		validate_optional(
			"execution program node.contract_fingerprint",
			non_empty_optional(&self.contract_fingerprint),
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
