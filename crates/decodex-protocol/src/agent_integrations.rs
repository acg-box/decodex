//! Independent native catalog and runtime observations for one task.
use serde::{Deserialize, Serialize};

/// One installed connector in the native committed runtime snapshot.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentAppStatusDto {
	/// Exact native connector identity.
	pub id: String,
	/// Best-effort runtime name, not canonical directory metadata.
	pub runtime_name: Option<String>,
	/// Effective configuration permits this connector.
	pub enabled: bool,
	/// The snapshot has a model-visible tool permitted by effective policy.
	pub callable: bool,
}

/// Selected MCP status fields; tool inventory does not prove runtime readiness.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentMcpStatusDto {
	/// Exact configured server name.
	pub name: String,
	/// Native owning plugin when known.
	pub plugin_id: Option<String>,
	/// Public presentation advertised by the initialized server, when available.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub presentation: Option<String>,
	/// Connection state, or None when native state is unavailable.
	pub runtime_status: Option<String>,
	/// Authentication observation, independent of the connection state.
	pub auth_status: String,
	/// Number of discovered tools, including a cached catalog.
	pub tool_count: usize,
	/// Discovery failure; Some means the empty catalog is not confirmed.
	pub tools_error: Option<String>,
	/// Number of resources in this inventory.
	pub resource_count: usize,
	/// Number of resource templates in this inventory.
	pub template_count: usize,
	/// Advertised names, including extensions/<name>; None means unknown, not empty.
	pub advertised_capabilities: Option<Vec<String>>,
}

/// Repository-scoped plugin configuration, not a runtime availability claim.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentPluginStatusDto {
	/// Exact native plugin identity.
	pub id: String,
	/// Human-readable plugin name.
	pub name: String,
	/// Whether native Codex reports an installed bundle.
	pub installed: bool,
	/// Whether native repository configuration enables the plugin.
	pub enabled: bool,
	/// Native availability policy state, or unknown for older providers.
	pub availability: String,
	/// Native reason for policy unavailability.
	pub disabled_reason: Option<String>,
}

/// Installed connector discovery, independent of MCP and plugin discovery.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum AgentAppInventory {
	/// Complete bounded runtime snapshot; not a successful tool execution receipt.
	Available {
		/// Installed connector observations.
		apps: Vec<AgentAppStatusDto>,
	},
	/// This provider has no supported endpoint.
	Unsupported,
	/// Complete inventory exceeds the public bound.
	CapacityExceeded,
	/// Current status cannot be read.
	Unavailable,
}

/// MCP discovery result, independent of plugin discovery success.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum AgentMcpInventory {
	/// Complete bounded inventory.
	Available {
		/// Server observations.
		servers: Vec<AgentMcpStatusDto>,
	},
	/// This provider has no supported endpoint.
	Unsupported,
	/// Complete inventory exceeds the public bound.
	CapacityExceeded,
	/// Current status cannot be read.
	Unavailable,
}

/// Plugin discovery preserves repository load errors alongside returned entries.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum AgentPluginInventory {
	/// Native catalog was read; errors identify incomplete repository discovery.
	Available {
		/// Returned plugin entries.
		plugins: Vec<AgentPluginStatusDto>,
		/// Native marketplace load failures.
		errors: Vec<String>,
	},
	/// This provider has no supported endpoint.
	Unsupported,
	/// Complete inventory exceeds the public bound.
	CapacityExceeded,
	/// Current status cannot be read.
	Unavailable,
}

/// Source-bound native integrations for the selected task.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum AgentIntegrationsResult {
	/// The exact thread and repository remained current throughout the read.
	Available {
		/// Native task working directory.
		cwd: String,
		/// MCP observations.
		mcp: AgentMcpInventory,
		/// Repository plugin observations.
		plugins: AgentPluginInventory,
		/// Native installed connector state.
		apps: AgentAppInventory,
	},
	/// The complete projection exceeds the response bound.
	CapacityExceeded,
	/// Thread, repository or connection ownership cannot be verified.
	Unavailable,
}
