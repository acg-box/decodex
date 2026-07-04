use crate::{
	prelude::Result,
	state::{
		ReviewHandoffMarker, StateStore,
		runtime_records::{ReviewLifecycleKey, ReviewLifecycleRuntimeRecord},
		runtime_row_parsers,
	},
};

impl StateStore {
	/// Create or replace the retained review handoff projection for one issue lane.
	pub(crate) fn upsert_review_handoff_marker(
		&self,
		project_id: &str,
		issue_id: &str,
		marker: &ReviewHandoffMarker,
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
				target_base_ref_name: marker.target_base_ref_name().map(str::to_owned),
				pr_head_ref_name: marker.pr_head_ref_name().to_owned(),
				pr_head_oid: marker.pr_head_oid().to_owned(),
				head_sha: marker.pr_head_oid().to_owned(),
				phase: String::from("request_pending"),
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
		let same_handoff_projection = record.run_id == marker.run_id()
			&& record.attempt_number == marker.attempt_number()
			&& record.pr_url == marker.pr_url()
			&& record.target_base_ref_name.as_deref() == marker.target_base_ref_name()
			&& record.pr_head_ref_name == marker.pr_head_ref_name()
			&& record.pr_head_oid == marker.pr_head_oid();

		record.run_id = marker.run_id().to_owned();
		record.attempt_number = marker.attempt_number();
		record.pr_url = marker.pr_url().to_owned();
		record.target_base_ref_name = marker.target_base_ref_name().map(str::to_owned);
		record.pr_head_ref_name = marker.pr_head_ref_name().to_owned();
		record.pr_head_oid = marker.pr_head_oid().to_owned();

		if !same_handoff_projection {
			record.head_sha = marker.pr_head_oid().to_owned();
			record.phase = String::from("request_pending");
			record.request_comment_database_id = None;
			record.request_created_at_unix_epoch = None;
			record.request_description_thumbs_up_count = None;
			record.request_retry_count = 0;
			record.external_round_count = 0;
			record.auto_merge_enabled_at_unix_epoch = None;
			record.landing_state = String::from("not_started");
			record.closeout_state = String::from("not_started");
			record.repair_attempt_count = 0;
			record.evidence_json = String::from("{}");

			record.next_action.clear();
		}

		record.updated_at = now.text;
		record.updated_at_unix = now.unix;

		self.persist_runtime_state_locked(&state)
	}

	/// Read the retained review handoff projection for one issue branch.
	pub(crate) fn review_handoff_marker(
		&self,
		project_id: &str,
		issue_id: &str,
		branch_name: &str,
	) -> Result<Option<ReviewHandoffMarker>> {
		Ok(self.review_lifecycle_record(project_id, issue_id, branch_name)?.map(|record| {
			ReviewHandoffMarker {
				run_id: record.run_id().to_owned(),
				attempt_number: record.attempt_number(),
				branch_name: record.branch_name().to_owned(),
				pr_url: record.pr_url().to_owned(),
				target_base_ref_name: record.target_base_ref_name().map(str::to_owned),
				pr_head_ref_name: record.pr_head_ref_name().to_owned(),
				pr_head_oid: record.pr_head_oid().to_owned(),
			}
		}))
	}
}
