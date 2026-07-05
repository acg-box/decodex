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
