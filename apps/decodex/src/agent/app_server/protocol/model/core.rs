use serde::{Deserialize, Serialize};

#[derive(Default)]
pub(in crate::agent::app_server) struct RunOutcome {
	pub(in crate::agent::app_server) final_output: String,
	pub(in crate::agent::app_server) turn_id: String,
}

#[derive(Debug, Serialize)]
pub(in crate::agent::app_server) struct InitializeParams {
	#[serde(rename = "clientInfo")]
	pub(in crate::agent::app_server) client_info: ClientInfo,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) capabilities: Option<InitializeCapabilities>,
}

#[derive(Debug, Serialize)]
pub(in crate::agent::app_server) struct ClientInfo {
	pub(in crate::agent::app_server) name: String,
	pub(in crate::agent::app_server) version: String,
}

#[derive(Debug, Serialize)]
pub(in crate::agent::app_server) struct InitializeCapabilities {
	#[serde(rename = "experimentalApi", skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) experimental_api: Option<bool>,
	#[serde(default, rename = "optOutNotificationMethods", skip_serializing_if = "Vec::is_empty")]
	pub(in crate::agent::app_server) opt_out_notification_methods: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(in crate::agent::app_server) struct InitializeResponse {
	#[serde(rename = "userAgent")]
	pub(in crate::agent::app_server) user_agent: String,
	#[allow(dead_code)]
	#[serde(rename = "codexHome")]
	pub(in crate::agent::app_server) codex_home: String,
	#[allow(dead_code)]
	#[serde(rename = "platformFamily")]
	pub(in crate::agent::app_server) platform_family: String,
	#[allow(dead_code)]
	#[serde(rename = "platformOs")]
	pub(in crate::agent::app_server) platform_os: String,
}
