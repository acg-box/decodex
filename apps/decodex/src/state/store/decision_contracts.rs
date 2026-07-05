mod mutation;
mod query;

use crate::{
	loop_contract::{DecisionContract, DecisionPromotion},
	prelude::Result,
	state::store::{DecisionContractRecord, StateStore},
};

impl StateStore {
	pub(crate) fn upsert_decision_contract(
		&self,
		project_id: &str,
		source_issue_id: Option<&str>,
		contract: DecisionContract,
	) -> Result<DecisionContractRecord> {
		mutation::upsert_decision_contract(self, project_id, source_issue_id, contract)
	}

	/// Read one local Loop/Decision Contract by project and contract id.
	#[allow(dead_code)]
	pub(crate) fn decision_contract(
		&self,
		project_id: &str,
		contract_id: &str,
	) -> Result<Option<DecisionContractRecord>> {
		query::decision_contract(self, project_id, contract_id)
	}

	/// List local Loop/Decision Contracts sourced from one tracker issue.
	#[allow(dead_code)]
	pub(crate) fn list_decision_contracts_for_issue(
		&self,
		project_id: &str,
		source_issue_id: &str,
	) -> Result<Vec<DecisionContractRecord>> {
		query::list_decision_contracts_for_issue(self, project_id, source_issue_id)
	}

	/// List local Loop/Decision Contracts for one project.
	#[allow(dead_code)]
	pub(crate) fn list_decision_contracts_for_project(
		&self,
		project_id: &str,
	) -> Result<Vec<DecisionContractRecord>> {
		query::list_decision_contracts_for_project(self, project_id)
	}

	/// Promote a latent Loop/Decision Contract into accepted execution authority.
	#[allow(dead_code)]
	pub(crate) fn promote_decision_contract(
		&self,
		project_id: &str,
		contract_id: &str,
		promotion: DecisionPromotion,
	) -> Result<DecisionContractRecord> {
		self.update_decision_contract(project_id, contract_id, |contract| {
			contract.promote(promotion)
		})
	}

	/// Mark a latent Loop/Decision Contract as waiting for more human direction.
	#[allow(dead_code)]
	pub(crate) fn mark_decision_contract_needs_human_decision(
		&self,
		project_id: &str,
		contract_id: &str,
		reason: &str,
	) -> Result<DecisionContractRecord> {
		self.update_decision_contract(project_id, contract_id, |contract| {
			contract.require_human_decision(reason.to_owned())
		})
	}

	/// Reject or supersede a Loop/Decision Contract.
	#[allow(dead_code)]
	pub(crate) fn reject_decision_contract(
		&self,
		project_id: &str,
		contract_id: &str,
		superseded_by_contract_id: Option<String>,
	) -> Result<DecisionContractRecord> {
		self.update_decision_contract(project_id, contract_id, |contract| {
			contract.reject_or_supersede(superseded_by_contract_id)
		})
	}

	#[allow(dead_code)]
	pub(super) fn update_decision_contract(
		&self,
		project_id: &str,
		contract_id: &str,
		update: impl FnOnce(&mut DecisionContract) -> Result<()>,
	) -> Result<DecisionContractRecord> {
		mutation::update_decision_contract(self, project_id, contract_id, update)
	}
}
