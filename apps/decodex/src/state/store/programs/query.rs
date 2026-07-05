use crate::{
	prelude::{Result, eyre},
	state::{
		runtime_records::{ExecutionProgramKey, ExecutionProgramRuntimeRecord},
		store::{
			ExecutionProgramRecord, ProgramIntakePlanRecord, ProgramIssueMappingRecord, StateStore,
			compare_execution_program_runtime_records, compare_program_intake_plan_records,
			compare_program_issue_mapping_records, validation,
		},
	},
};

pub(in crate::state::store::programs) fn execution_program(
	store: &StateStore,
	project_id: &str,
	program_id: &str,
) -> Result<Option<ExecutionProgramRecord>> {
	validation::validate_required_execution_program_field("project_id", project_id)?;
	validation::validate_required_execution_program_field("program_id", program_id)?;

	if let Some(sqlite) = &store.sqlite {
		let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;

		return sqlite
			.execution_program(project_id, program_id)
			.map(|record| record.map(|record| record.as_public()));
	}

	let state = store.lock()?;

	Ok(state
		.execution_programs
		.get(&ExecutionProgramKey::new(project_id, program_id))
		.map(ExecutionProgramRuntimeRecord::as_public))
}

pub(in crate::state::store::programs) fn list_execution_programs_for_contract(
	store: &StateStore,
	project_id: &str,
	source_contract_id: &str,
) -> Result<Vec<ExecutionProgramRecord>> {
	validation::validate_required_execution_program_field("project_id", project_id)?;
	validation::validate_required_execution_program_field(
		"source_contract_id",
		source_contract_id,
	)?;

	if let Some(sqlite) = &store.sqlite {
		let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;
		let records = sqlite
			.list_execution_programs_for_contract(project_id, source_contract_id)?
			.into_iter()
			.map(|record| record.as_public())
			.collect();

		return Ok(records);
	}

	let state = store.lock()?;
	let mut records = state
		.execution_programs
		.values()
		.filter(|record| {
			record.project_id == project_id
				&& record.source_contract_id.as_deref() == Some(source_contract_id)
		})
		.cloned()
		.collect::<Vec<_>>();

	records.sort_by(compare_execution_program_runtime_records);

	Ok(records.into_iter().map(|record| record.as_public()).collect())
}

pub(in crate::state::store::programs) fn list_execution_programs(
	store: &StateStore,
	project_id: &str,
) -> Result<Vec<ExecutionProgramRecord>> {
	validation::validate_required_execution_program_field("project_id", project_id)?;

	if let Some(sqlite) = &store.sqlite {
		let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;
		let records = sqlite
			.list_execution_programs(project_id)?
			.into_iter()
			.map(|record| record.as_public())
			.collect();

		return Ok(records);
	}

	let state = store.lock()?;
	let mut records = state
		.execution_programs
		.values()
		.filter(|record| record.project_id == project_id)
		.cloned()
		.collect::<Vec<_>>();

	records.sort_by(compare_execution_program_runtime_records);

	Ok(records.into_iter().map(|record| record.as_public()).collect())
}

pub(in crate::state::store::programs) fn list_program_intake_plans(
	store: &StateStore,
	project_id: &str,
) -> Result<Vec<ProgramIntakePlanRecord>> {
	validation::validate_required_execution_program_field("project_id", project_id)?;

	if let Some(sqlite) = &store.sqlite {
		let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;

		return sqlite.list_program_intake_plans(project_id);
	}

	let state = store.lock()?;
	let mut records = state
		.program_intake_plans
		.values()
		.filter(|record| record.project_id == project_id)
		.cloned()
		.collect::<Vec<_>>();

	records.sort_by(compare_program_intake_plan_records);

	Ok(records)
}

pub(in crate::state::store::programs) fn list_program_issue_mappings(
	store: &StateStore,
	project_id: &str,
	program_id: &str,
) -> Result<Vec<ProgramIssueMappingRecord>> {
	validation::validate_required_execution_program_field("project_id", project_id)?;
	validation::validate_required_execution_program_field("program_id", program_id)?;

	if let Some(sqlite) = &store.sqlite {
		let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;

		return sqlite.list_program_issue_mappings(project_id, program_id);
	}

	let state = store.lock()?;
	let mut records = state
		.program_issue_mappings
		.values()
		.filter(|record| record.project_id == project_id && record.program_id == program_id)
		.cloned()
		.collect::<Vec<_>>();

	records.sort_by(compare_program_issue_mapping_records);

	Ok(records)
}
