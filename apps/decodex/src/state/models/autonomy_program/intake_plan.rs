/// SQLite-backed Program Intake Plan projection retained by the local runtime.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProgramIntakePlanRecord {
	pub(in crate::state) project_id: String,
	pub(in crate::state) program_id: String,
	pub(in crate::state) plan_id: String,
	pub(in crate::state) intake_kind: String,
	pub(in crate::state) source_contract_id: Option<String>,
	pub(in crate::state) accepted_contract_fingerprint: String,
	pub(in crate::state) public_summary: String,
	pub(in crate::state) created_at: String,
	pub(in crate::state) created_at_unix: i64,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}
#[allow(dead_code)]
impl ProgramIntakePlanRecord {
	pub(crate) fn project_id(&self) -> &str {
		&self.project_id
	}

	pub(crate) fn program_id(&self) -> &str {
		&self.program_id
	}

	pub(crate) fn plan_id(&self) -> &str {
		&self.plan_id
	}

	pub(crate) fn intake_kind(&self) -> &str {
		&self.intake_kind
	}

	pub(crate) fn source_contract_id(&self) -> Option<&str> {
		self.source_contract_id.as_deref()
	}

	pub(crate) fn accepted_contract_fingerprint(&self) -> &str {
		&self.accepted_contract_fingerprint
	}

	pub(crate) fn public_summary(&self) -> &str {
		&self.public_summary
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
