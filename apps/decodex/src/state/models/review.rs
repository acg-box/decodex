/// Latest runtime-owned review-policy checkpoint for one run phase.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ReviewPolicyCheckpoint {
	pub(in crate::state) project_id: String,
	pub(in crate::state) issue_id: String,
	pub(in crate::state) run_id: String,
	pub(in crate::state) attempt_number: i64,
	pub(in crate::state) phase: String,
	pub(in crate::state) status: String,
	pub(in crate::state) head_sha: String,
	pub(in crate::state) nonclean_rounds: i64,
	pub(in crate::state) details_json: String,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}
#[cfg_attr(not(test), allow(dead_code))]
impl ReviewPolicyCheckpoint {
	pub(crate) fn project_id(&self) -> &str {
		&self.project_id
	}

	pub(crate) fn issue_id(&self) -> &str {
		&self.issue_id
	}

	pub(crate) fn run_id(&self) -> &str {
		&self.run_id
	}

	pub(crate) fn attempt_number(&self) -> i64 {
		self.attempt_number
	}

	pub(crate) fn phase(&self) -> &str {
		&self.phase
	}

	pub(crate) fn status(&self) -> &str {
		&self.status
	}

	pub(crate) fn head_sha(&self) -> &str {
		&self.head_sha
	}

	pub(crate) fn nonclean_rounds(&self) -> i64 {
		self.nonclean_rounds
	}

	pub(crate) fn details_json(&self) -> &str {
		&self.details_json
	}

	pub(crate) fn updated_at(&self) -> &str {
		&self.updated_at
	}

	pub(crate) fn updated_at_unix(&self) -> i64 {
		self.updated_at_unix
	}
}

/// Latest loop-guardrail checkpoint for one issue and stop reason.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LoopGuardrailCheckpoint {
	pub(in crate::state) project_id: String,
	pub(in crate::state) issue_id: String,
	pub(in crate::state) reason: String,
	pub(in crate::state) fingerprint: String,
	pub(in crate::state) run_id: String,
	pub(in crate::state) attempt_number: i64,
	pub(in crate::state) consecutive_count: i64,
	pub(in crate::state) details_json: String,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}
impl LoopGuardrailCheckpoint {
	#[cfg(test)]
	pub(crate) fn project_id(&self) -> &str {
		&self.project_id
	}

	#[cfg(test)]
	pub(crate) fn issue_id(&self) -> &str {
		&self.issue_id
	}

	pub(crate) fn reason(&self) -> &str {
		&self.reason
	}

	pub(crate) fn fingerprint(&self) -> &str {
		&self.fingerprint
	}

	pub(crate) fn run_id(&self) -> &str {
		&self.run_id
	}

	pub(crate) fn attempt_number(&self) -> i64 {
		self.attempt_number
	}

	pub(crate) fn consecutive_count(&self) -> i64 {
		self.consecutive_count
	}

	pub(crate) fn details_json(&self) -> &str {
		&self.details_json
	}

	#[cfg(test)]
	pub(crate) fn updated_at(&self) -> &str {
		&self.updated_at
	}

