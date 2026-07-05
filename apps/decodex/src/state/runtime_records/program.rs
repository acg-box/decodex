use crate::{
	execution_program::ExecutionProgram,
	loop_contract::{DecisionContract, DecisionContractStatus},
	state::DecisionContractRecord,
};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(in crate::state) struct DecisionContractKey {
	pub(in crate::state) project_id: String,
	pub(in crate::state) contract_id: String,
}
impl DecisionContractKey {
	pub(in crate::state) fn new(project_id: &str, contract_id: &str) -> Self {
		Self { project_id: project_id.to_owned(), contract_id: contract_id.to_owned() }
	}
}

#[derive(Clone, Debug)]
pub(in crate::state) struct DecisionContractRuntimeRecord {
	pub(in crate::state) project_id: String,
	pub(in crate::state) source_issue_id: Option<String>,
	pub(in crate::state) contract: DecisionContract,
	pub(in crate::state) status: DecisionContractStatus,
	pub(in crate::state) created_at: String,
	pub(in crate::state) created_at_unix: i64,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}
impl DecisionContractRuntimeRecord {
	#[allow(dead_code)]
	pub(in crate::state) fn key(&self) -> DecisionContractKey {
		DecisionContractKey::new(&self.project_id, self.contract.contract_id())
	}

	#[allow(dead_code)]
	pub(in crate::state) fn as_public(&self) -> DecisionContractRecord {
		DecisionContractRecord {
			project_id: self.project_id.clone(),
			source_issue_id: self.source_issue_id.clone(),
			contract: self.contract.clone(),
			status: self.status,
			created_at: self.created_at.clone(),
			created_at_unix: self.created_at_unix,
			updated_at: self.updated_at.clone(),
			updated_at_unix: self.updated_at_unix,
		}
	}
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(in crate::state) struct ExecutionProgramKey {
	pub(in crate::state) project_id: String,
	pub(in crate::state) program_id: String,
}
impl ExecutionProgramKey {
	pub(in crate::state) fn new(project_id: &str, program_id: &str) -> Self {
		Self { project_id: project_id.to_owned(), program_id: program_id.to_owned() }
	}
}

#[derive(Clone, Debug)]
pub(in crate::state) struct ExecutionProgramRuntimeRecord {
	pub(in crate::state) project_id: String,
	pub(in crate::state) source_contract_id: Option<String>,
	pub(in crate::state) program: ExecutionProgram,
	pub(in crate::state) created_at: String,
	pub(in crate::state) created_at_unix: i64,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}
impl ExecutionProgramRuntimeRecord {
	#[allow(dead_code)]
	pub(in crate::state) fn key(&self) -> ExecutionProgramKey {
		ExecutionProgramKey::new(&self.project_id, self.program.program_id())
	}

	#[allow(dead_code)]
	pub(in crate::state) fn as_public(&self) -> crate::state::ExecutionProgramRecord {
		crate::state::ExecutionProgramRecord {
			project_id: self.project_id.clone(),
			program: self.program.clone(),
			source_contract_id: self.source_contract_id.clone(),
			created_at: self.created_at.clone(),
			created_at_unix: self.created_at_unix,
			updated_at: self.updated_at.clone(),
			updated_at_unix: self.updated_at_unix,
		}
	}
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(in crate::state) struct ProgramIntakePlanKey {
	pub(in crate::state) project_id: String,
	pub(in crate::state) program_id: String,
	pub(in crate::state) plan_id: String,
}
impl ProgramIntakePlanKey {
	pub(in crate::state) fn new(project_id: &str, program_id: &str, plan_id: &str) -> Self {
		Self {
			project_id: project_id.to_owned(),
			program_id: program_id.to_owned(),
			plan_id: plan_id.to_owned(),
		}
	}
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(in crate::state) struct ProgramIssueMappingKey {
	pub(in crate::state) project_id: String,
	pub(in crate::state) program_id: String,
	pub(in crate::state) node_id: String,
}
impl ProgramIssueMappingKey {
	pub(in crate::state) fn new(project_id: &str, program_id: &str, node_id: &str) -> Self {
		Self {
			project_id: project_id.to_owned(),
			program_id: program_id.to_owned(),
			node_id: node_id.to_owned(),
		}
	}
}

pub(in crate::state) struct DecisionContractRuntimeRowParts {
	pub(in crate::state) project_id: String,
	pub(in crate::state) contract_id: String,
	pub(in crate::state) source_issue_id: Option<String>,
	pub(in crate::state) status: String,
	pub(in crate::state) payload_json: String,
	pub(in crate::state) created_at: String,
	pub(in crate::state) created_at_unix: i64,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}

pub(in crate::state) struct ExecutionProgramRuntimeRowParts {
	pub(in crate::state) project_id: String,
	pub(in crate::state) program_id: String,
	pub(in crate::state) source_contract_id: Option<String>,
	pub(in crate::state) payload_json: String,
	pub(in crate::state) created_at: String,
	pub(in crate::state) created_at_unix: i64,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}
