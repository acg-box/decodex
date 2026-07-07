use rusqlite::{Error, Row};
use serde_json::Value;

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
	let payload = serde_json::from_str::<Value>(&parts.payload_json)?;
	let payload_kind = payload.get("kind").and_then(Value::as_str);
	let legacy_docs_skill_drift = parts.kind == "docs_skill_drift";

	if legacy_docs_skill_drift && payload_kind != Some("docs_skill_drift") {
		eyre::bail!("Legacy autonomy signal row `{}` kind did not match payload.", parts.signal_id);
	}

	let mut signal = serde_json::from_value::<AutonomySignal>(payload)?;

	if legacy_docs_skill_drift {
		let (legacy_id, legacy_fingerprint) = signal.legacy_docs_skill_drift_identity()?;

		if signal.id() != legacy_id || parts.signal_id != legacy_id {
			eyre::bail!(
				"Legacy autonomy signal row `{}` identity did not match payload.",
				parts.signal_id
			);
		}
		if signal.fingerprint() != legacy_fingerprint || parts.fingerprint != legacy_fingerprint {
			eyre::bail!(
				"Legacy autonomy signal row `{}` fingerprint did not match payload.",
				parts.signal_id
			);
		}

		signal.recompute_canonical_identity()?;
	}

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
	if parts.signal_id != signal.id() && !legacy_docs_skill_drift {
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
	if !signal.kind().matches_stored_kind(&parts.kind)
		|| (parts.fingerprint != signal.fingerprint() && !legacy_docs_skill_drift)
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