	#[cfg(test)]
	pub(crate) fn updated_at_unix(&self) -> i64 {
		self.updated_at_unix
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ReviewHandoffMarker {
	pub(in crate::state) run_id: String,
	pub(in crate::state) attempt_number: i64,
	pub(in crate::state) branch_name: String,
	pub(in crate::state) pr_url: String,
	pub(in crate::state) target_base_ref_name: Option<String>,
	pub(in crate::state) pr_head_ref_name: String,
	pub(in crate::state) pr_head_oid: String,
}
impl ReviewHandoffMarker {
	pub(crate) fn new(
		run_id: impl Into<String>,
		attempt_number: i64,
		branch_name: impl Into<String>,
		pr_url: impl Into<String>,
		target_base_ref_name: impl Into<String>,
		pr_head_ref_name: impl Into<String>,
		pr_head_oid: impl Into<String>,
	) -> Self {
		Self {
			run_id: run_id.into(),
			attempt_number,
			branch_name: branch_name.into(),
			pr_url: pr_url.into(),
			target_base_ref_name: Some(target_base_ref_name.into()),
			pr_head_ref_name: pr_head_ref_name.into(),
			pr_head_oid: pr_head_oid.into(),
		}
	}

	pub(crate) fn branch_name(&self) -> &str {
		&self.branch_name
	}

	pub(crate) fn run_id(&self) -> &str {
		&self.run_id
	}

	pub(crate) fn attempt_number(&self) -> i64 {
		self.attempt_number
	}

	pub(crate) fn pr_url(&self) -> &str {
		&self.pr_url
	}

	pub(crate) fn target_base_ref_name(&self) -> Option<&str> {
		self.target_base_ref_name.as_deref()
	}

	pub(crate) fn pr_head_ref_name(&self) -> &str {
		&self.pr_head_ref_name
	}

	pub(crate) fn pr_head_oid(&self) -> &str {
		&self.pr_head_oid
	}
}

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
impl ReviewOrchestrationMarker {
	#[allow(clippy::too_many_arguments)]
	pub(crate) fn new(
		run_id: impl Into<String>,
		attempt_number: i64,
		branch_name: impl Into<String>,
		pr_url: impl Into<String>,
		head_sha: impl Into<String>,
		phase: impl Into<String>,
		request_comment_database_id: Option<i64>,
		request_created_at_unix_epoch: Option<i64>,
		request_description_thumbs_up_count: Option<usize>,
		request_retry_count: i64,
		external_round_count: i64,
		auto_merge_enabled_at_unix_epoch: Option<i64>,
	) -> Self {
		Self {
			run_id: run_id.into(),
			attempt_number,
			branch_name: branch_name.into(),
			pr_url: pr_url.into(),
			head_sha: head_sha.into(),
			phase: phase.into(),
			request_comment_database_id,
			request_created_at_unix_epoch,
			request_description_thumbs_up_count,
			request_retry_count,
			external_round_count,
			auto_merge_enabled_at_unix_epoch,
		}
	}

	pub(crate) fn branch_name(&self) -> &str {
		&self.branch_name
	}

	pub(crate) fn run_id(&self) -> &str {
		&self.run_id
	}

	pub(crate) fn attempt_number(&self) -> i64 {
		self.attempt_number
	}

	pub(crate) fn pr_url(&self) -> &str {
		&self.pr_url
	}

	pub(crate) fn head_sha(&self) -> &str {
		&self.head_sha
	}

	pub(crate) fn phase(&self) -> &str {
		&self.phase
	}

	pub(crate) fn request_comment_database_id(&self) -> Option<i64> {
		self.request_comment_database_id
	}

	pub(crate) fn request_created_at_unix_epoch(&self) -> Option<i64> {
		self.request_created_at_unix_epoch
	}

	pub(crate) fn request_description_thumbs_up_count(&self) -> Option<usize> {
		self.request_description_thumbs_up_count
	}

	pub(crate) fn request_retry_count(&self) -> i64 {
		self.request_retry_count
	}

	pub(crate) fn external_round_count(&self) -> i64 {
		self.external_round_count
	}

	pub(crate) fn auto_merge_enabled_at_unix_epoch(&self) -> Option<i64> {
		self.auto_merge_enabled_at_unix_epoch
	}
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
#[allow(dead_code)]
impl ReviewLifecycleRecord {
	pub(crate) fn project_id(&self) -> &str {
		&self.project_id
	}

	pub(crate) fn issue_id(&self) -> &str {
		&self.issue_id
	}

	pub(crate) fn branch_name(&self) -> &str {
		&self.branch_name
	}

	pub(crate) fn run_id(&self) -> &str {
		&self.run_id
	}

	pub(crate) fn attempt_number(&self) -> i64 {
		self.attempt_number
	}

	pub(crate) fn pr_url(&self) -> &str {
		&self.pr_url
	}

	pub(crate) fn target_base_ref_name(&self) -> Option<&str> {
		self.target_base_ref_name.as_deref()
	}

	pub(crate) fn pr_head_ref_name(&self) -> &str {
		&self.pr_head_ref_name
	}

	pub(crate) fn pr_head_oid(&self) -> &str {
		&self.pr_head_oid
	}

	pub(crate) fn head_sha(&self) -> &str {
		&self.head_sha
	}

	pub(crate) fn phase(&self) -> &str {
		&self.phase
	}

	pub(crate) fn request_comment_database_id(&self) -> Option<i64> {
		self.request_comment_database_id
	}

	pub(crate) fn request_created_at_unix_epoch(&self) -> Option<i64> {
		self.request_created_at_unix_epoch
	}

	pub(crate) fn request_description_thumbs_up_count(&self) -> Option<usize> {
		self.request_description_thumbs_up_count
	}

	pub(crate) fn request_retry_count(&self) -> i64 {
		self.request_retry_count
	}

	pub(crate) fn external_round_count(&self) -> i64 {
		self.external_round_count
	}

	pub(crate) fn auto_merge_enabled_at_unix_epoch(&self) -> Option<i64> {
		self.auto_merge_enabled_at_unix_epoch
	}

	pub(crate) fn landing_state(&self) -> &str {
		&self.landing_state
	}

	pub(crate) fn closeout_state(&self) -> &str {
		&self.closeout_state
	}

	pub(crate) fn repair_attempt_count(&self) -> i64 {
		self.repair_attempt_count
	}

	pub(crate) fn evidence_json(&self) -> &str {
		&self.evidence_json
	}

	pub(crate) fn next_action(&self) -> &str {
		&self.next_action
	}

	pub(crate) fn updated_at(&self) -> &str {
		&self.updated_at
	}

	pub(crate) fn updated_at_unix(&self) -> i64 {
		self.updated_at_unix
	}
}
