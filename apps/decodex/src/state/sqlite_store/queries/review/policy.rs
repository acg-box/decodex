use crate::state::sqlite_store::{
	SqliteStateStore,
	queries::{self, ReviewPolicyKey, ReviewPolicyRuntimeRecord, Row, StateData},
};

impl SqliteStateStore {
	pub(in crate::state) fn load_review_policy_checkpoints(
		&self,
		state: &mut StateData,
	) -> queries::Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, issue_id, run_id, attempt_number, phase, status, head_sha, \
			 nonclean_rounds, details_json, updated_at, updated_at_unix FROM review_policy_checkpoints",
		)?;
		let rows = statement.query_map([], review_policy_checkpoint_from_row)?;

		for row in rows {
			let (key, record) = row?;

			state.review_policy_checkpoints.insert(key, record);
		}

		Ok(())
	}

	pub(in crate::state) fn load_review_policy_checkpoints_for_project(
		&self,
		state: &mut StateData,
		project_id: &str,
	) -> queries::Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, issue_id, run_id, attempt_number, phase, status, head_sha, \
			 nonclean_rounds, details_json, updated_at, updated_at_unix FROM review_policy_checkpoints \
			 WHERE project_id = ?1",
		)?;
		let rows =
			statement.query_map(queries::params![project_id], review_policy_checkpoint_from_row)?;

		for row in rows {
			let (key, record) = row?;

			state.review_policy_checkpoints.insert(key, record);
		}

		Ok(())
	}
}

fn review_policy_checkpoint_from_row(
	row: &Row<'_>,
) -> rusqlite::Result<(ReviewPolicyKey, ReviewPolicyRuntimeRecord)> {
	let project_id: String = row.get(0)?;
	let issue_id: String = row.get(1)?;
	let run_id: String = row.get(2)?;
	let attempt_number: i64 = row.get(3)?;
	let phase: String = row.get(4)?;

	Ok((
		ReviewPolicyKey::new(&project_id, &issue_id, &run_id, attempt_number, &phase),
		ReviewPolicyRuntimeRecord {
			project_id,
			issue_id,
			run_id,
			attempt_number,
			phase,
			status: row.get(5)?,
			head_sha: row.get(6)?,
			nonclean_rounds: row.get(7)?,
			details_json: row.get(8)?,
			updated_at: row.get(9)?,
			updated_at_unix: row.get(10)?,
		},
	))
}
