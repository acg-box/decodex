use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::agent::app_server) struct SkillsListParams {
	pub(in crate::agent::app_server) cwds: Vec<String>,
	pub(in crate::agent::app_server) force_reload: bool,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) per_cwd_extra_user_roots: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub(in crate::agent::app_server) struct SkillsListResponse {
	pub(in crate::agent::app_server) data: Vec<SkillsListEntry>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
pub(in crate::agent::app_server) struct SkillsListEntry {
	pub(in crate::agent::app_server) cwd: String,
	pub(in crate::agent::app_server) errors: Vec<SkillErrorInfo>,
	pub(in crate::agent::app_server) skills: Vec<SkillMetadata>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
pub(in crate::agent::app_server) struct SkillErrorInfo {
	pub(in crate::agent::app_server) message: String,
	pub(in crate::agent::app_server) path: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
pub(in crate::agent::app_server) struct SkillMetadata {
	pub(in crate::agent::app_server) enabled: bool,
	pub(in crate::agent::app_server) name: String,
	pub(in crate::agent::app_server) scope: String,
}

#[derive(Debug, Serialize)]
pub(in crate::agent::app_server) struct PluginListParams {
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) cwds: Option<Vec<String>>,
	#[serde(rename = "marketplaceKinds", skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) marketplace_kinds: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub(in crate::agent::app_server) struct PluginListResponse {
	pub(in crate::agent::app_server) marketplaces: Vec<PluginMarketplaceEntry>,
	#[serde(default, rename = "marketplaceLoadErrors")]
	pub(in crate::agent::app_server) marketplace_load_errors: Vec<MarketplaceLoadErrorInfo>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
pub(in crate::agent::app_server) struct PluginMarketplaceEntry {
	pub(in crate::agent::app_server) name: String,
	pub(in crate::agent::app_server) plugins: Vec<PluginSummary>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
pub(in crate::agent::app_server) struct PluginSummary {
	pub(in crate::agent::app_server) enabled: bool,
	pub(in crate::agent::app_server) id: String,
	pub(in crate::agent::app_server) installed: bool,
	pub(in crate::agent::app_server) name: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
pub(in crate::agent::app_server) struct MarketplaceLoadErrorInfo {
	#[serde(rename = "marketplacePath")]
	pub(in crate::agent::app_server) marketplace_path: String,
	pub(in crate::agent::app_server) message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::agent::app_server) struct ListMcpServerStatusParams {
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) cursor: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) detail: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) limit: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub(in crate::agent::app_server) struct ListMcpServerStatusResponse {
	pub(in crate::agent::app_server) data: Vec<McpServerStatusSummary>,
	#[serde(rename = "nextCursor")]
	pub(in crate::agent::app_server) next_cursor: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub(in crate::agent::app_server) struct McpServerStatusSummary {
	#[serde(rename = "authStatus")]
	pub(in crate::agent::app_server) auth_status: String,
	pub(in crate::agent::app_server) name: String,
	pub(in crate::agent::app_server) tools: BTreeMap<String, Value>,
}
