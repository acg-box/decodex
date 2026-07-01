use crate::{
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
