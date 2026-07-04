#[derive(Clone)]
pub(in crate::orchestrator) struct ThreadArchiveCandidate {
	pub(in crate::orchestrator) issue_id: String,
	pub(in crate::orchestrator) issue_identifier: String,
	pub(in crate::orchestrator) run_id: String,
	pub(in crate::orchestrator) attempt_number: i64,
	pub(in crate::orchestrator) thread_id: String,
	pub(in crate::orchestrator) sequence_number: i64,
}

pub(in crate::orchestrator) struct ThreadArchiveCandidateSource<'a> {
	pub(in crate::orchestrator) run_id: &'a str,
	pub(in crate::orchestrator) issue_id: &'a str,
	pub(in crate::orchestrator) issue_identifier: &'a str,
	pub(in crate::orchestrator) attempt_number: i64,
	pub(in crate::orchestrator) thread_id: &'a str,
	pub(in crate::orchestrator) sequence_number: Option<i64>,
}
