//! Versioned native account settings. The caller owns task and account selection.
use super::{AppServerClient, ClientError, Outbound};
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

/// Persisted write and a fresh configuration read; not proof of live tool behavior.
#[derive(Debug)]
pub struct AppLinkSettingsWrite {
	/// Native receipt says a higher configuration layer overrides this edit.
	pub overridden: bool,
	/// Fresh readback for the same connection, directory and account.
	pub settings: AppLinkSettings,
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
		let response = tokio::time::timeout(
			Duration::from_secs(30),
			self.request("config/read", json!({"cwd":cwd,"includeLayers":true})),
		)
		.await
		.map_err(|_| ClientError::Io)??;
		let layers = response["layers"].as_array().ok_or(ClientError::InvalidFrame)?;
		// Native config/read orders layers high to low; the first user layer is active.
		let user = layers
			.iter()
			.find(|layer| layer["name"]["type"] == "user")
			.ok_or(ClientError::InvalidFrame)?;
		if !user["disabledReason"].is_null() {
			return Err(ClientError::InvalidFrame);
		}
		let file = required_string(&user["name"]["file"])?;
		if !Path::new(&file).is_absolute() {
			return Err(ClientError::InvalidFrame);
		}
		let version = required_string(&user["version"])?;
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

	/// Write one reviewed account setting with the observed native version, then read back.
	/// Never retries: an error after dispatch can mean the setting was already saved.
	pub async fn write_app_link_setting(
		&self,
		observed: &AppLinkSettings,
		edit: AppLinkSettingEdit,
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
		let receipt = tokio::time::timeout(
			Duration::from_secs(30),
			self.request(
				"config/batchWrite",
				json!({"filePath":observed.file,"expectedVersion":observed.version,"reloadUserConfig":true,
				"edits":[{"keyPath":key,"value":value,"mergeStrategy":"replace"}]}),
			),
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
		let settings = self.app_link_settings(&observed.cwd, &observed.app, &observed.link).await?;
		Ok(AppLinkSettingsWrite { overridden, settings })
	}
}

fn valid_identity(value: &str) -> bool {
	!value.is_empty() && value.len() <= 4096 && !value.chars().any(char::is_control)
}

fn quoted_key(value: &str) -> String {
	format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn required_string(value: &Value) -> Result<String, ClientError> {
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
mod tests {
	use super::*;
	use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

	async fn native(home: &Path) -> (AppServerClient, tokio::process::Child) {
		let binary =
			std::env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit installed binary");
		assert!(Path::new(&binary).is_absolute());
		let mut child = tokio::process::Command::new(binary)
			.arg("app-server")
			.env_clear()
			.env("HOME", home)
			.env("CODEX_HOME", home)
			.env("PATH", "/usr/bin:/bin")
			.current_dir(home)
			.stdin(std::process::Stdio::piped())
			.stdout(std::process::Stdio::piped())
			.stderr(std::process::Stdio::null())
			.kill_on_drop(true)
			.spawn()
			.unwrap();
		let (client, events) =
			AppServerClient::from_io(child.stdout.take().unwrap(), child.stdin.take().unwrap());
		tokio::spawn(async move {
			let mut events = events;
			while events.recv().await.is_some() {}
		});
		client
			.initialize(json!({"clientInfo":{"name":"decodex_link_settings_test","version":"0.1"},
			"capabilities":{"experimentalApi":true}}))
			.await
			.unwrap();
		(client, child)
	}

	#[tokio::test]
	#[ignore = "requires DECODEX_TEST_CODEX_BINARY; uses isolated native configuration"]
	async fn installed_native_account_settings_survive_restart_and_reject_stale_write() {
		let home = tempfile::tempdir().unwrap();
		std::fs::write(home.path().join("config.toml"), "model = \"gpt-5.6-sol\"\n").unwrap();
		let cwd = home.path().to_str().unwrap();
		let (client, mut child) = native(home.path()).await;
		let first =
			client.app_link_settings(cwd, "app.with.dot", " work.\"link\\one ").await.unwrap();
		assert_eq!(first.user_mode, None);
		let saved = client
			.write_app_link_setting(&first, AppLinkSettingEdit::ApprovalMode(Some("prompt".into())))
			.await
			.unwrap();
		assert_eq!(saved.settings.user_mode.as_deref(), Some("prompt"));
		assert!(!saved.overridden);
		assert!(matches!(
			client
				.write_app_link_setting(
					&first,
					AppLinkSettingEdit::ApprovalMode(Some("auto".into()))
				)
				.await,
			Err(ClientError::Remote(_))
		));
		let other = client.app_link_settings(cwd, "app.with.dot", "personal").await.unwrap();
		client
			.write_app_link_setting(
				&other,
				AppLinkSettingEdit::Reviewer(Some("auto_review".into())),
			)
			.await
			.unwrap();
		child.kill().await.unwrap();
		child.wait().await.unwrap();
		let (client, mut child) = native(home.path()).await;
		let cold =
			client.app_link_settings(cwd, "app.with.dot", " work.\"link\\one ").await.unwrap();
		assert_eq!(cold.effective_mode.as_deref(), Some("prompt"));
		let cleared = client
			.write_app_link_setting(&cold, AppLinkSettingEdit::ApprovalMode(None))
			.await
			.unwrap();
		assert_eq!(cleared.settings.user_mode, None);
		assert_eq!(
			client
				.app_link_settings(cwd, "app.with.dot", "personal")
				.await
				.unwrap()
				.user_reviewer
				.as_deref(),
			Some("auto_review")
		);
		child.kill().await.unwrap();
		child.wait().await.unwrap();
	}

	fn config(mode: &str, version: &str) -> Value {
		let config = json!({"apps":{"app.with.dot":{"links":{" work.\"link\\one ":{
			"default_tools_approval_mode":mode,"approvals_reviewer":"future_reviewer"
		}}}},"secret":"must-not-project"});
		json!({"config":config,"layers":[{"name":{"type":"user","file":"/fixture/config.toml"},
			"version":version,"config":config}]})
	}

	#[tokio::test]
	async fn account_write_preserves_scope_and_reads_back_without_claiming_live_application() {
		let (local, remote) = tokio::io::duplex(65536);
		let (reader, writer) = tokio::io::split(local);
		let (client, _events) = AppServerClient::from_io(reader, writer);
		let server = tokio::spawn(async move {
			let (reader, mut writer) = tokio::io::split(remote);
			let mut lines = BufReader::new(reader).lines();
			for index in 0..3 {
				let request: Value =
					serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
				let result = if index == 1 {
					assert_eq!(request["method"], "config/batchWrite");
					assert_eq!(
						request["params"],
						json!({"filePath":"/fixture/config.toml","expectedVersion":"v1",
						"reloadUserConfig":true,"edits":[{"keyPath":"apps.\"app.with.dot\".links.\" work.\\\"link\\\\one \".default_tools_approval_mode",
						"value":"auto","mergeStrategy":"replace"}]})
					);
					json!({"status":"okOverridden","filePath":"/fixture/config.toml","version":"v2"})
				} else {
					assert_eq!(request["method"], "config/read");
					assert_eq!(
						request["params"],
						json!({"cwd":"/fixture/project","includeLayers":true})
					);
					config(
						if index == 0 { "prompt" } else { "auto" },
						if index == 0 { "v1" } else { "v2" },
					)
				};
				writer
					.write_all(
						format!("{}\n", json!({"id":request["id"],"result":result})).as_bytes(),
					)
					.await
					.unwrap();
			}
		});
		let observed = client
			.app_link_settings("/fixture/project", "app.with.dot", " work.\"link\\one ")
			.await
			.unwrap();
		assert_eq!(observed.effective_mode.as_deref(), Some("prompt"));
		assert_eq!(observed.effective_reviewer.as_deref(), Some("future_reviewer"));
		assert!(!format!("{observed:?}").contains("must-not-project"));
		let result = client
			.write_app_link_setting(
				&observed,
				AppLinkSettingEdit::ApprovalMode(Some("auto".into())),
			)
			.await
			.unwrap();
		assert!(result.overridden);
		assert_eq!(result.settings.user_mode.as_deref(), Some("auto"));
		server.await.unwrap();
	}

	#[tokio::test]
	async fn native_conflict_is_not_retried_and_other_connections_cannot_use_snapshot() {
		let (local, remote) = tokio::io::duplex(65536);
		let (reader, writer) = tokio::io::split(local);
		let (client, _events) = AppServerClient::from_io(reader, writer);
		let server = tokio::spawn(async move {
			let (reader, mut writer) = tokio::io::split(remote);
			let mut lines = BufReader::new(reader).lines();
			let request: Value =
				serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
			writer
				.write_all(
					format!("{}\n", json!({"id":request["id"],"result":config("prompt","v1")}))
						.as_bytes(),
				)
				.await
				.unwrap();
			let request: Value =
				serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
			assert_eq!(request["method"], "config/batchWrite");
			assert!(request["params"]["edits"][0]["value"].is_null());
			writer.write_all(format!("{}\n",json!({"id":request["id"],"error":{"code":-32000,"message":"conflict","data":{"config_write_error_code":"configVersionConflict"}}})).as_bytes()).await.unwrap();
			assert!(
				tokio::time::timeout(Duration::from_millis(50), lines.next_line()).await.is_err()
			);
		});
		let observed = client
			.app_link_settings("/fixture/project", "app.with.dot", " work.\"link\\one ")
			.await
			.unwrap();
		let (other, _peer) = tokio::io::duplex(1024);
		let (r, w) = tokio::io::split(other);
		let (other, _events) = AppServerClient::from_io(r, w);
		assert!(matches!(
			other.write_app_link_setting(&observed, AppLinkSettingEdit::Reviewer(None)).await,
			Err(ClientError::InvalidFrame)
		));
		assert!(matches!(
			client
				.write_app_link_setting(
					&observed,
					AppLinkSettingEdit::Reviewer(Some("invalid".into()))
				)
				.await,
			Err(ClientError::InvalidFrame)
		));
		assert!(matches!(
			client.write_app_link_setting(&observed, AppLinkSettingEdit::Reviewer(None)).await,
			Err(ClientError::Remote(_))
		));
		server.await.unwrap();
	}
}
