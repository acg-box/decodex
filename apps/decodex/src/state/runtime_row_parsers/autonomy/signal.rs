use rusqlite::{Error, Row};

use crate::{
	autonomy_signal::AutonomySignal,
	prelude::eyre,
	state::{AutonomySignalRuntimeRecord, AutonomySignalRuntimeRowParts},
};

pub(in crate::state) fn autonomy_signal_runtime_row_parts(
	row: &Row<'_>,
) -> std::result::Result<AutonomySignalRuntimeRowParts, Error> {
	Ok(AutonomySignalRuntimeRowParts {
		project_id: row.get(0)?,
		signal_id: row.get(1)?,
		objective_id: row.get(2)?,
		objective_version: row.get(3)?,
		kind: row.get(4)?,
		fingerprint: row.get(5)?,
		freshness: row.get(6)?,
		evidence_class: row.get(7)?,
		confidence: row.get(8)?,
		privacy: row.get(9)?,
		payload_json: row.get(10)?,
		created_at: row.get(11)?,
		created_at_unix: row.get(12)?,
		updated_at: row.get(13)?,
		updated_at_unix: row.get(14)?,
	})
}

pub(in crate::state) fn autonomy_signal_record_from_row_parts(
	parts: AutonomySignalRuntimeRowParts,
) -> crate::prelude::Result<AutonomySignalRuntimeRecord> {
	let signal = serde_json::from_str::<AutonomySignal>(&parts.payload_json)?;
	let version = u64::try_from(parts.objective_version).map_err(|_| {
		eyre::eyre!("Autonomy signal row objective_version must be greater than zero.")
	})?;

	signal.validate()?;

	if parts.project_id != signal.project_id() {
		eyre::bail!(
			"Autonomy signal row project `{}` contained payload project `{}`.",
			parts.project_id,
			signal.project_id()
		);
	}
	if parts.signal_id != signal.id() {
		eyre::bail!(
			"Autonomy signal row `{}` contained payload `{}`.",
			parts.signal_id,
			signal.id()
		);
	}
	if parts.objective_id != signal.objective_id() {
		eyre::bail!(
			"Autonomy signal row objective `{}` contained payload `{}`.",
			parts.objective_id,
			signal.objective_id()
		);
	}
	if version != signal.objective_version() {
		eyre::bail!(
			"Autonomy signal row `{}` objective version {} contained payload version {}.",
			parts.signal_id,
			version,
			signal.objective_version()
		);
	}
	if parts.kind != signal.kind().as_str()
		|| parts.fingerprint != signal.fingerprint()
		|| parts.freshness != signal.freshness().as_str()
		|| parts.evidence_class != signal.evidence_class().as_str()
		|| parts.confidence != signal.confidence().as_str()
		|| parts.privacy != signal.privacy().as_str()
	{
		eyre::bail!(
			"Autonomy signal row `{}` readback columns differed from payload.",
			parts.signal_id
		);
	}

	Ok(AutonomySignalRuntimeRecord {
		project_id: parts.project_id,
		signal,
		created_at: parts.created_at,
		created_at_unix: parts.created_at_unix,
		updated_at: parts.updated_at,
		updated_at_unix: parts.updated_at_unix,
	})
}
