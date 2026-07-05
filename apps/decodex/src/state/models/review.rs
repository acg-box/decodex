mod checkpoints;
mod handoff;
mod records;

pub(crate) use self::{
	checkpoints::{LoopGuardrailCheckpoint, ReviewPolicyCheckpoint},
	handoff::ReviewHandoffMarker,
	records::{ReviewLifecycleRecord, ReviewOrchestrationMarker},
};

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
