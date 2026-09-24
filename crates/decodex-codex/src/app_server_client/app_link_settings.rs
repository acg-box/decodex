//! Versioned native account settings. The caller owns task and account selection.
use super::{AppServerClient, ClientError, HistoryGuard, Outbound, ServerRequestGuard};
use serde_json::{Value, json};
use std::{path::Path, time::Duration};
use tokio::sync::mpsc;

/// A single explicit edit; `None` removes only this account's override.
#[derive(Clone, Debug)]
pub enum AppLinkSettingEdit {
	/// Native mode: auto, prompt, writes, or approve.
	ApprovalMode(Option<String>),
	/// Native reviewer: user or auto_review.
	Reviewer(Option<String>),
}

/// Narrow readback. No credentials or unrelated native configuration leave this adapter.
#[derive(Clone)]
pub struct AppLinkSettings {
	connection: mpsc::Sender<Outbound>,
	cwd: String,
	app: String,
	link: String,
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

impl std::fmt::Debug for AppLinkSettings {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str("AppLinkSettings([private native account scope])")
	}
}

impl AppLinkSettings {
	/// Exact native app identity associated with this reviewed configuration.
	pub fn app_id(&self) -> &str {
		&self.app
	}

	/// Exact native connection identity; this is not the Codex login account.
	pub fn link_id(&self) -> &str {
		&self.link
	}

	/// Native writable file identity, shared by tasks and account-bound requests using it.
	pub fn config_file(&self) -> &str {
		&self.file
	}

	/// Reviewed version used for native conflict detection.
	pub fn config_version(&self) -> &str {
		&self.version
	}

	/// Opaque identity of the reviewed scope, version and displayed configuration.
	/// Callers must additionally bind this identity to their current task and process source.
	pub fn review_fingerprint(&self) -> String {
		use sha2::{Digest as _, Sha256};
		let facts = json!([
			self.cwd,
			self.app,
			self.link,
			self.file,
			self.version,
			self.effective_mode,
			self.effective_reviewer,
			self.user_mode,
			self.user_reviewer
		]);
		Sha256::digest(facts.to_string().as_bytes())
			.iter()
			.map(|byte| format!("{byte:02x}"))
			.collect()
	}
}

/// Saved connection overrides from one native writable layer, not a cloud account directory.
pub struct AppLinkSettingsCatalog {
	file: String,
	version: String,
	/// Connections with an explicit approval mode or reviewer override.
	pub entries: Vec<AppLinkSettings>,
}
impl AppLinkSettingsCatalog {
	/// Native shared writable configuration file, including when there are no overrides.
	pub fn config_file(&self) -> &str {
		&self.file
	}

	/// Version of the single configuration snapshot used for every entry.
	pub fn config_version(&self) -> &str {
		&self.version
	}
}
enum WriteGuard {
	Request(ServerRequestGuard),
	History(HistoryGuard),
}

