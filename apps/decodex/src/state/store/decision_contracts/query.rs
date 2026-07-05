use crate::{
	prelude::{Result, eyre},
	state::{
		runtime_records::{DecisionContractKey, DecisionContractRuntimeRecord},
		store::{
			DecisionContractRecord, StateStore, compare_decision_contract_runtime_records,
			validation,
		},
	},
};

pub(in crate::state::store::decision_contracts) fn decision_contract(
	store: &StateStore,
	project_id: &str,
	contract_id: &str,
) -> Result<Option<DecisionContractRecord>> {
	validation::validate_required_decision_contract_field("project_id", project_id)?;
	validation::validate_required_decision_contract_field("contract_id", contract_id)?;

	if let Some(sqlite) = &store.sqlite {
		let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;

		return sqlite
			.decision_contract(project_id, contract_id)
			.map(|record| record.map(|record| record.as_public()));
	}

	let state = store.lock()?;

	Ok(state
		.decision_contracts
		.get(&DecisionContractKey::new(project_id, contract_id))
		.map(DecisionContractRuntimeRecord::as_public))
}

pub(in crate::state::store::decision_contracts) fn list_decision_contracts_for_issue(
	store: &StateStore,
	project_id: &str,
	source_issue_id: &str,
) -> Result<Vec<DecisionContractRecord>> {
	validation::validate_required_decision_contract_field("project_id", project_id)?;
	validation::validate_required_decision_contract_field("source_issue_id", source_issue_id)?;

	if let Some(sqlite) = &store.sqlite {
		let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;
		let records = sqlite
			.list_decision_contracts_for_issue(project_id, source_issue_id)?
			.into_iter()
			.map(|record| record.as_public())
			.collect();

		return Ok(records);
	}

	let state = store.lock()?;
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

pub(in crate::state::store::decision_contracts) fn list_decision_contracts_for_project(
	store: &StateStore,
	project_id: &str,
) -> Result<Vec<DecisionContractRecord>> {
	validation::validate_required_decision_contract_field("project_id", project_id)?;

	if let Some(sqlite) = &store.sqlite {
		let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;
		let records = sqlite
			.list_decision_contracts_for_project(project_id)?
			.into_iter()
			.map(|record| record.as_public())
			.collect();

		return Ok(records);
	}

	let state = store.lock()?;
	let mut records = state
		.decision_contracts
		.values()
		.filter(|record| record.project_id == project_id)
		.cloned()
		.collect::<Vec<_>>();

	records.sort_by(compare_decision_contract_runtime_records);

	Ok(records.into_iter().map(|record| record.as_public()).collect())
}
