#[cfg(test)]
use crate::state::ReviewLifecycleHandoffFixture;
use crate::state::ReviewLifecycleRecord;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(in crate::state) struct ReviewLifecycleKey {
	pub(in crate::state) project_id: String,
	pub(in crate::state) issue_id: String,
	pub(in crate::state) branch_name: String,
}
impl ReviewLifecycleKey {
	pub(in crate::state) fn new(project_id: &str, issue_id: &str, branch_name: &str) -> Self {
		Self {
			project_id: project_id.to_owned(),
			issue_id: issue_id.to_owned(),
			branch_name: branch_name.to_owned(),
		}
	}
}

#[derive(Clone, Debug)]
pub(in crate::state) struct ReviewLifecycleRuntimeRecord {
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
impl ReviewLifecycleRuntimeRecord {
	pub(in crate::state) fn as_public(&self) -> ReviewLifecycleRecord {
		ReviewLifecycleRecord {
			project_id: self.project_id.clone(),
			issue_id: self.issue_id.clone(),
			branch_name: self.branch_name.clone(),
			run_id: self.run_id.clone(),
			attempt_number: self.attempt_number,
			pr_url: self.pr_url.clone(),
			target_base_ref_name: self.target_base_ref_name.clone(),
			pr_head_ref_name: self.pr_head_ref_name.clone(),
			pr_head_oid: self.pr_head_oid.clone(),
			head_sha: self.head_sha.clone(),
			phase: self.phase.clone(),
			request_comment_database_id: self.request_comment_database_id,
			request_created_at_unix_epoch: self.request_created_at_unix_epoch,
			request_description_thumbs_up_count: self.request_description_thumbs_up_count,
			request_retry_count: self.request_retry_count,
			external_round_count: self.external_round_count,
			auto_merge_enabled_at_unix_epoch: self.auto_merge_enabled_at_unix_epoch,
			landing_state: self.landing_state.clone(),
			closeout_state: self.closeout_state.clone(),
			repair_attempt_count: self.repair_attempt_count,
			evidence_json: self.evidence_json.clone(),
			next_action: self.next_action.clone(),
			schema_version: self.schema_version.clone(),
			subject_id: self.subject_id.clone(),
			sequence: self.sequence,
			transition: self.transition.clone(),
			previous_state: self.previous_state.clone(),
			next_state: self.next_state.clone(),
			review_level: self.review_level.clone(),
			review_gate_state: self.review_gate_state.clone(),
			base_branch: self.base_branch.clone(),
			validated_head_sha: self.validated_head_sha.clone(),
			worktree_path: self.worktree_path.clone(),
			merge_commit: self.merge_commit.clone(),
			cleanup_state: self.cleanup_state.clone(),
			authority: self.authority.clone(),
			actor: self.actor.clone(),
			source_evidence_refs_json: self.source_evidence_refs_json.clone(),
			idempotency_key: self.idempotency_key.clone(),
			correlation_id: self.correlation_id.clone(),
			causation_id: self.causation_id.clone(),
			decided_at: self.decided_at.clone(),
			updated_at: self.updated_at.clone(),
			updated_at_unix: self.updated_at_unix,
		}
	}

	#[cfg(test)]
	pub(in crate::state) fn matches_handoff_identity(
		&self,
		handoff: &ReviewLifecycleHandoffFixture,
	) -> bool {
		self.run_id == handoff.run_id()
			&& self.attempt_number == handoff.attempt_number()
			&& self.branch_name == handoff.branch_name()
			&& self.pr_url == handoff.pr_url()
	}
}
