mod accessors;
mod builder;
mod validation_impl;

use serde::{Deserialize, Serialize};

use crate::execution_program::model::{
	ExecutionConflictDomain, ExecutionLinearIssueMapping, ExecutionProgramDependency,
	ExecutionProgramNodeStage, ExecutionQueueIntent,
};

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
