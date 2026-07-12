use serde::Serialize;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct AuthorityTimelineEntry {
	pub(crate) generation: u64,
	pub(crate) sequence: u64,
	pub(crate) event_id: String,
	pub(crate) event_type: String,
	pub(crate) transition_id: String,
	pub(crate) correlation_id: String,
	pub(crate) causation_id: String,
	pub(crate) project_key: String,
	pub(crate) tracker_issue_id: String,
	pub(crate) binding_fingerprint: String,
	pub(crate) invocation_fingerprint: String,
	pub(crate) facts_fingerprint: String,
	pub(crate) decision: String,
	pub(crate) reason_codes: Vec<String>,
	pub(crate) operation_id: Option<String>,
	pub(crate) runtime_version: String,
	pub(crate) recorded_at_unix_micros: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct AuthorityTimelineReport {
	pub(crate) schema: &'static str,
	pub(crate) project_key: String,
	pub(crate) tracker_issue_id: String,
	pub(crate) event_count: usize,
	pub(crate) events: Vec<AuthorityTimelineEntry>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct AuthorityAuditReport {
	pub(crate) schema: &'static str,
	pub(crate) chain_valid: bool,
	pub(crate) generation: u64,
	pub(crate) total_event_count: usize,
	pub(crate) first_sequence: Option<u64>,
	pub(crate) last_sequence: Option<u64>,
	pub(crate) lane_event_count: usize,
	pub(crate) project_key: String,
	pub(crate) tracker_issue_id: String,
	pub(crate) privacy_projection: &'static str,
}
