mod mutation;
mod query;

use crate::{
	execution_program::ExecutionProgram,
	prelude::Result,
	state::store::{
		ExecutionProgramRecord, ProgramIntakePlanRecord, ProgramIssueMappingRecord, StateStore,
	},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProgramIntakeAttemptClaim {
	Acquired,
	Prepared,
	Started,
	Completed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProgramIntakeAttemptStatus {
	Prepared,
	Started,
	Completed,
}

impl StateStore {
	pub(crate) fn begin_program_intake_attempt(
		&self,
		project_id: &str,
		contract_id: &str,
		request_digest: &str,
	) -> Result<ProgramIntakeAttemptClaim> {
		mutation::begin_program_intake_attempt(self, project_id, contract_id, request_digest)
	}

	pub(crate) fn program_intake_attempt_status(
		&self,
		project_id: &str,
		contract_id: &str,
	) -> Result<Option<ProgramIntakeAttemptStatus>> {
		mutation::program_intake_attempt_status(self, project_id, contract_id)
	}

	pub(crate) fn mark_program_intake_attempt_started(
		&self,
		project_id: &str,
		contract_id: &str,
	) -> Result<()> {
		mutation::mark_program_intake_attempt_started(self, project_id, contract_id)
	}

	pub(crate) fn complete_program_intake_attempt(
		&self,
		project_id: &str,
		contract_id: &str,
	) -> Result<()> {
		mutation::complete_program_intake_attempt(self, project_id, contract_id)
	}

	/// Create or replace one local internal Execution Program payload.
	#[allow(dead_code)]
	pub(crate) fn upsert_execution_program(
		&self,
		project_id: &str,
		program: ExecutionProgram,
	) -> Result<ExecutionProgramRecord> {
		mutation::upsert_execution_program(self, project_id, program)
	}

	/// Delete one superseded private Execution Program and its derived intake state.
	pub(crate) fn delete_execution_program(
		&self,
		project_id: &str,
		program_id: &str,
	) -> Result<()> {
		mutation::delete_execution_program(self, project_id, program_id)
	}

	/// Read one local internal Execution Program by project and program id.
	#[allow(dead_code)]
	pub(crate) fn execution_program(
		&self,
		project_id: &str,
		program_id: &str,
	) -> Result<Option<ExecutionProgramRecord>> {
		query::execution_program(self, project_id, program_id)
	}

	/// List local internal Execution Programs derived from one Decision Contract.
	#[allow(dead_code)]
	pub(crate) fn list_execution_programs_for_contract(
		&self,
		project_id: &str,
		source_contract_id: &str,
	) -> Result<Vec<ExecutionProgramRecord>> {
		query::list_execution_programs_for_contract(self, project_id, source_contract_id)
	}

	/// List local internal Execution Programs retained for one project.
	#[allow(dead_code)]
	pub(crate) fn list_execution_programs(
		&self,
		project_id: &str,
	) -> Result<Vec<ExecutionProgramRecord>> {
		query::list_execution_programs(self, project_id)
	}

	/// List local Program Intake Plan records retained for one project.
	#[allow(dead_code)]
	pub(crate) fn list_program_intake_plans(
		&self,
		project_id: &str,
	) -> Result<Vec<ProgramIntakePlanRecord>> {
		query::list_program_intake_plans(self, project_id)
	}

	/// List local issue mappings for one internal Execution Program.
	#[allow(dead_code)]
	pub(crate) fn list_program_issue_mappings(
		&self,
		project_id: &str,
		program_id: &str,
	) -> Result<Vec<ProgramIssueMappingRecord>> {
		query::list_program_issue_mappings(self, project_id, program_id)
	}
}
