use crate::{
	execution_program::ExecutionProgram,
	prelude::Result,
	state::{
		runtime_records::{ExecutionProgramKey, ExecutionProgramRuntimeRecord},
		store,
		store::{ExecutionProgramRecord, StateStore, validation},
	},
};

pub(in crate::state::store::programs) fn upsert_execution_program(
	store: &StateStore,
	project_id: &str,
	program: ExecutionProgram,
) -> Result<ExecutionProgramRecord> {
	validation::validate_execution_program_record_inputs(project_id, &program)?;

	let now = store::timestamp_parts();
	let mut state = store.lock_without_refresh()?;
	let key = ExecutionProgramKey::new(project_id, program.program_id());
	let (created_at, created_at_unix) = state.execution_programs.get(&key).map_or_else(
		|| (now.text.clone(), now.unix),
		|record| (record.created_at.clone(), record.created_at_unix),
	);
	let record = ExecutionProgramRuntimeRecord {
		project_id: project_id.to_owned(),
		source_contract_id: program.source_contract_id().map(str::to_owned),
		program,
		created_at,
		created_at_unix,
		updated_at: now.text,
		updated_at_unix: now.unix,
	};

	state.execution_programs.insert(record.key(), record.clone());

	store::apply_derived_program_intake_state(&mut state, &record);

	store.upsert_execution_program_locked(&record)?;

	Ok(record.as_public())
}
