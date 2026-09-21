//! Native installation suggestions are distinct from ordinary MCP approval forms.
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One connector's fresh native access state and optional authorization page.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChiefInstallApp {
	/// Exact native connector identity.
	pub id: String,
	/// Native display name.
	pub name: String,
	/// Missing native access evidence is not success.
	pub accessible: bool,
	/// Whether repository configuration enables the connector.
	pub enabled: bool,
	/// Native link, validated before it enters this projection.
	pub install_url: Option<crate::McpAuthorizationUrl>,
}

/// Inspection is read-only. Only explicit commands may install or answer a suggestion.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum ChiefInstallState {
	/// Fresh, exact-target catalog and connector evidence.
	Available {
		/// Exact pending event inspected.
		event_id: i64,
		/// Exact integration identity.
		tool_id: String,
		/// Native display name.
		tool_name: String,
		/// Plugin installation evidence; None for connector-only requests.
		installed: Option<bool>,
		/// A durable attempt exists; its result must be reconciled, never replayed.
		attempted: bool,
		/// Whether a confirmed receipt supplies all installation-time connector requirements.
		authorization_requirements_known: bool,
		/// Whether explicit installation is currently allowed.
		can_install: bool,
		/// True only after plugin installation and connector access are verified.
		can_continue: bool,
		/// Identity of the exact target/policy facts shown to the user.
		review_token: String,
		/// Native source and policy facts that must accompany installation consent.
		review_details: String,
		/// Required connectors and their independent access observations.
		apps: Vec<ChiefInstallApp>,
	},
	/// Missing, stale or incomplete evidence; this never authorizes installation.
	Unavailable,
}

/// A validated installation target. Catalog lookup must resolve the actual install request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum McpInstallTarget {
	/// Plugin identity and optional hosted service identity supplied by native Codex.
	Plugin {
		/// Suggestion correlation, not an installation attempt or idempotency key.
		suggestion_id: Option<String>,
		/// Hosted plugin identity; absent for local marketplace plugins.
		remote_plugin_id: Option<String>,
		/// Connectors whose accessibility native Codex checks after installation.
		app_connector_ids: Vec<String>,
	},
	/// A connector must be connected through its native installation page.
	Connector,
}

/// Bounded suggestion facts. Parsing does not authorize installation or prove availability.
#[derive(Clone, Eq, PartialEq)]
pub struct McpInstallSuggestion {
	/// Exact catalog identity, never a display-name-derived install target.
	pub tool_id: String,
	/// Display name from the native request.
	pub tool_name: String,
	/// Type-specific native identities.
	pub target: McpInstallTarget,
	install_url: Option<String>,
}

impl std::fmt::Debug for McpInstallSuggestion {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str("McpInstallSuggestion([private request facts])")
	}
}

impl McpInstallSuggestion {
	/// Return None for ordinary forms; reject malformed or unsupported suggestions.
	pub fn from_request(request: &Value) -> Result<Option<Self>, &'static str> {
		let meta = &request["_meta"];
		if meta["codex_approval_kind"] != "tool_suggestion" {
			return Ok(None);
		}
		if request["serverName"] != "codex_apps"
			|| !matches!(request["mode"].as_str(), Some("form" | "openai/form" | "openaiForm"))
			|| meta["suggest_type"] != "install"
			|| !crate::mcp_request_fields(request)
				.map_err(|_| "Invalid installation suggestion form")?
				.is_empty()
		{
			return Err("Unsupported installation suggestion");
		}
		let tool_id = text(&meta["tool_id"], 1024)?;
		let tool_name = text(&meta["tool_name"], 2048)?;
		let target = match meta["tool_type"].as_str() {
			Some("plugin") => {
				let suggestion_id = optional_text(&meta["suggestion_id"], 1024)?;
				let remote_plugin_id = optional_text(&meta["remote_plugin_id"], 1024)?;
				let mut app_connector_ids = Vec::new();
				if !meta["app_connector_ids"].is_null() {
					let values = meta["app_connector_ids"]
						.as_array()
						.filter(|values| values.len() <= 128)
						.ok_or("Invalid suggested connector identities")?;
					for value in values {
						let id = text(value, 1024)?;
						if app_connector_ids.contains(&id) {
							return Err("Duplicate suggested connector identity");
						}
						app_connector_ids.push(id);
					}
				}
				McpInstallTarget::Plugin { suggestion_id, remote_plugin_id, app_connector_ids }
			},
			Some("connector") => {
				if ["suggestion_id", "remote_plugin_id", "app_connector_ids"]
					.iter()
					.any(|key| !meta[key].is_null())
				{
					return Err("Unexpected plugin identity on connector suggestion");
				}
				McpInstallTarget::Connector
			},
			_ => return Err("Unsupported suggested integration type"),
		};
		let install_url = optional_text(&meta["install_url"], 16384)?;
		if let Some(raw) = &install_url {
			let url = url::Url::parse(raw).map_err(|_| "Invalid installation link")?;
			if url.scheme() != "https"
				|| url.host_str().is_none()
				|| !url.username().is_empty()
				|| url.password().is_some()
			{
				return Err("Invalid installation link");
			}
		}
		if matches!(target, McpInstallTarget::Connector) && install_url.is_none() {
			return Err("Connector installation link is unavailable");
		}
		Ok(Some(Self { tool_id, tool_name, target, install_url }))
	}

	/// Exact validated native URL; open only after an explicit user action.
	pub fn install_url(&self) -> Option<&str> {
		self.install_url.as_deref()
	}
}

