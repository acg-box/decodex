use crate::state::sqlite_store::{
	SqliteStateStore,
	queries::{self, EvidenceArtifactKey, EvidenceArtifactRuntimeRecord, Row, StateData},
};

impl SqliteStateStore {
	pub(in crate::state) fn load_evidence_artifacts(
		&self,
		state: &mut StateData,
	) -> queries::Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, issue_id, artifact_kind, key_hash, phase, status, head_sha, \
			 key_json, payload_json, source_run_id, source_attempt_number, updated_at, \
			 updated_at_unix FROM evidence_artifacts",
		)?;
		let rows = statement.query_map([], evidence_artifact_from_row)?;

		for row in rows {
			let (key, record) = row?;

			state.evidence_artifacts.insert(key, record);
		}

		Ok(())
	}

	pub(in crate::state) fn load_evidence_artifacts_for_project(
		&self,
		state: &mut StateData,
		project_id: &str,
	) -> queries::Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, issue_id, artifact_kind, key_hash, phase, status, head_sha, \
			 key_json, payload_json, source_run_id, source_attempt_number, updated_at, \
			 updated_at_unix FROM evidence_artifacts WHERE project_id = ?1",
		)?;
		let rows = statement.query_map(queries::params![project_id], evidence_artifact_from_row)?;

		for row in rows {
			let (key, record) = row?;

			state.evidence_artifacts.insert(key, record);
		}

		Ok(())
	}
}

fn evidence_artifact_from_row(
	row: &Row<'_>,
) -> rusqlite::Result<(EvidenceArtifactKey, EvidenceArtifactRuntimeRecord)> {
	let project_id: String = row.get(0)?;
	let issue_id: String = row.get(1)?;
	let artifact_kind: String = row.get(2)?;
	let key_hash: String = row.get(3)?;

	Ok((
		EvidenceArtifactKey::new(&project_id, &issue_id, &artifact_kind, &key_hash),
		EvidenceArtifactRuntimeRecord {
			project_id,
			issue_id,
			artifact_kind,
			key_hash,
			phase: row.get(4)?,
			status: row.get(5)?,
			head_sha: row.get(6)?,
			key_json: row.get(7)?,
			payload_json: row.get(8)?,
			source_run_id: row.get(9)?,
			source_attempt_number: row.get(10)?,
			updated_at: row.get(11)?,
			updated_at_unix: row.get(12)?,
		},
	))
}
