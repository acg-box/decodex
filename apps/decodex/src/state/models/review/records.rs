#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg(test)]
pub(crate) struct ReviewLifecycleTransitionFixture {
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

/// Runtime lifecycle transition projection used by active post-review writers.
pub(crate) struct ReviewLifecycleTransitionInput<'a> {
	pub(crate) run_id: &'a str,
	pub(crate) attempt_number: i64,
	pub(crate) branch_name: &'a str,
	pub(crate) pr_url: &'a str,
	pub(crate) head_sha: &'a str,
	pub(crate) phase: &'a str,
	pub(crate) request_comment_database_id: Option<i64>,
	pub(crate) request_created_at_unix_epoch: Option<i64>,
	pub(crate) request_description_thumbs_up_count: Option<usize>,
	pub(crate) request_retry_count: i64,
	pub(crate) external_round_count: i64,
	pub(crate) auto_merge_enabled_at_unix_epoch: Option<i64>,
}

/// Runtime lifecycle handoff evidence used to create or refresh the authority row.
#[derive(Clone, Copy)]
pub(crate) struct ReviewLifecycleHandoffInput<'a> {
	pub(crate) run_id: &'a str,
	pub(crate) attempt_number: i64,
	pub(crate) branch_name: &'a str,
	pub(crate) pr_url: &'a str,
	pub(crate) base_ref_name: &'a str,
	pub(crate) head_ref_name: &'a str,
	pub(crate) head_sha: &'a str,
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
	pub(in crate::state) schema_version: String,
	pub(in crate::state) subject_id: String,
	pub(in crate::state) sequence: i64,
	pub(in crate::state) transition: String,
	pub(in crate::state) previous_state: String,
	pub(in crate::state) next_state: String,
	pub(in crate::state) review_level: String,
	pub(in crate::state) review_gate_state: String,
	pub(in crate::state) base_branch: Option<String>,
	pub(in crate::state) validated_head_sha: String,
	pub(in crate::state) worktree_path: String,
	pub(in crate::state) merge_commit: Option<String>,
	pub(in crate::state) cleanup_state: String,
	pub(in crate::state) authority: String,
	pub(in crate::state) actor: String,
	pub(in crate::state) source_evidence_refs_json: String,
	pub(in crate::state) idempotency_key: String,
	pub(in crate::state) correlation_id: String,
	pub(in crate::state) causation_id: Option<String>,
	pub(in crate::state) decided_at: String,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}
