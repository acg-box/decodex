use crate::execution_program::ExecutionProgram;

/// SQLite-backed internal Execution Program retained by the local runtime.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ExecutionProgramRecord {
	pub(in crate::state) project_id: String,
	pub(in crate::state) program: ExecutionProgram,
	pub(in crate::state) source_contract_id: Option<String>,
	pub(in crate::state) created_at: String,
	pub(in crate::state) created_at_unix: i64,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}
#[allow(dead_code)]
impl ExecutionProgramRecord {
	pub(crate) fn project_id(&self) -> &str {
		&self.project_id
	}

	pub(crate) fn program(&self) -> &ExecutionProgram {
		&self.program
	}

	pub(crate) fn program_id(&self) -> &str {
		self.program.program_id()
	}

	pub(crate) fn source_contract_id(&self) -> Option<&str> {
		self.source_contract_id.as_deref()
	}

	pub(crate) fn created_at(&self) -> &str {
		&self.created_at
	}

	pub(crate) fn created_at_unix(&self) -> i64 {
		self.created_at_unix
	}

	pub(crate) fn updated_at(&self) -> &str {
		&self.updated_at
	}

	pub(crate) fn updated_at_unix(&self) -> i64 {
		self.updated_at_unix
	}
}
