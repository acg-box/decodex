use crate::{
	loop_contract::{DecisionContract, DecisionPromotion},
	prelude::{Result, eyre},
};

use super::{
	super::runtime_records::{DecisionContractKey, DecisionContractRuntimeRecord},
	DecisionContractRecord, StateStore, compare_decision_contract_runtime_records, timestamp_parts,
	validate_decision_contract_record_inputs, validate_required_decision_contract_field,
};

impl StateStore {
	pub(crate) fn upsert_decision_contract(
		&self,
		project_id: &str,
		source_issue_id: Option<&str>,
		contract: DecisionContract,
	) -> Result<DecisionContractRecord> {
		validate_decision_contract_record_inputs(project_id, source_issue_id, &contract)?;

		let now = timestamp_parts();
		let mut state = self.lock_without_refresh()?;
		let key = DecisionContractKey::new(project_id, contract.contract_id());
		let (created_at, created_at_unix) = state.decision_contracts.get(&key).map_or_else(
			|| (now.text.clone(), now.unix),
			|record| (record.created_at.clone(), record.created_at_unix),
		);
		let record = DecisionContractRuntimeRecord {
			project_id: project_id.to_owned(),
			source_issue_id: source_issue_id.map(str::to_owned),
			status: contract.status(),
			contract,
			created_at,
			created_at_unix,
			updated_at: now.text,
			updated_at_unix: now.unix,
		};

		state.decision_contracts.insert(record.key(), record.clone());
		self.upsert_decision_contract_locked(&record)?;

		Ok(record.as_public())
	}

	/// Read one local Loop/Decision Contract by project and contract id.
	#[allow(dead_code)]
	pub(crate) fn decision_contract(
		&self,
		project_id: &str,
		contract_id: &str,
	) -> Result<Option<DecisionContractRecord>> {
		validate_required_decision_contract_field("project_id", project_id)?;
		validate_required_decision_contract_field("contract_id", contract_id)?;

		if let Some(sqlite) = &self.sqlite {
			let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;

			return sqlite
				.decision_contract(project_id, contract_id)
				.map(|record| record.map(|record| record.as_public()));
		}

		let state = self.lock()?;

		Ok(state
			.decision_contracts
			.get(&DecisionContractKey::new(project_id, contract_id))
			.map(DecisionContractRuntimeRecord::as_public))
	}

	/// Read one local Loop/Decision Contract for non-mutating readback/reconciliation only.
	///
	/// Readback and scheduler reconciliation treat quarantined legacy contract payloads as
	/// absent so stale Programs cannot crash operator surfaces or direct Program selection,
	/// while strict execution-facing reads still fail closed on removed contract shapes.
	pub(crate) fn decision_contract_for_readback(
		&self,
		project_id: &str,
		contract_id: &str,
	) -> Result<Option<DecisionContractRecord>> {
		validate_required_decision_contract_field("project_id", project_id)?;
		validate_required_decision_contract_field("contract_id", contract_id)?;

		if let Some(sqlite) = &self.sqlite {
			let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;

			return sqlite
				.decision_contract_for_readback(project_id, contract_id)
				.map(|record| record.map(|record| record.as_public()));
		}

		let state = self.lock()?;

		Ok(state
			.decision_contracts
			.get(&DecisionContractKey::new(project_id, contract_id))
			.map(DecisionContractRuntimeRecord::as_public))
	}

	/// List local Loop/Decision Contracts sourced from one tracker issue.
	#[allow(dead_code)]
	pub(crate) fn list_decision_contracts_for_issue(
		&self,
		project_id: &str,
		source_issue_id: &str,
	) -> Result<Vec<DecisionContractRecord>> {
		validate_required_decision_contract_field("project_id", project_id)?;
		validate_required_decision_contract_field("source_issue_id", source_issue_id)?;

		if let Some(sqlite) = &self.sqlite {
			let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;
			let records = sqlite
				.list_decision_contracts_for_issue(project_id, source_issue_id)?
				.into_iter()
				.map(|record| record.as_public())
				.collect();

			return Ok(records);
		}

		let state = self.lock()?;
		let mut records = state
			.decision_contracts
			.values()
			.filter(|record| {
				record.project_id == project_id
					&& record.source_issue_id.as_deref() == Some(source_issue_id)
			})
			.cloned()
			.collect::<Vec<_>>();

		records.sort_by(compare_decision_contract_runtime_records);

		Ok(records.into_iter().map(|record| record.as_public()).collect())
	}

	/// List local Loop/Decision Contracts for one project.
	#[allow(dead_code)]
	pub(crate) fn list_decision_contracts_for_project(
		&self,
		project_id: &str,
	) -> Result<Vec<DecisionContractRecord>> {
		validate_required_decision_contract_field("project_id", project_id)?;

		if let Some(sqlite) = &self.sqlite {
			let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;
			let records = sqlite
				.list_decision_contracts_for_project(project_id)?
				.into_iter()
				.map(|record| record.as_public())
				.collect();

			return Ok(records);
		}

		let state = self.lock()?;
		let mut records = state
			.decision_contracts
			.values()
			.filter(|record| record.project_id == project_id)
			.cloned()
			.collect::<Vec<_>>();

		records.sort_by(compare_decision_contract_runtime_records);

		Ok(records.into_iter().map(|record| record.as_public()).collect())
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
		validate_required_decision_contract_field("project_id", project_id)?;
		validate_required_decision_contract_field("contract_id", contract_id)?;

		let now = timestamp_parts();
		let key = DecisionContractKey::new(project_id, contract_id);
		let mut state = self.lock()?;
		let mut record = state
			.decision_contracts
			.get(&key)
			.cloned()
			.ok_or_else(|| eyre::eyre!("Decision contract `{contract_id}` does not exist."))?;

		update(&mut record.contract)?;

		record.contract.validate()?;

		record.status = record.contract.status();
		record.updated_at = now.text;
		record.updated_at_unix = now.unix;

		state.decision_contracts.insert(key, record.clone());
		self.upsert_decision_contract_locked(&record)?;

		Ok(record.as_public())
	}
}
