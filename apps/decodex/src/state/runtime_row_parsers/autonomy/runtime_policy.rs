use rusqlite::{Error, Row};

use crate::{
	prelude::eyre,
	state::runtime_records::{
		AutonomyRuntimePolicyRuntimeRecord, AutonomyRuntimePolicyRuntimeRowParts,
	},
};

pub(in crate::state) fn autonomy_runtime_policy_runtime_row_parts(
	row: &Row<'_>,
) -> std::result::Result<AutonomyRuntimePolicyRuntimeRowParts, Error> {
	Ok(AutonomyRuntimePolicyRuntimeRowParts {
		project_id: row.get(0)?,
		policy_id: row.get(1)?,
		policy_version: row.get(2)?,
		objective_id: row.get(3)?,
		objective_version: row.get(4)?,
		objective_digest: row.get(5)?,
		authority_ref: row.get(6)?,
		accepted_by: row.get(7)?,
		accepted_at: row.get(8)?,
		acceptance_source: row.get(9)?,
		public_non_goals_json: row.get(10)?,
	})
}

pub(in crate::state) fn autonomy_runtime_policy_record_from_row_parts(
	parts: AutonomyRuntimePolicyRuntimeRowParts,
) -> crate::prelude::Result<AutonomyRuntimePolicyRuntimeRecord> {
	let objective_version = u64::try_from(parts.objective_version).map_err(|_| {
		eyre::eyre!("Autonomy runtime policy objective_version exceeds the supported range.")
	})?;
	let public_non_goals = serde_json::from_str::<Vec<String>>(&parts.public_non_goals_json)?;
	let record = AutonomyRuntimePolicyRuntimeRecord {
		project_id: parts.project_id,
		policy_id: parts.policy_id,
		policy_version: parts.policy_version,
		objective_id: parts.objective_id,
		objective_version,
		objective_digest: parts.objective_digest,
		authority_ref: parts.authority_ref,
		accepted_by: parts.accepted_by,
		accepted_at: parts.accepted_at,
		acceptance_source: parts.acceptance_source,
		public_non_goals,
	};

	record.as_public().validate()?;

	Ok(record)
}