/// Native write acknowledgement, separate from readback and live tool behavior.
#[derive(Debug)]
pub struct AppLinkSettingsWrite {
	/// Native receipt says a higher configuration layer overrides this edit.
	pub overridden: bool,
	/// Native version after the acknowledged save.
	pub version: String,
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
		self.project_app_link(cwd, app, link, &response)
	}

	async fn read_app_config(&self, cwd: &str) -> Result<Value, ClientError> {
		if !Path::new(cwd).is_absolute() {
			return Err(ClientError::InvalidFrame);
		}
		tokio::time::timeout(
			Duration::from_secs(30),
			self.request("config/read", json!({"cwd":cwd,"includeLayers":true})),
		)
		.await
		.map_err(|_| ClientError::Io)?
	}

	fn project_app_link(
		&self,
		cwd: &str,
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
			connection: self.outbound.clone(),
			cwd: cwd.into(),
			app: app.into(),
			link: link.into(),
			file,
			version,
			effective_mode: setting(effective, "default_tools_approval_mode")?,
			effective_reviewer: setting(effective, "approvals_reviewer")?,
			user_mode: setting(writable, "default_tools_approval_mode")?,
			user_reviewer: setting(writable, "approvals_reviewer")?,
		})
	}

	/// Read only saved native connection overrides from one config snapshot.
	/// Reject malformed or oversized inventories rather than silently omitting settings.
	pub async fn saved_app_link_settings(
		&self,
		cwd: &str,
	) -> Result<AppLinkSettingsCatalog, ClientError> {
		let response = self.read_app_config(cwd).await?;
		let (user, file, version) = writable_layer(&response)?;
		if !response["config"].is_object() {
			return Err(ClientError::InvalidFrame);
		}
		let mut entries = Vec::new();
		let mut bytes = 0usize;
		if let Some(apps) = object_or_absent(user["config"].get("apps"))? {
			for (app, config) in apps {
				let config = config.as_object().ok_or(ClientError::InvalidFrame)?;
				if let Some(links) = object_or_absent(config.get("links"))? {
					for (link, config) in links {
						let config = config.as_object().ok_or(ClientError::InvalidFrame)?;
						if ["default_tools_approval_mode", "approvals_reviewer"]
							.iter()
							.all(|k| config.get(*k).is_none_or(Value::is_null))
						{
							continue;
						}
						let entry = self.project_app_link(cwd, app, link, &response)?;
						bytes += app.len()
							+ link.len() + 128 + [
							&entry.effective_mode,
							&entry.effective_reviewer,
							&entry.user_mode,
							&entry.user_reviewer,
						]
						.iter()
						.filter_map(|v| v.as_ref())
						.map(|v| v.len())
						.sum::<usize>();
						if bytes > 256 * 1024 {
							return Err(ClientError::CapacityExceeded);
						}
						entries.push(entry);
					}
				}
			}
		}
		entries.sort_by(|a, b| (&a.app, &a.link).cmp(&(&b.app, &b.link)));
		Ok(AppLinkSettingsCatalog { file, version, entries })
	}

	/// Modify an existing saved connection after its original approval has ended.
	/// Native file/version validation and the current source guard still apply.
	pub async fn write_saved_app_link_setting(
		&self,
		observed: &AppLinkSettings,
		edit: AppLinkSettingEdit,
		guard: HistoryGuard,
	) -> Result<AppLinkSettingsWrite, ClientError> {
		if observed.user_mode.is_none() && observed.user_reviewer.is_none() {
			return Err(ClientError::InvalidFrame);
		}
		self.write_app_link_setting_inner(observed, edit, Some(WriteGuard::History(guard))).await
	}

	/// Write one reviewed account setting with the observed native version.
	/// Never retries: an error after dispatch can mean the setting was already saved.
	pub async fn write_app_link_setting(
		&self,
		observed: &AppLinkSettings,
		edit: AppLinkSettingEdit,
	) -> Result<AppLinkSettingsWrite, ClientError> {
		self.write_app_link_setting_inner(observed, edit, None).await
	}

	/// Write only while the exact originating native approval request remains live.
	/// Resolution observed by the transport prevents dispatch even before the host processes it.
	pub async fn write_app_link_setting_guarded(
		&self,
		observed: &AppLinkSettings,
		edit: AppLinkSettingEdit,
		guard: ServerRequestGuard,
	) -> Result<AppLinkSettingsWrite, ClientError> {
		self.write_app_link_setting_inner(observed, edit, Some(WriteGuard::Request(guard))).await
	}

	async fn write_app_link_setting_inner(
		&self,
		observed: &AppLinkSettings,
		edit: AppLinkSettingEdit,
		guard: Option<WriteGuard>,
	) -> Result<AppLinkSettingsWrite, ClientError> {
		if !self.outbound.same_channel(&observed.connection) {
			return Err(ClientError::InvalidFrame);
		}
		let (field, value) = match edit {
			AppLinkSettingEdit::ApprovalMode(value)
				if value
					.as_deref()
					.is_none_or(|v| matches!(v, "auto" | "prompt" | "writes" | "approve")) =>
				("default_tools_approval_mode", value),
			AppLinkSettingEdit::Reviewer(value)
				if value.as_deref().is_none_or(|v| matches!(v, "user" | "auto_review")) =>
				("approvals_reviewer", value),
			_ => return Err(ClientError::InvalidFrame),
		};
		let key = format!(
			"apps.{}.links.{}.{field}",
			quoted_key(&observed.app),
			quoted_key(&observed.link)
		);
		let params = json!({"filePath":observed.file,"expectedVersion":observed.version,"reloadUserConfig":true,
			"edits":[{"keyPath":key,"value":value,"mergeStrategy":"replace"}]});
		let receipt = tokio::time::timeout(Duration::from_secs(30), async {
			match guard {
				Some(WriteGuard::Request(guard)) =>
					self.request_guarded("config/batchWrite", params, guard).await,
				Some(WriteGuard::History(guard)) =>
					self.request_with_history("config/batchWrite", params, guard).await,
				None => self.request("config/batchWrite", params).await,
			}
		})
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
		let version = required_string(&receipt["version"])?;
		Ok(AppLinkSettingsWrite { overridden, version })
	}
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
fn object_or_absent(
	value: Option<&Value>,
) -> Result<Option<&serde_json::Map<String, Value>>, ClientError> {
	match value {
		None | Some(Value::Null) => Ok(None),
		Some(Value::Object(object)) => Ok(Some(object)),
		_ => Err(ClientError::InvalidFrame),
	}
}

pub(super) fn valid_identity(value: &str) -> bool {
	!value.is_empty() && value.len() <= 4096 && !value.chars().any(char::is_control)
}

pub(super) fn quoted_key(value: &str) -> String {
	format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Validate the narrow native write envelope accepted by the retained process bridge.
/// Native configuration still owns writable-file, version and managed-policy validation.
pub fn is_app_link_settings_write(params: &Value) -> bool {
	let Some(object) = params.as_object() else { return false };
	if object.len() != 4
		|| params["reloadUserConfig"] != true
		|| !params["filePath"].as_str().is_some_and(|p| Path::new(p).is_absolute())
		|| required_string(&params["expectedVersion"]).is_err()
	{
		return false;
	}
	let Some(edits) = params["edits"].as_array().filter(|edits| edits.len() == 1) else {
		return false;
	};
	let edit = &edits[0];
	if !edit.as_object().is_some_and(|edit| edit.len() == 3)
		|| edit["mergeStrategy"] != "replace"
		|| edit.get("value").is_none()
	{
		return false;
	}
	let Some(path) = edit["keyPath"].as_str().and_then(|p| p.strip_prefix("apps.")) else {
		return false;
	};
	let Some((app, path)) = take_quoted_key(path) else { return false };
	let Some(path) = path.strip_prefix(".links.") else { return false };
	let Some((link, field)) = take_quoted_key(path) else { return false };
	if !valid_identity(&app) || !valid_identity(&link) {
		return false;
	}
	let allowed: &[&str] = match field {
		".default_tools_approval_mode" => &["auto", "prompt", "writes", "approve"],
		".approvals_reviewer" => &["user", "auto_review"],
		_ => return false,
	};
	edit["value"].is_null() || edit["value"].as_str().is_some_and(|v| allowed.contains(&v))
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
mod tests;

#[cfg(test)]
#[path = "app_link_native_tests.rs"]
mod native_tests;
