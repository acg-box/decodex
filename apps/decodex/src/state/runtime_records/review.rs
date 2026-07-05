use serde_json::Value;

use crate::{
	prelude::{Result, eyre},
	state::{
		LoopGuardrailCheckpoint, ReviewHandoffMarker, ReviewLifecycleRecord, ReviewPolicyCheckpoint,
	},
};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(in crate::state) struct LoopGuardrailKey {
	pub(in crate::state) project_id: String,
	pub(in crate::state) issue_id: String,
	pub(in crate::state) reason: String,
}
impl LoopGuardrailKey {
	pub(in crate::state) fn new(project_id: &str, issue_id: &str, reason: &str) -> Self {
		Self {
			project_id: project_id.to_owned(),
			issue_id: issue_id.to_owned(),
			reason: reason.to_owned(),
		}
	}
}

#[derive(Clone, Debug)]
pub(in crate::state) struct LoopGuardrailRuntimeRecord {
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
impl LoopGuardrailRuntimeRecord {
	pub(in crate::state) fn as_public(&self) -> LoopGuardrailCheckpoint {
		LoopGuardrailCheckpoint {
			project_id: self.project_id.clone(),
			issue_id: self.issue_id.clone(),
			reason: self.reason.clone(),
			fingerprint: self.fingerprint.clone(),
			run_id: self.run_id.clone(),
			attempt_number: self.attempt_number,
			consecutive_count: self.consecutive_count,
			details_json: self.details_json.clone(),
			updated_at: self.updated_at.clone(),
			updated_at_unix: self.updated_at_unix,
		}
	}
}

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

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(in crate::state) struct ReviewPolicyKey {
	pub(in crate::state) project_id: String,
	pub(in crate::state) issue_id: String,
	pub(in crate::state) run_id: String,
	pub(in crate::state) attempt_number: i64,
	pub(in crate::state) phase: String,
}
impl ReviewPolicyKey {
	pub(in crate::state) fn new(
		project_id: &str,
		issue_id: &str,
		run_id: &str,
		attempt_number: i64,
		phase: &str,
	) -> Self {
		Self {
			project_id: project_id.to_owned(),
			issue_id: issue_id.to_owned(),
			run_id: run_id.to_owned(),
			attempt_number,
			phase: phase.to_owned(),
		}
	}
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(in crate::state) struct EvidenceArtifactKey {
	pub(in crate::state) project_id: String,
	pub(in crate::state) issue_id: String,
	pub(in crate::state) artifact_kind: String,
	pub(in crate::state) key_hash: String,
}
impl EvidenceArtifactKey {
	pub(in crate::state) fn new(
		project_id: &str,
		issue_id: &str,
		artifact_kind: &str,
		key_hash: &str,
	) -> Self {
		Self {
			project_id: project_id.to_owned(),
			issue_id: issue_id.to_owned(),
			artifact_kind: artifact_kind.to_owned(),
			key_hash: key_hash.to_owned(),
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
			updated_at: self.updated_at.clone(),
			updated_at_unix: self.updated_at_unix,
		}
	}

	pub(in crate::state) fn matches_handoff_identity(&self, handoff: &ReviewHandoffMarker) -> bool {
		self.run_id == handoff.run_id()
			&& self.attempt_number == handoff.attempt_number()
			&& self.branch_name == handoff.branch_name()
			&& self.pr_url == handoff.pr_url()
	}
}

#[derive(Clone, Debug)]
pub(in crate::state) struct ReviewPolicyRuntimeRecord {
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
impl ReviewPolicyRuntimeRecord {
	pub(in crate::state) fn as_public(&self) -> ReviewPolicyCheckpoint {
		ReviewPolicyCheckpoint {
			project_id: self.project_id.clone(),
			issue_id: self.issue_id.clone(),
			run_id: self.run_id.clone(),
			attempt_number: self.attempt_number,
			phase: self.phase.clone(),
			status: self.status.clone(),
			head_sha: self.head_sha.clone(),
			nonclean_rounds: self.nonclean_rounds,
			details_json: self.details_json.clone(),
			updated_at: self.updated_at.clone(),
			updated_at_unix: self.updated_at_unix,
		}
	}
}

#[derive(Clone, Debug)]
pub(in crate::state) struct EvidenceArtifactRuntimeRecord {
	pub(in crate::state) project_id: String,
	pub(in crate::state) issue_id: String,
	pub(in crate::state) artifact_kind: String,
	pub(in crate::state) key_hash: String,
	pub(in crate::state) phase: String,
	pub(in crate::state) status: String,
	pub(in crate::state) head_sha: Option<String>,
	pub(in crate::state) key_json: String,
	pub(in crate::state) payload_json: String,
	pub(in crate::state) source_run_id: String,
	pub(in crate::state) source_attempt_number: i64,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}
impl EvidenceArtifactRuntimeRecord {
	pub(in crate::state) fn as_review_policy_checkpoint(&self) -> Result<ReviewPolicyCheckpoint> {
		let payload = serde_json::from_str::<Value>(&self.payload_json).map_err(|error| {
			eyre::eyre!(
				"Invalid review checkpoint artifact payload for issue `{}` phase `{}` head `{:?}`: {error}",
				self.issue_id,
				self.phase,
				self.head_sha
			)
		})?;
		let nonclean_rounds =
			payload.get("nonclean_rounds").and_then(Value::as_i64).unwrap_or_default();
		let details_json =
			payload.get("details_json").and_then(Value::as_str).unwrap_or("{}").to_owned();

		Ok(ReviewPolicyCheckpoint {
			project_id: self.project_id.clone(),
			issue_id: self.issue_id.clone(),
			run_id: self.source_run_id.clone(),
			attempt_number: self.source_attempt_number,
			phase: self.phase.clone(),
			status: self.status.clone(),
			head_sha: self.head_sha.clone().unwrap_or_default(),
			nonclean_rounds,
			details_json,
			updated_at: self.updated_at.clone(),
			updated_at_unix: self.updated_at_unix,
		})
	}
}
