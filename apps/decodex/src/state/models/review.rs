mod checkpoints;
#[cfg(test)] mod handoff;
mod records;

#[cfg(test)] pub(crate) use self::handoff::ReviewLifecycleHandoffFixture;
#[cfg(test)] pub(crate) use self::records::ReviewLifecycleTransitionFixture;
pub(crate) use self::{
	checkpoints::{LoopGuardrailCheckpoint, ReviewPolicyCheckpoint},
	records::{ReviewLifecycleHandoffInput, ReviewLifecycleRecord, ReviewLifecycleTransitionInput},
};

pub(crate) trait ReviewLifecycleReadback {
	fn branch_name(&self) -> &str;
	fn run_id(&self) -> &str;
	fn attempt_number(&self) -> i64;
	fn pr_url(&self) -> &str;
	fn head_sha(&self) -> &str;
	fn request_comment_database_id(&self) -> Option<i64>;
	fn request_created_at_unix_epoch(&self) -> Option<i64>;
	fn request_retry_count(&self) -> i64;
}

#[cfg(test)]
#[allow(dead_code)]
impl ReviewLifecycleTransitionFixture {
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

	#[cfg(test)]
	pub(crate) fn phase(&self) -> &str {
		&self.phase
	}

	pub(crate) fn request_comment_database_id(&self) -> Option<i64> {
		self.request_comment_database_id
	}

	pub(crate) fn request_created_at_unix_epoch(&self) -> Option<i64> {
		self.request_created_at_unix_epoch
	}

	#[cfg(test)]
	pub(crate) fn request_description_thumbs_up_count(&self) -> Option<usize> {
		self.request_description_thumbs_up_count
	}

	pub(crate) fn request_retry_count(&self) -> i64 {
		self.request_retry_count
	}

	#[cfg(test)]
	pub(crate) fn external_round_count(&self) -> i64 {
		self.external_round_count
	}

	#[cfg(test)]
	pub(crate) fn auto_merge_enabled_at_unix_epoch(&self) -> Option<i64> {
		self.auto_merge_enabled_at_unix_epoch
	}

	pub(crate) fn from_lifecycle_record(record: &ReviewLifecycleRecord) -> Self {
		Self::new(
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
		)
	}
}

#[cfg(test)]
#[allow(dead_code)]
impl ReviewLifecycleHandoffFixture {
	pub(crate) fn from_lifecycle_record(record: &ReviewLifecycleRecord) -> Option<Self> {
		Some(Self::new(
			record.run_id().to_owned(),
			record.attempt_number(),
			record.branch_name().to_owned(),
			record.pr_url().to_owned(),
			record.target_base_ref_name()?.to_owned(),
			record.pr_head_ref_name().to_owned(),
			record.pr_head_oid().to_owned(),
		))
	}
}

#[cfg(test)]
impl ReviewLifecycleReadback for ReviewLifecycleTransitionFixture {
	fn branch_name(&self) -> &str {
		self.branch_name()
	}

	fn run_id(&self) -> &str {
		self.run_id()
	}

	fn attempt_number(&self) -> i64 {
		self.attempt_number()
	}

	fn pr_url(&self) -> &str {
		self.pr_url()
	}

	fn head_sha(&self) -> &str {
		self.head_sha()
	}

	fn request_comment_database_id(&self) -> Option<i64> {
		self.request_comment_database_id()
	}

	fn request_created_at_unix_epoch(&self) -> Option<i64> {
		self.request_created_at_unix_epoch()
	}

	fn request_retry_count(&self) -> i64 {
		self.request_retry_count()
	}
}

