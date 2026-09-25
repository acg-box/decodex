//! Native voice preferences. Saving affects the next call and never restarts audio.
use super::{AppServerClient, ClientError, Outbound};
use serde_json::{Value, json};
use std::{path::Path, time::Duration};
use tokio::sync::mpsc;

/// Reviewed native configuration, including the version needed for a conditional write.
#[derive(Clone)]
pub struct NativeVoiceSettings {
	connection: mpsc::Sender<Outbound>,
	cwd: String,
	file: String,
	version: String,
	/// V3 uses the native V1 catalog.
	pub voices: Vec<String>,
	/// Effective project voice, or an unknown voice supplied by a newer server.
	pub effective: Option<String>,
	/// The user preference can be overridden by project or managed configuration.
	pub preference: Option<String>,
}

impl NativeVoiceSettings {
	/// Bind a displayed selection to its native directory, version and effective settings.
	pub fn fingerprint(&self) -> String {
		use sha2::{Digest as _, Sha256};
		let value = json!([
			self.cwd,
			self.file,
			self.version,
			self.voices,
			self.effective,
			self.preference
		]);
		Sha256::digest(value.to_string().as_bytes())
			.iter()
			.map(|byte| format!("{byte:02x}"))
			.collect()
	}
}

impl AppServerClient {
	/// Read only voice-related fields from the effective and writable native layers.
	pub async fn realtime_voice_settings(
		&self,
		cwd: &str,
	) -> Result<NativeVoiceSettings, ClientError> {
		if !Path::new(cwd).is_absolute() {
			return Err(ClientError::InvalidFrame);
		}
		tokio::time::timeout(Duration::from_secs(15), async {
			let config =
				self.request("config/read", json!({"cwd":cwd,"includeLayers":true})).await?;
			let layers = config["layers"].as_array().ok_or(ClientError::InvalidFrame)?;
			let user = layers
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
			let (voices, default) = self.realtime_voice_catalog().await;
			let effective =
				optional_voice(&config["config"]["realtime"]["voice"])?.or(Some(default));
			let preference = optional_voice(&user["config"]["realtime"]["voice"])?;
			Ok(NativeVoiceSettings {
				connection: self.outbound.clone(),
				cwd: cwd.into(),
				file,
				version,
				voices,
				effective,
				preference,
			})
		})
		.await
		.map_err(|_| ClientError::Io)?
	}

	/// Save one reviewed voice with native compare-and-swap, then read effective settings.
	/// An error after dispatch is unconfirmed; this method never retries a write.
	pub async fn write_realtime_voice(
		&self,
		observed: &NativeVoiceSettings,
		voice: &str,
	) -> Result<NativeVoiceSettings, ClientError> {
		if !self.outbound.same_channel(&observed.connection)
			|| !observed.voices.iter().any(|v| v == voice)
		{
			return Err(ClientError::InvalidFrame);
		}
		let params = json!({"filePath":observed.file,"expectedVersion":observed.version,
			"reloadUserConfig":false,"edits":[{"keyPath":"realtime.voice","value":voice,"mergeStrategy":"replace"}]});
		let receipt = tokio::time::timeout(
			Duration::from_secs(15),
			self.request("config/batchWrite", params),
		)
		.await
		.map_err(|_| ClientError::Io)??;
		if !matches!(receipt["status"].as_str(), Some("ok" | "okOverridden"))
			|| receipt["filePath"] != observed.file
			|| string(&receipt["version"]).is_err()
		{
			return Err(ClientError::InvalidFrame);
		}
		self.realtime_voice_settings(&observed.cwd).await
	}

	pub(super) async fn realtime_voice_catalog(&self) -> (Vec<String>, String) {
		if let Ok(value) = self.request("thread/realtime/listVoices", json!({})).await
			&& let Some(catalog) = value["voices"]["v1"].as_array()
			&& !catalog.is_empty()
			&& catalog.len() <= 64
			&& let Ok(voices) = catalog.iter().map(string).collect::<Result<Vec<_>, _>>()
			&& voices.iter().all(|voice| super::realtime_settings::known_voice(voice))
			&& let Ok(default) = string(&value["voices"]["defaultV1"])
			&& voices.contains(&default)
		{
			return (voices, default);
		}
		(
			["juniper", "maple", "spruce", "ember", "vale", "breeze", "arbor", "sol", "cove"]
				.into_iter()
				.map(str::to_owned)
				.collect(),
			"cove".into(),
		)
	}
}

fn string(value: &Value) -> Result<String, ClientError> {
	value
		.as_str()
		.filter(|v| !v.is_empty() && v.len() <= 4096 && !v.chars().any(char::is_control))
		.map(str::to_owned)
		.ok_or(ClientError::InvalidFrame)
}

fn optional_voice(value: &Value) -> Result<Option<String>, ClientError> {
	if value.is_null() { Ok(None) } else { string(value).map(Some) }
}

/// Accept only a conditional voice edit through the retained native transport.
pub fn is_realtime_voice_write(params: &Value) -> bool {
	params.as_object().is_some_and(|v| v.len() == 4)
		&& params["reloadUserConfig"] == false
		&& params["filePath"].as_str().is_some_and(|p| Path::new(p).is_absolute())
		&& string(&params["expectedVersion"]).is_ok()
		&& params["edits"].as_array().is_some_and(|edits| {
			edits.len() == 1
				&& edits[0].as_object().is_some_and(|v| v.len() == 3)
				&& edits[0]["keyPath"] == "realtime.voice"
				&& edits[0]["mergeStrategy"] == "replace"
				&& string(&edits[0]["value"]).is_ok()
		})
}

#[cfg(test)]
#[path = "realtime_preferences_tests.rs"]
mod tests;
