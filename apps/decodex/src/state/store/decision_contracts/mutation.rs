use crate::{
	loop_contract::DecisionContract,
	prelude::{Result, eyre},
	state::{
		runtime_records::{DecisionContractKey, DecisionContractRuntimeRecord},
		store,
		store::{DecisionContractRecord, StateStore, validation},
	},
};

pub(in crate::state::store::decision_contracts) fn upsert_decision_contract(
	store: &StateStore,
	project_id: &str,
	source_issue_id: Option<&str>,
	contract: DecisionContract,
) -> Result<DecisionContractRecord> {
	validation::validate_decision_contract_record_inputs(project_id, source_issue_id, &contract)?;

	let now = store::timestamp_parts();
	let mut state = store.lock_without_refresh()?;
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
	store.upsert_decision_contract_locked(&record)?;

	Ok(record.as_public())
}

pub(in crate::state::store::decision_contracts) fn update_decision_contract(
	store: &StateStore,
	project_id: &str,
	contract_id: &str,
	update: impl FnOnce(&mut DecisionContract) -> Result<()>,
) -> Result<DecisionContractRecord> {
	validation::validate_required_decision_contract_field("project_id", project_id)?;
	validation::validate_required_decision_contract_field("contract_id", contract_id)?;

	let now = store::timestamp_parts();
	let key = DecisionContractKey::new(project_id, contract_id);
	let mut state = store.lock()?;
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
	store.upsert_decision_contract_locked(&record)?;

	Ok(record.as_public())
}