#[allow(dead_code)]
impl ReviewLifecycleRecord {
	#[cfg(test)]
	pub(crate) fn from_test_lifecycle_fixtures(
		handoff: &ReviewLifecycleHandoffFixture,
		orchestration: Option<&ReviewLifecycleTransitionFixture>,
	) -> Self {
		let head_sha = orchestration
			.map(ReviewLifecycleTransitionFixture::head_sha)
			.unwrap_or_else(|| handoff.pr_head_oid());
		Self {
			project_id: String::from("test"),
			issue_id: String::from("test"),
			branch_name: handoff.branch_name().to_owned(),
			run_id: handoff.run_id().to_owned(),
			attempt_number: handoff.attempt_number(),
			pr_url: handoff.pr_url().to_owned(),
			target_base_ref_name: handoff.target_base_ref_name().map(str::to_owned),
			pr_head_ref_name: handoff.pr_head_ref_name().to_owned(),
			pr_head_oid: handoff.pr_head_oid().to_owned(),
			head_sha: head_sha.to_owned(),
			phase: orchestration
				.map(ReviewLifecycleTransitionFixture::phase)
				.unwrap_or("request_pending")
				.to_owned(),
			request_comment_database_id: orchestration
				.and_then(ReviewLifecycleTransitionFixture::request_comment_database_id),
			request_created_at_unix_epoch: orchestration
				.and_then(ReviewLifecycleTransitionFixture::request_created_at_unix_epoch),
			request_description_thumbs_up_count: orchestration
				.and_then(ReviewLifecycleTransitionFixture::request_description_thumbs_up_count),
			request_retry_count: orchestration
				.map(ReviewLifecycleTransitionFixture::request_retry_count)
				.unwrap_or(0),
			external_round_count: orchestration
				.map(ReviewLifecycleTransitionFixture::external_round_count)
				.unwrap_or(0),
			auto_merge_enabled_at_unix_epoch: orchestration
				.and_then(ReviewLifecycleTransitionFixture::auto_merge_enabled_at_unix_epoch),
			landing_state: String::from("not_started"),
			closeout_state: String::from("not_started"),
			repair_attempt_count: 0,
			evidence_json: String::from("{}"),
			next_action: String::from("wait_for_external_review_result"),
			schema_version: String::from("test"),
			subject_id: String::from("test"),
			sequence: 1,
			transition: String::from("test"),
			previous_state: String::from("test"),
			next_state: String::from("test"),
			review_level: String::from("standard"),
			review_gate_state: String::from("pending"),
			base_branch: handoff.target_base_ref_name().map(str::to_owned),
			validated_head_sha: head_sha.to_owned(),
			worktree_path: String::new(),
			merge_commit: None,
			cleanup_state: String::from("pending"),
			authority: String::from("test"),
			actor: String::from("test"),
			source_evidence_refs_json: String::from("[]"),
			idempotency_key: String::from("test"),
			correlation_id: String::from("test"),
			causation_id: None,
			decided_at: String::from("1970-01-01T00:00:00Z"),
			updated_at: String::from("1970-01-01T00:00:00Z"),
			updated_at_unix: 0,
		}
	}

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

	pub(crate) fn schema_version(&self) -> &str {
		&self.schema_version
	}

	pub(crate) fn subject_id(&self) -> &str {
		&self.subject_id
	}

	pub(crate) fn sequence(&self) -> i64 {
		self.sequence
	}

	pub(crate) fn transition(&self) -> &str {
		&self.transition
	}

	pub(crate) fn previous_state(&self) -> &str {
		&self.previous_state
	}

	pub(crate) fn next_state(&self) -> &str {
		&self.next_state
	}

	pub(crate) fn review_level(&self) -> &str {
		&self.review_level
	}

	pub(crate) fn review_gate_state(&self) -> &str {
		&self.review_gate_state
	}

	pub(crate) fn base_branch(&self) -> Option<&str> {
		self.base_branch.as_deref()
	}

	pub(crate) fn validated_head_sha(&self) -> &str {
		&self.validated_head_sha
	}

	pub(crate) fn worktree_path(&self) -> &str {
		&self.worktree_path
	}

	pub(crate) fn merge_commit(&self) -> Option<&str> {
		self.merge_commit.as_deref()
	}

	pub(crate) fn cleanup_state(&self) -> &str {
		&self.cleanup_state
	}

	pub(crate) fn authority(&self) -> &str {
		&self.authority
	}

	pub(crate) fn actor(&self) -> &str {
		&self.actor
	}

	pub(crate) fn source_evidence_refs_json(&self) -> &str {
		&self.source_evidence_refs_json
	}

	pub(crate) fn idempotency_key(&self) -> &str {
		&self.idempotency_key
	}

	pub(crate) fn correlation_id(&self) -> &str {
		&self.correlation_id
	}

	pub(crate) fn causation_id(&self) -> Option<&str> {
		self.causation_id.as_deref()
	}

	pub(crate) fn decided_at(&self) -> &str {
		&self.decided_at
	}

	pub(crate) fn updated_at(&self) -> &str {
		&self.updated_at
	}

	pub(crate) fn updated_at_unix(&self) -> i64 {
		self.updated_at_unix
	}
}

impl ReviewLifecycleReadback for ReviewLifecycleRecord {
	fn branch_name(&self) -> &str {
		self.branch_name()
	}

	fn run_id(&self) -> &str {
		self.run_id()
	}

	fn attempt_number(&self) -> i64 {
		self.attempt_number()
	}

	fn pr_url(&self) -> &str {
		self.pr_url()
	}

	fn head_sha(&self) -> &str {
		self.head_sha()
	}

	fn request_comment_database_id(&self) -> Option<i64> {
		self.request_comment_database_id()
	}

	fn request_created_at_unix_epoch(&self) -> Option<i64> {
		self.request_created_at_unix_epoch()
	}

	fn request_retry_count(&self) -> i64 {
		self.request_retry_count()
	}
}
