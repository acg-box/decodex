use serde::Serialize;

use crate::recovery::{GHOST_LANE_CLASSIFICATION, MCP_TEST_FIXTURE_GHOST_LANE_CLASSIFICATION};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(in crate::recovery) struct GhostLaneRecoveryReport {
	pub(in crate::recovery) project_id: String,
	pub(in crate::recovery) diagnostics: Vec<GhostLaneDiagnostic>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(in crate::recovery) struct GhostLaneDiagnostic {
	pub(in crate::recovery) project_id: String,
	pub(in crate::recovery) issue_id: String,
	pub(in crate::recovery) issue_identifier: Option<String>,
	pub(in crate::recovery) run_id: String,
	pub(in crate::recovery) attempt_number: i64,
	pub(in crate::recovery) attempt_status: String,
	pub(in crate::recovery) classification: String,
	pub(in crate::recovery) reason: String,
	pub(in crate::recovery) run_lease: bool,
	pub(in crate::recovery) control_channel: String,
	pub(in crate::recovery) evidence: Vec<String>,
	pub(in crate::recovery) blockers: Vec<String>,
	pub(in crate::recovery) next_action: String,
}
impl GhostLaneDiagnostic {
	pub(in crate::recovery) fn recoverable(&self) -> bool {
		(self.classification == GHOST_LANE_CLASSIFICATION
			|| self.classification == MCP_TEST_FIXTURE_GHOST_LANE_CLASSIFICATION)
			&& self.blockers.is_empty()
	}
}
