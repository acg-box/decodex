//! Read native connection settings to reconcile historical configuration receipts.
use std::{
	fmt::{Debug, Formatter},
	path::Path,
	time::Duration,
};

use serde_json::{Value, json};
use tokio::time;

use crate::app_server_client::{AppServerClient, ClientError};

/// Narrow readback. No credentials or unrelated native configuration leave this adapter.
#[derive(Clone)]
pub struct AppLinkSettings {
	file: String,
	version: String,
	/// Account approval mode in the effective repository configuration, before policy precedence.
	pub effective_mode: Option<String>,
	/// Account reviewer in the effective repository configuration, before policy precedence.
	pub effective_reviewer: Option<String>,
	/// Account approval mode in the writable user layer.
	pub user_mode: Option<String>,
	/// Account reviewer in the writable user layer.
	pub user_reviewer: Option<String>,
}
impl AppLinkSettings {
	/// Native writable file identity, shared by tasks and account-bound requests using it.
	pub fn config_file(&self) -> &str {
		&self.file
	}

	/// Reviewed version used for native conflict detection.
	pub fn config_version(&self) -> &str {
		&self.version
	}
}

impl Debug for AppLinkSettings {
	fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
		f.write_str("AppLinkSettings([private native account scope])")
	}
}

impl AppServerClient {
	/// Read the exact account in effective and writable native configuration layers.
	/// The caller must obtain connector and link identities from native account metadata.
	pub async fn app_link_settings(
		&self,
		cwd: &str,
		app: &str,
		link: &str,
	) -> Result<AppLinkSettings, ClientError> {
		if !Path::new(cwd).is_absolute() || !valid_identity(app) || !valid_identity(link) {
			return Err(ClientError::InvalidFrame);
		}

		let response = self.read_app_config(cwd).await?;

		self.project_app_link(app, link, &response)
	}

	async fn read_app_config(&self, cwd: &str) -> Result<Value, ClientError> {
		if !Path::new(cwd).is_absolute() {
			return Err(ClientError::InvalidFrame);
		}

		time::timeout(
			Duration::from_secs(30),
			self.request("config/read", json!({"cwd":cwd,"includeLayers":true})),
		)
		.await
		.map_err(|_| ClientError::Io)?
	}

	fn project_app_link(
		&self,
		app: &str,
		link: &str,
		response: &Value,
	) -> Result<AppLinkSettings, ClientError> {
		if !valid_identity(app) || !valid_identity(link) {
			return Err(ClientError::InvalidFrame);
		}

		let (user, file, version) = writable_layer(response)?;
		let effective = account_config(&response["config"], app, link)?;
		let writable = account_config(&user["config"], app, link)?;

		Ok(AppLinkSettings {
			file,
			version,
			effective_mode: setting(effective, "default_tools_approval_mode")?,
			effective_reviewer: setting(effective, "approvals_reviewer")?,
			user_mode: setting(writable, "default_tools_approval_mode")?,
			user_reviewer: setting(writable, "approvals_reviewer")?,
		})
	}
}

pub(super) fn valid_identity(value: &str) -> bool {
	!value.is_empty() && value.len() <= 4_096 && !value.chars().any(char::is_control)
}

pub(super) fn quoted_key(value: &str) -> String {
	format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

pub(super) fn take_quoted_key(path: &str) -> Option<(String, &str)> {
	let mut decoded = String::new();
	let mut chars = path.strip_prefix('"')?.char_indices();

	while let Some((index, ch)) = chars.next() {
		match ch {
			'"' => return Some((decoded, &path[index + 2..])),
			'\\' => match chars.next()?.1 {
				escaped @ ('\\' | '"') => decoded.push(escaped),
				_ => return None,
			},
			_ => decoded.push(ch),
		}
	}

	None
}

pub(super) fn required_string(value: &Value) -> Result<String, ClientError> {
	value.as_str().filter(|s| valid_identity(s)).map(str::to_owned).ok_or(ClientError::InvalidFrame)
}

fn writable_layer(response: &Value) -> Result<(&Value, String, String), ClientError> {
	let layers = response["layers"].as_array().ok_or(ClientError::InvalidFrame)?;
	// Native config/read orders layers high to low; the first user layer is active.
	let user = layers
		.iter()
		.find(|layer| layer["name"]["type"] == "user")
		.ok_or(ClientError::InvalidFrame)?;

	if !user["disabledReason"].is_null() || !user["config"].is_object() {
		return Err(ClientError::InvalidFrame);
	}

	let file = required_string(&user["name"]["file"])?;

	if !Path::new(&file).is_absolute() {
		return Err(ClientError::InvalidFrame);
	}

	Ok((user, file, required_string(&user["version"])?))
}

fn account_config<'a>(
	config: &'a Value,
	app: &str,
	link: &str,
) -> Result<Option<&'a Value>, ClientError> {
	if !config.is_object() {
		return Err(ClientError::InvalidFrame);
	}

	let mut current = config;

	for key in ["apps", app, "links", link] {
		match current.get(key) {
			None | Some(Value::Null) => return Ok(None),
			Some(value) if value.is_object() => current = value,
			_ => return Err(ClientError::InvalidFrame),
		}
	}

	Ok(Some(current))
}

fn setting(value: Option<&Value>, field: &str) -> Result<Option<String>, ClientError> {
	match value.and_then(|v| v.get(field)) {
		None | Some(Value::Null) => Ok(None),
		Some(value) => required_string(value).map(Some),
	}
}

#[cfg(test)]
#[path = "app_link_settings_tests.rs"]
pub(super) mod tests;
