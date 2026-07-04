use serde::Deserialize;

use crate::orchestrator::operator_http::types::DashboardClientSubscription;

pub(in crate::orchestrator::operator_http) enum DashboardClientFrame {
	Text(Vec<u8>),
	Close,
	Ping(Vec<u8>),
	Pong,
}

#[derive(Default)]
pub(in crate::orchestrator::operator_http) struct DashboardWebSocketSession {
	pub(in crate::orchestrator::operator_http) subscription: DashboardClientSubscription,
}

#[derive(Debug, Deserialize)]
pub(in crate::orchestrator::operator_http) struct DashboardClientMessage {
	#[serde(rename = "type")]
	pub(in crate::orchestrator::operator_http) message_type: String,
	#[serde(rename = "requestId")]
	pub(in crate::orchestrator::operator_http) request_id: Option<String>,

	pub(in crate::orchestrator::operator_http) action: Option<String>,
	#[serde(rename = "projectId")]
	pub(in crate::orchestrator::operator_http) project_id: Option<String>,
	#[serde(rename = "issueId")]
	pub(in crate::orchestrator::operator_http) issue_id: Option<String>,
	#[serde(rename = "runId")]
	pub(in crate::orchestrator::operator_http) run_id: Option<String>,
	#[serde(rename = "accountSelector")]
	pub(in crate::orchestrator::operator_http) account_selector: Option<String>,
}

pub(in crate::orchestrator::operator_http) struct DashboardControlAck<'a> {
	pub(in crate::orchestrator::operator_http) request_id: Option<&'a str>,
	pub(in crate::orchestrator::operator_http) action: &'a str,
	pub(in crate::orchestrator::operator_http) accepted: bool,
	pub(in crate::orchestrator::operator_http) status: &'a str,
	pub(in crate::orchestrator::operator_http) message: &'a str,
	pub(in crate::orchestrator::operator_http) project_id: Option<&'a str>,
	pub(in crate::orchestrator::operator_http) issue_id: Option<&'a str>,
	pub(in crate::orchestrator::operator_http) run_id: Option<&'a str>,
	pub(in crate::orchestrator::operator_http) subscription:
		Option<&'a DashboardClientSubscription>,
}
