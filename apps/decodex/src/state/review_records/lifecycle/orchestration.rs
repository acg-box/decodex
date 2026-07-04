use crate::{
	prelude::Result,
	state::{
		ReviewHandoffMarker, ReviewOrchestrationMarker, StateStore,
		runtime_records::{ReviewLifecycleKey, ReviewLifecycleRuntimeRecord},
		runtime_row_parsers,
	},
};

impl StateStore {
	/// Create or replace the retained review orchestration projection for one issue lane.
	pub(crate) fn upsert_review_orchestration_marker(
		&self,
		project_id: &str,
		issue_id: &str,
		marker: &ReviewOrchestrationMarker,
	) -> Result<()> {
		let now = runtime_row_parsers::timestamp_parts();
		let key = ReviewLifecycleKey::new(project_id, issue_id, marker.branch_name());
		let mut state = self.lock()?;
		let record = state.review_lifecycle_records.entry(key).or_insert_with(|| {
			ReviewLifecycleRuntimeRecord {
				project_id: project_id.to_owned(),
				issue_id: issue_id.to_owned(),
				branch_name: marker.branch_name().to_owned(),
				run_id: marker.run_id().to_owned(),
				attempt_number: marker.attempt_number(),
				pr_url: marker.pr_url().to_owned(),
				target_base_ref_name: None,
				pr_head_ref_name: marker.branch_name().to_owned(),
				pr_head_oid: marker.head_sha().to_owned(),
				head_sha: marker.head_sha().to_owned(),
				phase: marker.phase().to_owned(),
				request_comment_database_id: None,
				request_created_at_unix_epoch: None,
				request_description_thumbs_up_count: None,
				request_retry_count: 0,
				external_round_count: 0,
				auto_merge_enabled_at_unix_epoch: None,
				landing_state: String::from("not_started"),
				closeout_state: String::from("not_started"),
				repair_attempt_count: 0,
				evidence_json: String::from("{}"),
				next_action: String::new(),
				updated_at: now.text.clone(),
				updated_at_unix: now.unix,
			}
		});

		record.run_id = marker.run_id().to_owned();
		record.attempt_number = marker.attempt_number();
		record.pr_url = marker.pr_url().to_owned();
		record.head_sha = marker.head_sha().to_owned();
		record.phase = marker.phase().to_owned();
		record.request_comment_database_id = marker.request_comment_database_id();
		record.request_created_at_unix_epoch = marker.request_created_at_unix_epoch();
		record.request_description_thumbs_up_count = marker.request_description_thumbs_up_count();
		record.request_retry_count = marker.request_retry_count();
		record.external_round_count = marker.external_round_count();
		record.auto_merge_enabled_at_unix_epoch = marker.auto_merge_enabled_at_unix_epoch();
		record.updated_at = now.text;
		record.updated_at_unix = now.unix;

		self.persist_runtime_state_locked(&state)
	}

	/// Read retained review orchestration for the current handoff identity.
	pub(crate) fn review_orchestration_marker(
		&self,
		project_id: &str,
		issue_id: &str,
		review_handoff: &ReviewHandoffMarker,
	) -> Result<Option<ReviewOrchestrationMarker>> {
		let Some(record) =
			self.review_lifecycle_record(project_id, issue_id, review_handoff.branch_name())?
		else {
			return Ok(None);
		};

		if record.run_id() != review_handoff.run_id()
			|| record.attempt_number() != review_handoff.attempt_number()
			|| record.branch_name() != review_handoff.branch_name()
			|| record.pr_url() != review_handoff.pr_url()
		{
			return Ok(None);
		}

		Ok(Some(ReviewOrchestrationMarker::new(
			record.run_id().to_owned(),
			record.attempt_number(),
			record.branch_name().to_owned(),
			record.pr_url().to_owned(),
			record.head_sha().to_owned(),
			record.phase().to_owned(),
			record.request_comment_database_id(),
			record.request_created_at_unix_epoch(),
			record.request_description_thumbs_up_count(),
			record.request_retry_count(),
			record.external_round_count(),
			record.auto_merge_enabled_at_unix_epoch(),
		)))
	}
}
