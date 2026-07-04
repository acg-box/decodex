//! Versioned Execution Program aggregate.

mod constructors;
mod evaluation;
mod validation;

use serde::{Deserialize, Serialize};

use crate::{
	execution_program::{
		ExecutionProgramNode,
		intake::ProgramIntakePlan,
		validation::{execution_program_record_version, execution_program_schema},
	},
	prelude::Result,
};

/// Versioned internal Execution Program derived from an accepted Decision Contract.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct ExecutionProgram {
	#[serde(default = "execution_program_schema")]
	schema: String,
	#[serde(default = "execution_program_record_version")]
	record_version: u16,
	pub(in crate::execution_program) program_id: String,
	pub(in crate::execution_program) service_id: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::execution_program) source_contract_id: Option<String>,
	pub(in crate::execution_program) accepted_contract_fingerprint: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	program_intake_plan: Option<ProgramIntakePlan>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(in crate::execution_program) nodes: Vec<ExecutionProgramNode>,
}
impl ExecutionProgram {
	/// Stable internal program id.
	pub(crate) fn program_id(&self) -> &str {
		&self.program_id
	}

	/// Service id that owns queue-label decisions.
	pub(crate) fn service_id(&self) -> &str {
		&self.service_id
	}

	/// Accepted Decision Contract id that authorized this program, for goal intake.
	pub(crate) fn source_contract_id(&self) -> Option<&str> {
		self.source_contract_id.as_deref()
	}

	/// Stable authority fingerprint for this program.
	pub(crate) fn accepted_contract_fingerprint(&self) -> &str {
		&self.accepted_contract_fingerprint
	}

	/// Durable program-intake plan metadata, when the payload is not a legacy row.
	pub(crate) fn program_intake_plan(&self) -> Option<&ProgramIntakePlan> {
		self.program_intake_plan.as_ref()
	}

	/// Program nodes.
	pub(crate) fn nodes(&self) -> &[ExecutionProgramNode] {
		&self.nodes
	}

	/// Replace program nodes after runtime reconciliation refreshes tracker issue facts.
	pub(crate) fn with_nodes(mut self, nodes: Vec<ExecutionProgramNode>) -> Result<Self> {
		self.nodes = nodes;

		self.validate()?;

		Ok(self)
	}
}
