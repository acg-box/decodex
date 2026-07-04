use serde::Deserialize;

#[derive(Deserialize)]
pub(in crate::orchestrator::operator_http) struct OperatorAccountRequest {
	pub(in crate::orchestrator::operator_http) selector: Option<String>,
	pub(in crate::orchestrator::operator_http) auth_json_path: Option<String>,
	pub(in crate::orchestrator::operator_http) random_name_offset: Option<i64>,
}

#[derive(Deserialize)]
pub(in crate::orchestrator::operator_http) struct OperatorLinearScanHttpRequest {
	#[serde(alias = "projectId")]
	pub(in crate::orchestrator::operator_http) project_id: Option<String>,
}

#[derive(Deserialize)]
pub(in crate::orchestrator::operator_http) struct OperatorLaneInterruptHttpRequest {
	#[serde(alias = "projectId")]
	pub(in crate::orchestrator::operator_http) project_id: Option<String>,
	pub(in crate::orchestrator::operator_http) issue: String,
	#[serde(alias = "runId")]
	pub(in crate::orchestrator::operator_http) run_id: String,
	pub(in crate::orchestrator::operator_http) force: Option<bool>,
	pub(in crate::orchestrator::operator_http) reason: Option<String>,
}

#[derive(Deserialize)]
pub(in crate::orchestrator::operator_http) struct OperatorLaneSteerHttpRequest {
	#[serde(alias = "projectId")]
	pub(in crate::orchestrator::operator_http) project_id: Option<String>,
	pub(in crate::orchestrator::operator_http) issue: Option<String>,
	#[serde(alias = "issueId")]
	pub(in crate::orchestrator::operator_http) issue_id: Option<String>,
	#[serde(alias = "runId")]
	pub(in crate::orchestrator::operator_http) run_id: String,
	#[serde(alias = "expectedTurnId")]
	pub(in crate::orchestrator::operator_http) expected_turn_id: String,
	pub(in crate::orchestrator::operator_http) message: String,
	#[serde(alias = "waitTimeoutMs")]
	pub(in crate::orchestrator::operator_http) wait_timeout_ms: Option<u64>,
}
