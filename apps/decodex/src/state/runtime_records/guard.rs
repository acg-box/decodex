use crate::state::LoopGuardrailCheckpoint;

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

#[derive(Clone, Copy, Eq, PartialEq)]
pub(in crate::state) enum GuardRetention {
	Local,
	ParentAfterHandoff,
	AdoptingChild,
}
