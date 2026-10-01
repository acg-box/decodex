//! Native web-search defaults. Existing loaded threads retain their search configuration.
use std::{path::Path, time::Duration};

use serde_json::{Value, json};
use tokio::{sync::mpsc::Sender, time};

use crate::app_server_client::{AppServerClient, ClientError, Outbound};

const MODES: [&str; 4] = ["disabled", "cached", "indexed", "live"];

/// A native user-layer selection reviewed in one project's effective configuration.
#[derive(Clone)]
pub struct NativeSearchSettings {
	connection: Sender<Outbound>,
	cwd: String,
	file: String,
	version: String,
	/// Known modes allowed by the current native requirements.
	pub modes: Vec<String>,
	/// Effective project default; this does not describe an already loaded conversation.
	pub effective: Option<String>,
	/// User-layer preference, which a project or managed layer can override.
	pub preference: Option<String>,
}
impl NativeSearchSettings {
	/// Bind the review to the native file version, choices and effective project default.
	pub fn fingerprint(&self) -> String {
		use sha2::{Digest as _, Sha256};

		Sha256::digest(
			json!([self.cwd, self.file, self.version, self.modes, self.effective, self.preference])
				.to_string()
				.as_bytes(),
		)
		.iter()
		.map(|byte| format!("{byte:02x}"))
		.collect()
	}
}

impl AppServerClient {
	/// Read search defaults and requirements without resuming or changing a conversation.
	pub async fn search_settings(&self, cwd: &str) -> Result<NativeSearchSettings, ClientError> {
		if !Path::new(cwd).is_absolute() {
			return Err(ClientError::InvalidFrame);
		}

		time::timeout(Duration::from_secs(15), async {
			let config =
				self.request("config/read", json!({"cwd":cwd,"includeLayers":true})).await?;
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

			let version = string(&user["version"])?;
			let requirements = self.request("configRequirements/read", json!({})).await?;
			let requirements = requirements.get("requirements").ok_or(ClientError::InvalidFrame)?;
			let allowed = &requirements["allowedWebSearchModes"];
			let modes = match allowed {
				Value::Null => MODES.iter().map(|s| (*s).to_owned()).collect(),
				Value::Array(modes) => {
					let modes = modes.iter().map(string).collect::<Result<Vec<_>, _>>()?;

					MODES
						.iter()
						.filter(|m| modes.iter().any(|v| v == *m))
						.map(|m| (*m).to_owned())
						.collect()
				},
				_ => return Err(ClientError::InvalidFrame),
			};

			Ok(NativeSearchSettings {
				connection: self.outbound.clone(),
				cwd: cwd.into(),
				file,
				version,
				modes,
				effective: optional(&config["config"]["web_search"])?,
				preference: optional(&user["config"]["web_search"])?,
			})
		})
		.await
		.map_err(|_| ClientError::Io)?
	}

	/// Save one explicit default with native compare-and-swap and verify the user-layer value.
	/// Do not restart, resume, or fork existing conversations to apply this change.
	pub async fn write_search_mode(
		&self,
		observed: &NativeSearchSettings,
		mode: &str,
	) -> Result<NativeSearchSettings, ClientError> {
		if !self.outbound.same_channel(&observed.connection)
			|| !observed.modes.iter().any(|m| m == mode)
		{
			return Err(ClientError::InvalidFrame);
		}

		let receipt = time::timeout(
			Duration::from_secs(15),
			self.request(
				"config/batchWrite",
				json!({
					"filePath":observed.file,"expectedVersion":observed.version,"reloadUserConfig":false,
					"edits":[{"keyPath":"web_search","value":mode,"mergeStrategy":"replace"}]
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

		self.search_settings(&observed.cwd).await
	}
}

/// Permit only one versioned search-mode preference edit on the retained native transport.
pub fn is_search_mode_write(params: &Value) -> bool {
	params.as_object().is_some_and(|v| v.len() == 4)
		&& params["reloadUserConfig"] == false
		&& params["filePath"].as_str().is_some_and(|p| Path::new(p).is_absolute())
		&& string(&params["expectedVersion"]).is_ok()
		&& params["edits"].as_array().is_some_and(|edits| {
			edits.len() == 1
				&& edits[0].as_object().is_some_and(|v| v.len() == 3)
				&& edits[0]["keyPath"] == "web_search"
				&& edits[0]["mergeStrategy"] == "replace"
				&& edits[0]["value"].as_str().is_some_and(|mode| MODES.contains(&mode))
		})
}

fn string(value: &Value) -> Result<String, ClientError> {
	value
		.as_str()
		.filter(|v| !v.is_empty() && v.len() <= 4_096 && !v.chars().any(char::is_control))
		.map(str::to_owned)
		.ok_or(ClientError::InvalidFrame)
}

fn optional(value: &Value) -> Result<Option<String>, ClientError> {
	if value.is_null() { Ok(None) } else { string(value).map(Some) }
}

#[cfg(test)]
mod tests {
	use crate::app_server_client::search_preferences::{self};
	#[test]
	fn search_write_permits_only_a_reviewed_default_without_reload_or_other_edits() {
		let valid = search_preferences::json!({"filePath":"/home/config.toml","expectedVersion":"v1","reloadUserConfig":false,"edits":[{"keyPath":"web_search","value":"indexed","mergeStrategy":"replace"}]});

		assert!(search_preferences::is_search_mode_write(&valid));

		for field in ["expectedVersion", "reloadUserConfig"] {
			let mut missing = valid.clone();

			missing.as_object_mut().unwrap().remove(field);

			assert!(!search_preferences::is_search_mode_write(&missing));
		}

		let mut changed = valid.clone();

		changed["reloadUserConfig"] = search_preferences::json!(true);

		assert!(!search_preferences::is_search_mode_write(&changed));

		let mut changed = valid.clone();

		changed["edits"][0]["keyPath"] =
			search_preferences::json!("features.standalone_web_search");

		assert!(!search_preferences::is_search_mode_write(&changed));

		let mut changed = valid.clone();

		changed["edits"][0]["value"] = search_preferences::json!("future-mode");

		assert!(!search_preferences::is_search_mode_write(&changed));
	}
}
