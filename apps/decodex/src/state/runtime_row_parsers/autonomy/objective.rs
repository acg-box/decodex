use rusqlite::{Error, Row};

use crate::{
	autonomy_objective::AutonomyObjectiveContract,
	prelude::eyre,
	state::{AutonomyObjectiveRuntimeRecord, AutonomyObjectiveRuntimeRowParts},
};

pub(in crate::state) fn autonomy_objective_runtime_row_parts(
	row: &Row<'_>,
) -> std::result::Result<AutonomyObjectiveRuntimeRowParts, Error> {
	Ok(AutonomyObjectiveRuntimeRowParts {
		project_id: row.get(0)?,
		objective_id: row.get(1)?,
		version: row.get(2)?,
		state: row.get(3)?,
		payload_json: row.get(4)?,
		created_at: row.get(5)?,
		created_at_unix: row.get(6)?,
		updated_at: row.get(7)?,
		updated_at_unix: row.get(8)?,
	})
}

pub(in crate::state) fn autonomy_objective_record_from_row_parts(
	parts: AutonomyObjectiveRuntimeRowParts,
) -> crate::prelude::Result<AutonomyObjectiveRuntimeRecord> {
	let objective = serde_json::from_str::<AutonomyObjectiveContract>(&parts.payload_json)?;
	let objective_state = objective.state();
	let version = u64::try_from(parts.version)
		.map_err(|_| eyre::eyre!("Autonomy objective row version must be greater than zero."))?;

	objective.validate()?;

	if parts.project_id != objective.project_id() {
		eyre::bail!(
			"Autonomy objective row project `{}` contained payload project `{}`.",
			parts.project_id,
			objective.project_id()
		);
	}
	if parts.objective_id != objective.id() {
		eyre::bail!(
			"Autonomy objective row `{}` contained payload `{}`.",
			parts.objective_id,
			objective.id()
		);
	}
	if version != objective.version() {
		eyre::bail!(
			"Autonomy objective row `{}` version {} contained payload version {}.",
			parts.objective_id,
			version,
			objective.version()
		);
	}
	if parts.state != objective_state.as_str() {
		eyre::bail!(
			"Autonomy objective row `{}` version {} state `{}` differed from payload state `{}`.",
			parts.objective_id,
			version,
			parts.state,
			objective_state.as_str()
		);
	}

	Ok(AutonomyObjectiveRuntimeRecord {
		project_id: parts.project_id,
		state: objective_state,
		objective,
		created_at: parts.created_at,
		created_at_unix: parts.created_at_unix,
		updated_at: parts.updated_at,
		updated_at_unix: parts.updated_at_unix,
	})
}
