//! Native hook review and narrowly scoped config edits. Callers own consent and durable receipts.
use super::{AppServerClient, ClientError, HistoryGuard};
use serde::Deserialize;
use serde_json::{Value, json};

/// Native directory inventory and the reviewed active user-config revision.
#[derive(Clone)]
pub struct HookSettingsReview {
	/// Complete native entry, including handler details, trust, warnings and errors.
	pub inventory: Value,
	version: String,
	file: String,
	saved_hooks: Value,
}

/// Explicit action on one reviewed hook; trust never implies enablement.
pub enum HookSettingsChange {
	/// Trust exactly the reviewed content hash.
	Trust,
	/// Change the shared enabled flag without changing trust.
	Enabled(bool),
}

/// Native config acknowledgement; read hooks again to establish effective state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HookSettingsWrite {
	/// The user config was saved.
	Saved,
	/// A higher-priority config layer overrides the saved value.
	Overridden,
}

impl HookSettingsReview {
	/// Native writable file identity, shared by all tasks that use this config.
	pub fn config_file(&self) -> &str {
		&self.file
	}

	/// Exact saved override for one hook, distinct from effective metadata and inherited values.
	pub fn saved_hook(&self, key: &str) -> Option<&Value> {
		self.saved_hooks.get(key)
	}

	/// Native active user-config revision included in every reviewed write.
	pub fn config_version(&self) -> &str {
		&self.version
	}

	/// Construct one edit for an exact reviewed non-managed hook.
	pub fn change(&self, key: &str, change: HookSettingsChange) -> Result<Value, ClientError> {
		let hooks = self.inventory["hooks"].as_array().ok_or(ClientError::InvalidFrame)?;
		let hook = hooks.iter().find(|hook| hook["key"] == key).ok_or(ClientError::InvalidFrame)?;
		if hook["isManaged"] != false
			|| !matches!(hook["trustStatus"].as_str(), Some("untrusted" | "trusted" | "modified"))
		{
			return Err(ClientError::InvalidFrame);
		}
		let (field, value) = match change {
			HookSettingsChange::Trust => ("trusted_hash", hook["currentHash"].clone()),
			HookSettingsChange::Enabled(enabled) => ("enabled", json!(enabled)),
		};
		let params = json!({"edits":[{"keyPath":format!("hooks.state.{}.{field}",json!(key)),"value":value,"mergeStrategy":"replace"}],"expectedVersion":self.version,"reloadUserConfig":true});
		if !is_hook_settings_write(&params) {
			return Err(ClientError::InvalidFrame);
		}
		Ok(params)
	}
}

impl AppServerClient {
	/// Read hook metadata and the base user config version from this native owner.
	pub async fn hook_settings(&self, cwd: &str) -> Result<HookSettingsReview, ClientError> {
		if !std::path::Path::new(cwd).is_absolute() {
			return Err(ClientError::InvalidFrame);
		}
		tokio::time::timeout(std::time::Duration::from_secs(30), async {
			let config =
				self.request("config/read", json!({"cwd":cwd,"includeLayers":true})).await?;
			let layers = config["layers"].as_array().ok_or(ClientError::InvalidFrame)?;
			// config/read returns highest precedence first; native writes select the active user
			// layer.
			let layer = layers
				.iter()
				.find(|row| row["name"]["type"] == "user")
				.ok_or(ClientError::InvalidFrame)?;
			if !layer["disabledReason"].is_null() {
				return Err(ClientError::InvalidFrame);
			}
			let version = bounded_text(&layer["version"], 4096)?.to_owned();
			let file = bounded_text(&layer["name"]["file"], 4096)?.to_owned();
			if !std::path::Path::new(&file).is_absolute() {
				return Err(ClientError::InvalidFrame);
			}
			let saved_hooks = layer["config"]["hooks"]["state"].clone();

			let response = self.request("hooks/list", json!({"cwds":[cwd]})).await?;
			let entries = response["data"].as_array().ok_or(ClientError::InvalidFrame)?;
			if entries.len() != 1 || entries[0]["cwd"] != cwd {
				return Err(ClientError::InvalidFrame);
			}
			let inventory = entries[0].clone();
			validate_inventory(&inventory)?;
			Ok(HookSettingsReview { inventory, version, file, saved_hooks })
		})
		.await
		.map_err(|_| ClientError::Io)?
	}