fn text(value: &Value, maximum: usize) -> Result<String, &'static str> {
	value
		.as_str()
		.filter(|text| {
			!text.trim().is_empty() && text.len() <= maximum && !text.chars().any(char::is_control)
		})
		.map(str::to_owned)
		.ok_or("Invalid installation suggestion field")
}
fn optional_text(value: &Value, maximum: usize) -> Result<Option<String>, &'static str> {
	if value.is_null() { Ok(None) } else { text(value, maximum).map(Some) }
}

#[cfg(test)]
mod tests {
	use super::*;
	use serde_json::json;
	fn plugin() -> Value {
		json!({"serverName":"codex_apps","mode":"form","requestedSchema":{"type":"object","properties":{}},"_meta":{"codex_approval_kind":"tool_suggestion","suggest_type":"install","tool_type":"plugin","tool_id":"sample@market","tool_name":"Sample","suggestion_id":"suggestion-1","remote_plugin_id":"plugins~sample","app_connector_ids":["connector-1"]}})
	}
	#[test]
	fn plugin_identity_survives_without_becoming_an_install_attempt() {
		let parsed = McpInstallSuggestion::from_request(&plugin()).unwrap().unwrap();
		assert_eq!(parsed.tool_id, "sample@market");
		assert_eq!(
			parsed.target,
			McpInstallTarget::Plugin {
				suggestion_id: Some("suggestion-1".into()),
				remote_plugin_id: Some("plugins~sample".into()),
				app_connector_ids: vec!["connector-1".into()]
			}
		);
		let mut legacy = plugin();
		legacy["_meta"].as_object_mut().unwrap().remove("suggestion_id");
		assert!(McpInstallSuggestion::from_request(&legacy).unwrap().is_some());
	}
	#[test]
	fn malformed_suggestions_never_become_ordinary_approval_forms() {
		for (pointer, value) in [
			("/serverName", json!("other")),
			("/_meta/tool_type", json!("future")),
			("/_meta/suggest_type", json!("uninstall")),
			("/_meta/tool_id", json!("")),
			("/_meta/suggestion_id", json!({"id":1})),
			("/_meta/app_connector_ids", json!(["same", "same"])),
			("/_meta/install_url", json!("javascript:alert(1)")),
			("/_meta/tool_id", json!("x".repeat(1025))),
		] {
			let mut request = plugin();
			if pointer == "/_meta/install_url" {
				request["_meta"]["install_url"] = value;
			} else {
				*request.pointer_mut(pointer).unwrap() = value;
			}
			assert!(McpInstallSuggestion::from_request(&request).is_err(), "{pointer}");
		}
		assert_eq!(McpInstallSuggestion::from_request(&json!({"mode":"form"})).unwrap(), None);
	}
	#[test]
	fn connector_requires_its_own_link_and_never_inherits_plugin_identity() {
		let mut request = plugin();
		request["_meta"] = json!({"codex_approval_kind":"tool_suggestion","suggest_type":"install","tool_type":"connector","tool_id":"connector-1","tool_name":"Calendar","install_url":"https://chatgpt.com/apps/calendar/connector-1"});
		let parsed = McpInstallSuggestion::from_request(&request).unwrap().unwrap();
		assert_eq!(parsed.target, McpInstallTarget::Connector);
		assert_eq!(parsed.install_url(), Some("https://chatgpt.com/apps/calendar/connector-1"));
		assert!(!format!("{parsed:?}").contains("https"));
		for url in
			["file:///tmp/plugin", "https://user:password@example.com", "http://example.com", ""]
		{
			request["_meta"]["install_url"] = json!(url);
			assert!(McpInstallSuggestion::from_request(&request).is_err());
		}
	}
}
