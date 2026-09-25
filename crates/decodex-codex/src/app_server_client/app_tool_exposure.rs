//! Connector-level exposure preferences. Native Codex owns tool filtering and approvals.
use super::{
	AppServerClient, ClientError, HistoryGuard, Outbound,
	app_link_settings::{quoted_key, required_string, take_quoted_key, valid_identity},
};
use serde_json::{Value, json};
use std::{path::Path, time::Duration};
use tokio::sync::mpsc;

/// A reviewed connector preference, separate from connected-account approval settings.
#[derive(Clone)]
pub struct AppToolExposureSettings {
	connection: mpsc::Sender<Outbound>,
	cwd: String,
	app: String,
	file: String,
	version: String,
	/// Layered connector omissions. This is not the final set of available tools.
	pub effective: Option<Vec<String>>,
	/// Writable user-layer omissions. None inherits; an empty list explicitly clears omissions.
	pub preference: Option<Vec<String>>,
}

impl std::fmt::Debug for AppToolExposureSettings {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str("AppToolExposureSettings([private native connector scope])")
	}
}

impl AppToolExposureSettings {
	/// Bind the reviewed preference to its native scope, version and effective configuration.
	pub fn fingerprint(&self) -> String {
		use sha2::{Digest as _, Sha256};
		let facts =
			json!([self.cwd, self.app, self.file, self.version, self.effective, self.preference]);
		Sha256::digest(facts.to_string().as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
	}
}

/// A native write receipt with fresh configuration readback, not proof of live tool exposure.
#[derive(Debug)]
pub struct AppToolExposureWrite {
	/// A higher configuration layer overrides this saved preference.
	pub overridden: bool,
	/// Readback from the same native connection and connector scope.
	pub settings: AppToolExposureSettings,
}

impl AppServerClient {
	/// Read one connector's effective and writable exposure preference.
	/// The caller obtains the connector identity from the current native App inventory.
	pub async fn app_tool_exposure(
		&self,
		cwd: &str,
		app: &str,
	) -> Result<AppToolExposureSettings, ClientError> {
		if !Path::new(cwd).is_absolute() || !valid_identity(app) || app == "_default" {
			return Err(ClientError::InvalidFrame);
		}
		let value = tokio::time::timeout(
			Duration::from_secs(15),
			self.request("config/read", json!({"cwd":cwd,"includeLayers":true})),
		)
		.await
		.map_err(|_| ClientError::Io)??;
		let user = value["layers"]
			.as_array()
			.and_then(|layers| layers.iter().find(|layer| layer["name"]["type"] == "user"))
			.ok_or(ClientError::InvalidFrame)?;
		if !user["disabledReason"].is_null() {
			return Err(ClientError::InvalidFrame);
		}
		let file = required_string(&user["name"]["file"])?;
		if !Path::new(&file).is_absolute() {
			return Err(ClientError::InvalidFrame);
		}
		Ok(AppToolExposureSettings {
			connection: self.outbound.clone(),
			cwd: cwd.into(),
			app: app.into(),
			file,
			version: required_string(&user["version"])?,
			effective: omissions(&value["config"], app)?,
			preference: omissions(&user["config"], app)?,
		})
	}

	/// Save one connector leaf with native version checking and a live source guard.
	/// The caller reserves the operation durably before calling. Errors after dispatch
	/// leave the effect uncertain; this method never retries or starts a model turn.
	pub async fn write_app_tool_exposure(
		&self,
		observed: &AppToolExposureSettings,
		preference: Option<Vec<String>>,
		guard: HistoryGuard,
	) -> Result<AppToolExposureWrite, ClientError> {
		if !self.outbound.same_channel(&observed.connection)
			|| !valid_preference(preference.as_deref())
		{
			return Err(ClientError::InvalidFrame);
		}
		let params = json!({"filePath":observed.file,"expectedVersion":observed.version,"reloadUserConfig":true,
			"edits":[{"keyPath":format!("apps.{}.omit_tools_from",quoted_key(&observed.app)),"value":preference,"mergeStrategy":"replace"}]});
		let receipt = tokio::time::timeout(
			Duration::from_secs(15),
			self.request_with_history("config/batchWrite", params, guard),
		)
		.await
		.map_err(|_| ClientError::Io)??;
		let overridden = match receipt["status"].as_str() {
			Some("ok") => false,
			Some("okOverridden") => true,
			_ => return Err(ClientError::InvalidFrame),
		};
		if receipt["filePath"] != observed.file || required_string(&receipt["version"]).is_err() {
			return Err(ClientError::InvalidFrame);
		}
		let settings = self.app_tool_exposure(&observed.cwd, &observed.app).await?;
		Ok(AppToolExposureWrite { overridden, settings })
	}
}

fn omissions(config: &Value, app: &str) -> Result<Option<Vec<String>>, ClientError> {
	if !config.is_object() {
		return Err(ClientError::InvalidFrame);
	}
	let mut scope = config;
	for key in ["apps", app] {
		match scope.get(key) {
			None | Some(Value::Null) => return Ok(None),
			Some(value) if value.is_object() => scope = value,
			_ => return Err(ClientError::InvalidFrame),
		}
	}
	match scope.get("omit_tools_from") {
		None | Some(Value::Null) => Ok(None),
		Some(Value::Array(values)) if values.len() <= 16 => {
			// Preserve future values for display. Never silently discard an unknown restriction.
			let values = values.iter().map(required_string).collect::<Result<Vec<_>, _>>()?;
			if values.iter().collect::<std::collections::HashSet<_>>().len() != values.len() {
				return Err(ClientError::InvalidFrame);
			}
			Ok(Some(values))
		},
		_ => Err(ClientError::InvalidFrame),
	}
}

fn valid_preference(values: Option<&[String]>) -> bool {
	values.is_none_or(|values| {
		values.len() <= 3
			&& values.iter().all(|v| matches!(v.as_str(), "code_mode" | "deferred" | "direct"))
			&& values.iter().collect::<std::collections::HashSet<_>>().len() == values.len()
	})
}

/// Admit only a versioned connector exposure edit through the retained process bridge.
pub fn is_app_tool_exposure_write(params: &Value) -> bool {
	if !params.as_object().is_some_and(|p| p.len() == 4)
		|| params["reloadUserConfig"] != true
		|| !params["filePath"].as_str().is_some_and(|p| Path::new(p).is_absolute())
		|| required_string(&params["expectedVersion"]).is_err()
	{
		return false;
	}
	let Some(edits) = params["edits"].as_array().filter(|e| e.len() == 1) else {
		return false;
	};
	let edit = &edits[0];
	if !edit.as_object().is_some_and(|e| e.len() == 3) || edit["mergeStrategy"] != "replace" {
		return false;
	}
	let Some(path) = edit["keyPath"].as_str().and_then(|p| p.strip_prefix("apps.")) else {
		return false;
	};
	let Some((app, field)) = take_quoted_key(path) else {
		return false;
	};
	if !valid_identity(&app) || app == "_default" || field != ".omit_tools_from" {
		return false;
	}
	match edit.get("value") {
		Some(Value::Null) => true,
		Some(Value::Array(values)) => values
			.iter()
			.map(|v| v.as_str().map(str::to_owned))
			.collect::<Option<Vec<_>>>()
			.is_some_and(|v| valid_preference(Some(&v))),
		_ => false,
	}
}

#[cfg(test)]
#[path = "app_tool_exposure_tests.rs"]
mod tests;
