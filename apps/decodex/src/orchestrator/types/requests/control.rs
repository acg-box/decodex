use crate::orchestrator::types::{Duration, Path, Serialize};

/// Multi-project local control-plane daemon request.
pub(crate) struct ServeRequest<'a> {
	pub(crate) config_path: Option<&'a Path>,
	pub(crate) listen_address: &'a str,
	pub(crate) dev: bool,
}

/// Agent-readable runtime diagnosis request.
pub(crate) struct DiagnoseRequest<'a> {
	pub(crate) config_path: Option<&'a Path>,
	pub(crate) json: bool,
	pub(crate) limit: usize,
}

/// Local private execution evidence readback request.
pub(crate) struct EvidenceRequest<'a> {
	pub(crate) config_path: Option<&'a Path>,
	pub(crate) project_id: Option<&'a str>,
	pub(crate) issue: &'a str,
	pub(crate) run_id: Option<&'a str>,
	pub(crate) attempt_number: Option<i64>,
	pub(crate) json: bool,
	pub(crate) include_payload: bool,
}

/// Current lane steer request.
pub(crate) struct LaneSteerRequest<'a> {
	pub(crate) config_path: Option<&'a Path>,
	pub(crate) project_id: Option<&'a str>,
	pub(crate) issue: &'a str,
	pub(crate) run_id: &'a str,
	pub(crate) expected_turn_id: &'a str,
	pub(crate) message: &'a str,
	pub(crate) source: &'a str,
	pub(crate) wait_timeout: Duration,
}

/// Current lane steer result without raw operator message content.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LaneSteerReport {
	pub(crate) project_id: String,
	pub(crate) issue_id: String,
	pub(crate) issue_identifier: Option<String>,
	pub(crate) run_id: String,
	pub(crate) attempt_number: i64,
	pub(crate) thread_id: Option<String>,
	pub(crate) expected_turn_id: String,
	pub(crate) current_turn_id: Option<String>,
	pub(crate) response_turn_id: Option<String>,
	pub(crate) audit_record_id: i64,
	pub(crate) request_id: String,
	pub(crate) request_path: Option<String>,
	pub(crate) outcome: String,
	pub(crate) reason: String,
	pub(crate) failure_class: Option<String>,
	pub(crate) delivery_status: String,
	pub(crate) message_byte_count: usize,
	pub(crate) message_line_count: usize,
}