	/// Submit one version-checked edit while the owner's source guard remains current.
	/// No replay is permitted after an uncertain outcome; saved does not mean effective.
	pub async fn write_hook_settings(
		&self,
		params: Value,
		guard: HistoryGuard,
	) -> Result<HookSettingsWrite, ClientError> {
		if !is_hook_settings_write(&params) {
			return Err(ClientError::InvalidFrame);
		}
		let response = tokio::time::timeout(
			std::time::Duration::from_secs(30),
			self.request_with_history("config/batchWrite", params, guard),
		)
		.await
		.map_err(|_| ClientError::Io)??;
		bounded_text(&response["version"], 4096)?;
		if !response["filePath"].as_str().is_some_and(|p| std::path::Path::new(p).is_absolute()) {
			return Err(ClientError::InvalidFrame);
		}
		match response["status"].as_str() {
			Some("ok") => Ok(HookSettingsWrite::Saved),
			Some("okOverridden") => Ok(HookSettingsWrite::Overridden),
			_ => Err(ClientError::InvalidFrame),
		}
	}
}

fn bounded_text(value: &Value, limit: usize) -> Result<&str, ClientError> {
	value
		.as_str()
		.filter(|s| !s.is_empty() && s.len() <= limit && !s.chars().any(char::is_control))
		.ok_or(ClientError::InvalidFrame)
}

fn validate_inventory(entry: &Value) -> Result<(), ClientError> {
	if entry.to_string().len() > 256 * 1024 {
		return Err(ClientError::CapacityExceeded);
	}
	if !entry["warnings"].is_array() || !entry["errors"].is_array() {
		return Err(ClientError::InvalidFrame);
	}
	let hooks = entry["hooks"].as_array().ok_or(ClientError::InvalidFrame)?;
	let mut keys = std::collections::HashSet::new();
	for hook in hooks {
		if !keys.insert(bounded_text(&hook["key"], 4096)?)
			|| !hook["enabled"].is_boolean()
			|| !hook["isManaged"].is_boolean()
		{
			return Err(ClientError::InvalidFrame);
		}
		bounded_text(&hook["currentHash"], 4096)?;
		bounded_text(&hook["trustStatus"], 128)?;
		bounded_text(&hook["handlerType"], 128)?;
		bounded_text(&hook["eventName"], 128)?;
		if !hook["sourcePath"].as_str().is_some_and(|p| std::path::Path::new(p).is_absolute()) {
			return Err(ClientError::InvalidFrame);
		}
	}
	Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WriteParams {
	edits: Vec<Edit>,
	expected_version: String,
	reload_user_config: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Edit {
	key_path: String,
	value: Value,
	merge_strategy: String,
}

/// Admit only one hook enabled/trusted-hash edit to the active native user config.
pub fn is_hook_settings_write(params: &Value) -> bool {
	let Ok(write) = serde_json::from_value::<WriteParams>(params.clone()) else { return false };
	if write.edits.len() != 1
		|| !write.reload_user_config
		|| bounded_text(&json!(write.expected_version), 4096).is_err()
	{
		return false;
	}
	let edit = &write.edits[0];
	let Some((quoted, field)) =
		edit.key_path.strip_prefix("hooks.state.").and_then(|s| s.rsplit_once('.'))
	else {
		return false;
	};
	let Ok(key) = serde_json::from_str::<String>(quoted) else { return false };
	if bounded_text(&json!(key), 4096).is_err()
		|| serde_json::to_string(&key).ok().as_deref() != Some(quoted)
		|| edit.merge_strategy != "replace"
	{
		return false;
	}
	match field {
		"enabled" => edit.value.is_boolean(),
		"trusted_hash" => bounded_text(&edit.value, 4096).is_ok(),
		_ => false,
	}
}

#[cfg(test)]
#[path = "hooks_tests.rs"]
mod tests;
