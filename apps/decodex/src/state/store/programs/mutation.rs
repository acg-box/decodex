use sha2::{Digest as _, Sha256};

use crate::{
	execution_program::ExecutionProgram,
	prelude::{Result, eyre},
	state::{
		runtime_records::{ExecutionProgramKey, ExecutionProgramRuntimeRecord},
		store,
		store::{
			ExecutionProgramRecord, ProgramIntakeAttemptClaim, ProgramIntakeAttemptStatus,
			StateStore, validation,
		},
	},
};

pub(in crate::state::store::programs) fn begin_program_intake_attempt(
	store: &StateStore,
	project_id: &str,
	contract_id: &str,
	request_digest: &str,
) -> Result<ProgramIntakeAttemptClaim> {
	for (name, value) in [
		("project_id", project_id),
		("contract_id", contract_id),
		("request_digest", request_digest),
	] {
		validation::validate_required_execution_program_field(name, value)?;
	}

	let now = store::timestamp_parts();
	let canonical_key = canonical_program_intake_key(project_id, contract_id);

	if let Some(sqlite) = &store.sqlite {
		let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;

		return match sqlite.begin_program_intake_attempt(
			project_id,
			contract_id,
			&canonical_key,
			request_digest,
			&now.text,
		)? {
			"acquired" => Ok(ProgramIntakeAttemptClaim::Acquired),
			"prepared" => Ok(ProgramIntakeAttemptClaim::Prepared),
			"started" => Ok(ProgramIntakeAttemptClaim::Started),
			"completed" => Ok(ProgramIntakeAttemptClaim::Completed),
			_ => crate::prelude::eyre::bail!("Program Intake attempt state is invalid."),
		};
	}

	let key = (project_id.to_owned(), contract_id.to_owned());
	let mut state = store.lock()?;

	if let Some((status, stored_digest)) = state.program_intake_attempts.get(&key) {
		if stored_digest != request_digest {
			eyre::bail!("program_intake_attempt_request_mismatch");
		}

		return Ok(match status {
			ProgramIntakeAttemptStatus::Prepared => ProgramIntakeAttemptClaim::Prepared,
			ProgramIntakeAttemptStatus::Started => ProgramIntakeAttemptClaim::Started,
			ProgramIntakeAttemptStatus::Completed => ProgramIntakeAttemptClaim::Completed,
		});
	}

	state
		.program_intake_attempts
		.insert(key, (ProgramIntakeAttemptStatus::Prepared, request_digest.to_owned()));

	Ok(ProgramIntakeAttemptClaim::Acquired)
}

pub(in crate::state::store::programs) fn program_intake_attempt_status(
	store: &StateStore,
	project_id: &str,
	contract_id: &str,
) -> Result<Option<ProgramIntakeAttemptStatus>> {
	if let Some(sqlite) = &store.sqlite {
		let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;
		let status = sqlite.program_intake_attempt_status(project_id, contract_id)?;

		return status
			.map(|status| match status.as_str() {
				"prepared" => Ok(ProgramIntakeAttemptStatus::Prepared),
				"started" => Ok(ProgramIntakeAttemptStatus::Started),
				"completed" => Ok(ProgramIntakeAttemptStatus::Completed),
				_ => crate::prelude::eyre::bail!("Program Intake attempt state is invalid."),
			})
			.transpose();
	}

	let state = store.lock()?;

	Ok(state
		.program_intake_attempts
		.get(&(project_id.to_owned(), contract_id.to_owned()))
		.map(|(status, _)| *status))
}

pub(in crate::state::store::programs) fn mark_program_intake_attempt_started(
	store: &StateStore,
	project_id: &str,
	contract_id: &str,
) -> Result<()> {
	let now = store::timestamp_parts();

	if let Some(sqlite) = &store.sqlite {
		let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;

		return sqlite.mark_program_intake_attempt_started(project_id, contract_id, &now.text);
	}

	let mut state = store.lock()?;
	let (status, _) = state
		.program_intake_attempts
		.get_mut(&(project_id.to_owned(), contract_id.to_owned()))
		.ok_or_else(|| eyre::eyre!("Program Intake attempt claim does not exist."))?;

	if *status != ProgramIntakeAttemptStatus::Prepared {
		eyre::bail!("Program Intake attempt is not retry-safe prepared state.");
	}

	*status = ProgramIntakeAttemptStatus::Started;

	Ok(())
}

pub(in crate::state::store::programs) fn complete_program_intake_attempt(
	store: &StateStore,
	project_id: &str,
	contract_id: &str,
) -> Result<()> {
	let now = store::timestamp_parts();

	if let Some(sqlite) = &store.sqlite {
		let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;

		return sqlite.complete_program_intake_attempt(project_id, contract_id, &now.text);
	}

	let key = (project_id.to_owned(), contract_id.to_owned());
	let mut state = store.lock()?;
	let (status, _) = state
		.program_intake_attempts
		.get_mut(&key)
		.ok_or_else(|| eyre::eyre!("Program Intake attempt claim does not exist."))?;

	if *status != ProgramIntakeAttemptStatus::Started
		&& *status != ProgramIntakeAttemptStatus::Completed
	{
		eyre::bail!("Program Intake attempt has not started external mutation.");
	}

	*status = ProgramIntakeAttemptStatus::Completed;

	Ok(())
}

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

fn canonical_program_intake_key(project_id: &str, contract_id: &str) -> String {
	let digest = Sha256::digest(format!("{project_id}\0{contract_id}").as_bytes());

	format!(
		"program-intake-{}",
		digest.iter().map(|byte| format!("{byte:02x}")).collect::<String>()
	)
}
