use crate::{
	execution_program::ExecutionProgram,
	prelude::{Result, eyre},
};

use super::{
	super::runtime_records::{ExecutionProgramKey, ExecutionProgramRuntimeRecord},
	ExecutionProgramRecord, ProgramIntakePlanRecord, ProgramIssueMappingRecord, StateStore,
	apply_derived_program_intake_state, compare_execution_program_runtime_records,
	compare_program_intake_plan_records, compare_program_issue_mapping_records, timestamp_parts,
	validate_execution_program_record_inputs, validate_required_execution_program_field,
};

impl StateStore {
	/// Create or replace one local internal Execution Program payload.
	#[allow(dead_code)]
	pub(crate) fn upsert_execution_program(
		&self,
		project_id: &str,
		program: ExecutionProgram,
	) -> Result<ExecutionProgramRecord> {
		validate_execution_program_record_inputs(project_id, &program)?;

		let now = timestamp_parts();
		let mut state = self.lock_without_refresh()?;
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

		apply_derived_program_intake_state(&mut state, &record);

		self.upsert_execution_program_locked(&record)?;

		Ok(record.as_public())
	}

	/// Read one local internal Execution Program by project and program id.
	#[allow(dead_code)]
	pub(crate) fn execution_program(
		&self,
		project_id: &str,
		program_id: &str,
	) -> Result<Option<ExecutionProgramRecord>> {
		validate_required_execution_program_field("project_id", project_id)?;
		validate_required_execution_program_field("program_id", program_id)?;

		if let Some(sqlite) = &self.sqlite {
			let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;

			return sqlite
				.execution_program(project_id, program_id)
				.map(|record| record.map(|record| record.as_public()));
		}

		let state = self.lock()?;

		Ok(state
			.execution_programs
			.get(&ExecutionProgramKey::new(project_id, program_id))
			.map(ExecutionProgramRuntimeRecord::as_public))
	}

	/// List local internal Execution Programs derived from one Decision Contract.
	#[allow(dead_code)]
	pub(crate) fn list_execution_programs_for_contract(
		&self,
		project_id: &str,
		source_contract_id: &str,
	) -> Result<Vec<ExecutionProgramRecord>> {
		validate_required_execution_program_field("project_id", project_id)?;
		validate_required_execution_program_field("source_contract_id", source_contract_id)?;

		if let Some(sqlite) = &self.sqlite {
			let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;
			let records = sqlite
				.list_execution_programs_for_contract(project_id, source_contract_id)?
				.into_iter()
				.map(|record| record.as_public())
				.collect();

			return Ok(records);
		}

		let state = self.lock()?;
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

	/// List local internal Execution Programs retained for one project.
	#[allow(dead_code)]
	pub(crate) fn list_execution_programs(
		&self,
		project_id: &str,
	) -> Result<Vec<ExecutionProgramRecord>> {
		validate_required_execution_program_field("project_id", project_id)?;

		if let Some(sqlite) = &self.sqlite {
			let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;
			let records = sqlite
				.list_execution_programs(project_id)?
				.into_iter()
				.map(|record| record.as_public())
				.collect();

			return Ok(records);
		}

		let state = self.lock()?;
		let mut records = state
			.execution_programs
			.values()
			.filter(|record| record.project_id == project_id)
			.cloned()
			.collect::<Vec<_>>();

		records.sort_by(compare_execution_program_runtime_records);

		Ok(records.into_iter().map(|record| record.as_public()).collect())
	}

	/// List local Program Intake Plan records retained for one project.
	#[allow(dead_code)]
	pub(crate) fn list_program_intake_plans(
		&self,
		project_id: &str,
	) -> Result<Vec<ProgramIntakePlanRecord>> {
		validate_required_execution_program_field("project_id", project_id)?;

		if let Some(sqlite) = &self.sqlite {
			let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;

			return sqlite.list_program_intake_plans(project_id);
		}

		let state = self.lock()?;
		let mut records = state
			.program_intake_plans
			.values()
			.filter(|record| record.project_id == project_id)
			.cloned()
			.collect::<Vec<_>>();

		records.sort_by(compare_program_intake_plan_records);

		Ok(records)
	}

	/// List local issue mappings for one internal Execution Program.
	#[allow(dead_code)]
	pub(crate) fn list_program_issue_mappings(
		&self,
		project_id: &str,
		program_id: &str,
	) -> Result<Vec<ProgramIssueMappingRecord>> {
		validate_required_execution_program_field("project_id", project_id)?;
		validate_required_execution_program_field("program_id", program_id)?;

		if let Some(sqlite) = &self.sqlite {
			let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;

			return sqlite.list_program_issue_mappings(project_id, program_id);
		}

		let state = self.lock()?;
		let mut records = state
			.program_issue_mappings
			.values()
			.filter(|record| record.project_id == project_id && record.program_id == program_id)
			.cloned()
			.collect::<Vec<_>>();

		records.sort_by(compare_program_issue_mapping_records);

		Ok(records)
	}
}
