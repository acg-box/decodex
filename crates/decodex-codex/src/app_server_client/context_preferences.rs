//! Read and edit selected native defaults without overriding loaded conversations.
use std::{path::Path, time::Duration};

use serde_json::Value;
use sha2::{Digest as _, Sha256};
use tokio::{sync::mpsc::Sender, time};

use crate::app_server_client::{AppServerClient, ClientError, Outbound, app_link_settings};

/// Native context feature keys, in presentation order.
pub const CONTEXT_FEATURES: [&str; 2] = ["async_compaction", "retain_client_tool_outputs"];

/// Native user settings and effective project configuration from one observation.
#[derive(Clone)]
pub struct NativeContextSettings {
	connection: Sender<Outbound>,
	cwd: String,
	file: String,
	version: String,
	user_features: Value,
	/// Effective configured defaults. Native runtime eligibility still applies.
	pub effective: [bool; 2],
	/// Explicit user values; absent values use the native default.
	pub preference: [Option<bool>; 2],
	/// Startup defaults for existing user-configured MCP servers, without their credentials.
	pub mcp_startup: Vec<(String, bool, Option<bool>)>,
}
impl NativeContextSettings {
	/// Bind a settings review to the native file version and effective configuration.
	pub fn fingerprint(&self) -> String {
		Sha256::digest(
			serde_json::json!([
				self.cwd,
				self.file,
				self.version,
				self.user_features,
				self.effective,
				self.preference,
				self.mcp_startup
			])
			.to_string()
			.as_bytes(),
		)
		.iter()
		.map(|b| format!("{b:02x}"))
		.collect()
	}
}

impl AppServerClient {
	/// Read the native user layer and effective project context defaults.
	pub async fn context_settings(&self, cwd: &str) -> Result<NativeContextSettings, ClientError> {
		if !Path::new(cwd).is_absolute() {
			return Err(ClientError::InvalidFrame);
		}

		let config = time::timeout(
			Duration::from_secs(15),
			self.request("config/read", serde_json::json!({"cwd":cwd,"includeLayers":true})),
		)
		.await
		.map_err(|_| ClientError::Io)??;
		let user = config["layers"]
			.as_array()
			.ok_or(ClientError::InvalidFrame)?
			.iter()
			.find(|layer| layer["name"]["type"] == "user")
			.ok_or(ClientError::InvalidFrame)?;

		if !user["disabledReason"].is_null() || !config["config"].is_object() {
			return Err(ClientError::InvalidFrame);
		}

		let file = string(&user["name"]["file"])?;

		if !Path::new(&file).is_absolute() {
			return Err(ClientError::InvalidFrame);
		}

		let user_features = user["config"]["features"].clone();
		let mut effective = [false; 2];
		let mut preference = [None; 2];

		for (index, key) in CONTEXT_FEATURES.iter().enumerate() {
			effective[index] = enabled(&config["config"]["features"][key], index)?.unwrap_or(false);
			preference[index] = enabled(&user_features[key], index)?;
		}

		Ok(NativeContextSettings {
			connection: self.outbound.clone(),
			cwd: cwd.into(),
			file,
			version: string(&user["version"])?,
			user_features,
			effective,
			preference,
			mcp_startup: mcp_startup(
				&user["config"]["mcp_servers"],
				&config["config"]["mcp_servers"],
			),
		})
	}

	/// Save one explicit on/off value with native compare-and-swap, then read it back.
	/// Preserve async thresholds and respect higher-priority configuration layers.
	pub async fn write_context_preference(
		&self,
		observed: &NativeContextSettings,
		feature: &str,
		enabled: bool,
	) -> Result<NativeContextSettings, ClientError> {
		let index = CONTEXT_FEATURES
			.iter()
			.position(|key| *key == feature)
			.ok_or(ClientError::InvalidFrame)?;

		let key = if index == 0 && observed.user_features[feature].is_object() {
			"features.async_compaction.enabled".to_owned()
		} else {
			format!("features.{feature}")
		};
		self.write_native_default(observed, key, serde_json::json!(enabled)).await
	}

