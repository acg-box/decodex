use crate::loop_contract::{DecisionContract, DecisionContractStatus};

/// SQLite-backed Loop/Decision Contract retained by the local runtime.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DecisionContractRecord {
	pub(in crate::state) project_id: String,
	pub(in crate::state) source_issue_id: Option<String>,
	pub(in crate::state) contract: DecisionContract,
	pub(in crate::state) status: DecisionContractStatus,
	pub(in crate::state) created_at: String,
	pub(in crate::state) created_at_unix: i64,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}
#[allow(dead_code)]
impl DecisionContractRecord {
	pub(crate) fn project_id(&self) -> &str {
		&self.project_id
	}

	pub(crate) fn source_issue_id(&self) -> Option<&str> {
		self.source_issue_id.as_deref()
	}

	pub(crate) fn contract(&self) -> &DecisionContract {
		&self.contract
	}

	pub(crate) fn contract_id(&self) -> &str {
		self.contract.contract_id()
	}

	pub(crate) fn status(&self) -> DecisionContractStatus {
		self.status
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
