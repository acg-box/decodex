use serde_json::Value;

use crate::{
	prelude::{Result, eyre},
	state::ReviewPolicyCheckpoint,
};

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
