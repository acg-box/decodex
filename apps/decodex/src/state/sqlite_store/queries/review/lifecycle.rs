use crate::state::sqlite_store::{
	SqliteStateStore,
	queries::{self, ReviewLifecycleKey, ReviewLifecycleRuntimeRecord, Row, StateData},
};

impl SqliteStateStore {
	pub(in crate::state) fn load_review_lifecycle_records(
		&self,
		state: &mut StateData,
	) -> queries::Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, issue_id, branch_name, run_id, attempt_number, pr_url, \
			 target_base_ref_name, pr_head_ref_name, pr_head_oid, head_sha, phase, \
			 request_comment_database_id, request_created_at_unix_epoch, \
			 request_description_thumbs_up_count, request_retry_count, external_round_count, \
			 auto_merge_enabled_at_unix_epoch, landing_state, closeout_state, \
			 repair_attempt_count, evidence_json, next_action, updated_at, updated_at_unix \
			 FROM review_lifecycle_records",
		)?;
		let rows = statement.query_map([], review_lifecycle_record_from_row)?;

		for row in rows {
			let (key, record) = row?;

			state.review_lifecycle_records.insert(key, record);
		}

		Ok(())
	}

	pub(in crate::state) fn load_review_lifecycle_records_for_project(
		&self,
		state: &mut StateData,
		project_id: &str,
	) -> queries::Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, issue_id, branch_name, run_id, attempt_number, pr_url, \
			 target_base_ref_name, pr_head_ref_name, pr_head_oid, head_sha, phase, \
			 request_comment_database_id, request_created_at_unix_epoch, \
			 request_description_thumbs_up_count, request_retry_count, external_round_count, \
			 auto_merge_enabled_at_unix_epoch, landing_state, closeout_state, \
			 repair_attempt_count, evidence_json, next_action, updated_at, updated_at_unix \
			 FROM review_lifecycle_records WHERE project_id = ?1",
		)?;
		let rows =
			statement.query_map(queries::params![project_id], review_lifecycle_record_from_row)?;

		for row in rows {
			let (key, record) = row?;

			state.review_lifecycle_records.insert(key, record);
		}

		Ok(())
	}
}

fn review_lifecycle_record_from_row(
	row: &Row<'_>,
) -> rusqlite::Result<(ReviewLifecycleKey, ReviewLifecycleRuntimeRecord)> {
	let project_id: String = row.get(0)?;
	let issue_id: String = row.get(1)?;
	let branch_name: String = row.get(2)?;
	let run_id: String = row.get(3)?;
	let attempt_number: i64 = row.get(4)?;
	let request_description_thumbs_up_count =
		row.get::<_, Option<i64>>(13)?.and_then(|count| usize::try_from(count).ok());

	Ok((
		ReviewLifecycleKey::new(&project_id, &issue_id, &branch_name),
		ReviewLifecycleRuntimeRecord {
			project_id,
			issue_id,
			branch_name,
			run_id,
			attempt_number,
			pr_url: row.get(5)?,
			target_base_ref_name: row.get(6)?,
			pr_head_ref_name: row.get(7)?,
			pr_head_oid: row.get(8)?,
			head_sha: row.get(9)?,
			phase: row.get(10)?,
			request_comment_database_id: row.get(11)?,
			request_created_at_unix_epoch: row.get(12)?,
			request_description_thumbs_up_count,
			request_retry_count: row.get(14)?,
			external_round_count: row.get(15)?,
			auto_merge_enabled_at_unix_epoch: row.get(16)?,
			landing_state: row.get(17)?,
			closeout_state: row.get(18)?,
			repair_attempt_count: row.get(19)?,
			evidence_json: row.get(20)?,
			next_action: row.get(21)?,
			updated_at: row.get(22)?,
			updated_at_unix: row.get(23)?,
		},
	))
}
