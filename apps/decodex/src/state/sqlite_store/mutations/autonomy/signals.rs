use crate::state::sqlite_store::mutations::{
	self, AutonomyProposalRuntimeRecord, AutonomySignalRuntimeRecord, Result, SqliteStateStore,
	eyre,
};

impl SqliteStateStore {
	pub(in crate::state) fn upsert_autonomy_signal(
		&self,
		record: &AutonomySignalRuntimeRecord,
	) -> Result<()> {
		let payload_json = serde_json::to_string(&record.signal)?;
		let version = i64::try_from(record.signal.objective_version()).map_err(|_| {
			eyre::eyre!("Autonomy signal objective_version exceeds SQLite integer range.")
		})?;

		self.connection.execute(
			"INSERT INTO autonomy_signals (
					project_id, signal_id, objective_id, objective_version, kind, fingerprint,
					freshness, evidence_class, confidence, privacy, payload_json, created_at,
					created_at_unix, updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
			 ON CONFLICT(project_id, signal_id) DO UPDATE SET
				 objective_id = excluded.objective_id,
				 objective_version = excluded.objective_version,
				 kind = excluded.kind,
				 fingerprint = excluded.fingerprint,
				 freshness = excluded.freshness,
				 evidence_class = excluded.evidence_class,
				 confidence = excluded.confidence,
				 privacy = excluded.privacy,
				 payload_json = excluded.payload_json,
				 updated_at = excluded.updated_at,
				 updated_at_unix = excluded.updated_at_unix",
			mutations::params![
				&record.project_id,
				record.signal.id(),
				record.signal.objective_id(),
				version,
				record.signal.kind().as_str(),
				record.signal.fingerprint(),
				record.signal.freshness().as_str(),
				record.signal.evidence_class().as_str(),
				record.signal.confidence().as_str(),
				record.signal.privacy().as_str(),
				payload_json,
				&record.created_at,
				record.created_at_unix,
				&record.updated_at,
				record.updated_at_unix,
			],
		)?;

		Ok(())
	}

	pub(in crate::state) fn upsert_autonomy_proposal(
		&self,
		record: &AutonomyProposalRuntimeRecord,
	) -> Result<()> {
		let payload_json = serde_json::to_string(&record.proposal)?;
		let version = i64::try_from(record.proposal.objective_version()).map_err(|_| {
			eyre::eyre!("Autonomy proposal objective_version exceeds SQLite integer range.")
		})?;

		self.connection.execute(
			"INSERT INTO autonomy_proposals (
					project_id, proposal_id, objective_id, objective_version, state, fingerprint,
					source_family, intended_surface, payload_json, created_at, created_at_unix,
					updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
			 ON CONFLICT(project_id, proposal_id) DO UPDATE SET
				 objective_id = excluded.objective_id,
				 objective_version = excluded.objective_version,
				 state = excluded.state,
				 fingerprint = excluded.fingerprint,
				 source_family = excluded.source_family,
				 intended_surface = excluded.intended_surface,
				 payload_json = excluded.payload_json,
				 updated_at = excluded.updated_at,
				 updated_at_unix = excluded.updated_at_unix",
			mutations::params![
				&record.project_id,
				record.proposal.id(),
				record.proposal.objective_id(),
				version,
				record.state.as_str(),
				record.proposal.fingerprint(),
				record.proposal.source_family(),
				record.proposal.intended_surface(),
				payload_json,
				&record.created_at,
				record.created_at_unix,
				&record.updated_at,
				record.updated_at_unix,
			],
		)?;

		Ok(())
	}
}
