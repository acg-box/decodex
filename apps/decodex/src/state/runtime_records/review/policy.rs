use crate::state::ReviewPolicyCheckpoint;

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
