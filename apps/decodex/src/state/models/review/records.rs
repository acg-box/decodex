#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ReviewOrchestrationMarker {
	pub(in crate::state) run_id: String,
	pub(in crate::state) attempt_number: i64,
	pub(in crate::state) branch_name: String,
	pub(in crate::state) pr_url: String,
	pub(in crate::state) head_sha: String,
	pub(in crate::state) phase: String,
	pub(in crate::state) request_comment_database_id: Option<i64>,
	pub(in crate::state) request_created_at_unix_epoch: Option<i64>,
	pub(in crate::state) request_description_thumbs_up_count: Option<usize>,
	pub(in crate::state) request_retry_count: i64,
	pub(in crate::state) external_round_count: i64,
	pub(in crate::state) auto_merge_enabled_at_unix_epoch: Option<i64>,
}

/// Runtime-owned review lifecycle record for one retained PR-backed lane.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ReviewLifecycleRecord {
	pub(in crate::state) project_id: String,
	pub(in crate::state) issue_id: String,
	pub(in crate::state) branch_name: String,
	pub(in crate::state) run_id: String,
	pub(in crate::state) attempt_number: i64,
	pub(in crate::state) pr_url: String,
	pub(in crate::state) target_base_ref_name: Option<String>,
	pub(in crate::state) pr_head_ref_name: String,
	pub(in crate::state) pr_head_oid: String,
	pub(in crate::state) head_sha: String,
	pub(in crate::state) phase: String,
	pub(in crate::state) request_comment_database_id: Option<i64>,
	pub(in crate::state) request_created_at_unix_epoch: Option<i64>,
	pub(in crate::state) request_description_thumbs_up_count: Option<usize>,
	pub(in crate::state) request_retry_count: i64,
	pub(in crate::state) external_round_count: i64,
	pub(in crate::state) auto_merge_enabled_at_unix_epoch: Option<i64>,
	pub(in crate::state) landing_state: String,
	pub(in crate::state) closeout_state: String,
	pub(in crate::state) repair_attempt_count: i64,
	pub(in crate::state) evidence_json: String,
	pub(in crate::state) next_action: String,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}