	/// Save catalog readiness for one existing user-configured server, then read it back.
	pub async fn write_mcp_startup_preference(
		&self,
		observed: &NativeContextSettings,
		server: &str,
		enabled: bool,
	) -> Result<NativeContextSettings, ClientError> {
		if !observed.mcp_startup.iter().any(|(name, _, _)| name == server) {
			return Err(ClientError::InvalidFrame);
		}

		self.write_native_default(
			observed,
			format!("mcp_servers.{}.startup_readiness", app_link_settings::quoted_key(server)),
			serde_json::json!(if enabled { "catalog" } else { "connection" }),
		)
		.await
	}

	async fn write_native_default(
		&self,
		observed: &NativeContextSettings,
		key: String,
		value: Value,
	) -> Result<NativeContextSettings, ClientError> {
		if !self.outbound.same_channel(&observed.connection) {
			return Err(ClientError::InvalidFrame);
		}

		let receipt = time::timeout(
			Duration::from_secs(15),
			self.request(
				"config/batchWrite",
				serde_json::json!({
					"filePath":observed.file,"expectedVersion":observed.version,"reloadUserConfig":false,
					"edits":[{"keyPath":key,"value":value,"mergeStrategy":"replace"}]
				}),
			),
		)
		.await
		.map_err(|_| ClientError::Io)??;

		if !matches!(receipt["status"].as_str(), Some("ok" | "okOverridden"))
			|| receipt["filePath"] != observed.file
			|| string(&receipt["version"]).is_err()
		{
			return Err(ClientError::InvalidFrame);
		}

		self.context_settings(&observed.cwd).await
	}
}

/// Permit one versioned edit for an exposed context or MCP startup preference.
pub fn is_context_preference_write(params: &Value) -> bool {
	params.as_object().is_some_and(|v| v.len() == 4)
		&& params["reloadUserConfig"] == false
		&& params["filePath"].as_str().is_some_and(|p| Path::new(p).is_absolute())
		&& string(&params["expectedVersion"]).is_ok()
		&& params["edits"].as_array().is_some_and(|edits| {
			edits.len() == 1
				&& edits[0].as_object().is_some_and(|v| v.len() == 3)
				&& edits[0]["mergeStrategy"] == "replace"
				&& valid_preference_edit(&edits[0])
		})
}

fn valid_preference_edit(edit: &Value) -> bool {
	let Some(path) = edit["keyPath"].as_str() else { return false };

	if matches!(
		path,
		"features.async_compaction"
			| "features.async_compaction.enabled"
			| "features.retain_client_tool_outputs"
	) {
		return edit["value"].is_boolean();
	}

	path.strip_prefix("mcp_servers.").and_then(app_link_settings::take_quoted_key).is_some_and(
		|(server, field)| {
			app_link_settings::valid_identity(&server) && field == ".startup_readiness"
		},
	) && matches!(edit["value"].as_str(), Some("catalog" | "connection"))
}

fn mcp_startup(user: &Value, effective: &Value) -> Vec<(String, bool, Option<bool>)> {
	let Some(servers) = user.as_object() else { return Vec::new() };

	servers
		.iter()
		.filter_map(|(name, entry)| {
			if !app_link_settings::valid_identity(name) || !entry.is_object() {
				return None;
			}
			let preference = catalog_readiness(&entry["startup_readiness"])?;
			let effective = catalog_readiness(&effective[name]["startup_readiness"])?;

			Some((name.clone(), effective.unwrap_or(false), preference))
		})
		.collect()
}

fn catalog_readiness(value: &Value) -> Option<Option<bool>> {
	match value {
		Value::Null => Some(None),
		Value::String(value) if value == "catalog" => Some(Some(true)),
		Value::String(value) if value == "connection" => Some(Some(false)),
		_ => None,
	}
}

fn enabled(value: &Value, index: usize) -> Result<Option<bool>, ClientError> {
	match value {
		Value::Null => Ok(None),
		Value::Bool(value) => Ok(Some(*value)),
		Value::Object(config) if index == 0 => match config.get("enabled") {
			None | Some(Value::Null) => Ok(None),
			Some(Value::Bool(value)) => Ok(Some(*value)),
			_ => Err(ClientError::InvalidFrame),
		},
		_ => Err(ClientError::InvalidFrame),
	}
}

fn string(value: &Value) -> Result<String, ClientError> {
	value
		.as_str()
		.filter(|v| !v.is_empty() && v.len() <= 4_096 && !v.chars().any(char::is_control))
		.map(str::to_owned)
		.ok_or(ClientError::InvalidFrame)
}

#[cfg(test)]
#[path = "context_preferences_tests.rs"]
mod tests;
