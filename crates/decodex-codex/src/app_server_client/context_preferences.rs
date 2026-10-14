//! Read and edit two native context defaults without overriding loaded conversations.
use std::{path::Path, time::Duration};

use serde_json::Value;
use sha2::{Digest as _, Sha256};
use tokio::{sync::mpsc::Sender, time};

use crate::app_server_client::{AppServerClient, ClientError, Outbound};

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
				self.preference
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

		if !self.outbound.same_channel(&observed.connection) {
			return Err(ClientError::InvalidFrame);
		}

		let key = if index == 0 && observed.user_features[feature].is_object() {
			"features.async_compaction.enabled".to_owned()
		} else {
			format!("features.{feature}")
		};
		let receipt = time::timeout(
			Duration::from_secs(15),
			self.request(
				"config/batchWrite",
				serde_json::json!({
					"filePath":observed.file,"expectedVersion":observed.version,"reloadUserConfig":false,
					"edits":[{"keyPath":key,"value":enabled,"mergeStrategy":"replace"}]
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

/// Permit only one versioned boolean edit for the two exposed context preferences.
pub fn is_context_preference_write(params: &Value) -> bool {
	params.as_object().is_some_and(|v| v.len() == 4)
		&& params["reloadUserConfig"] == false
		&& params["filePath"].as_str().is_some_and(|p| Path::new(p).is_absolute())
		&& string(&params["expectedVersion"]).is_ok()
		&& params["edits"].as_array().is_some_and(|edits| {
			edits.len() == 1
				&& edits[0].as_object().is_some_and(|v| v.len() == 3)
				&& matches!(
					edits[0]["keyPath"].as_str(),
					Some(
						"features.async_compaction"
							| "features.async_compaction.enabled"
							| "features.retain_client_tool_outputs"
					)
				)
				&& edits[0]["mergeStrategy"] == "replace"
				&& edits[0]["value"].is_boolean()
		})
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
