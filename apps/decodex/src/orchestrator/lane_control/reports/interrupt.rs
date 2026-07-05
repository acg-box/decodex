use serde::Serialize;

use crate::orchestrator::lane_control::reports::{
	LaneHardInterruptReport, LaneSoftInterruptReport,
};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LaneInterruptReport {
	pub(in crate::orchestrator::lane_control) project_id: String,
	pub(in crate::orchestrator::lane_control) issue: String,
	pub(in crate::orchestrator::lane_control) issue_id: String,
	pub(in crate::orchestrator::lane_control) issue_identifier: Option<String>,
	pub(in crate::orchestrator::lane_control) run_id: String,
	pub(in crate::orchestrator::lane_control) attempt_number: i64,
	pub(in crate::orchestrator::lane_control) force: bool,
	pub(in crate::orchestrator::lane_control) classification: String,
	pub(in crate::orchestrator::lane_control) soft_interrupt: LaneSoftInterruptReport,
	pub(in crate::orchestrator::lane_control) hard_interrupt: Option<LaneHardInterruptReport>,
	pub(in crate::orchestrator::lane_control) next_action: String,
}
impl LaneInterruptReport {
	pub(crate) fn http_status_line(&self) -> &'static str {
		if self.soft_interrupt.status == "pending" && self.hard_interrupt.is_none() {
			"202 Accepted"
		} else {
			"200 OK"
		}
	}
}
