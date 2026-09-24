//! Isolated native and wire qualification of account-scoped config edits.

use super::*;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[tokio::test]
async fn resolved_native_approval_prevents_late_account_setting_write() {
	let (local, remote) = tokio::io::duplex(65536);
	let (reader, writer) = tokio::io::split(local);
	let (client, mut events) = AppServerClient::from_io(reader, writer);
	let params = json!({"threadId":"thread","turnId":"turn","serverName":"codex_apps"});
	let offered = params.clone();
	let server = tokio::spawn(async move {
		let (reader, mut writer) = tokio::io::split(remote);
		let mut lines = BufReader::new(reader).lines();
		writer
			.write_all(
				format!(
					"{}\n",
					json!({"id":"approval","method":"mcpServer/elicitation/request","params":offered})
				)
				.as_bytes(),
			)
			.await
			.unwrap();
		let read: Value = serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
		assert_eq!(read["method"], "config/read");
		writer.write_all(format!("{}\n",json!({"method":"serverRequest/resolved","params":{"threadId":"thread","requestId":"approval"}})).as_bytes()).await.unwrap();
		writer
			.write_all(
				format!("{}\n", json!({"id":read["id"],"result":config("prompt","v1")})).as_bytes(),
			)
			.await
			.unwrap();
		assert!(tokio::time::timeout(Duration::from_millis(50), lines.next_line()).await.is_err());
	});
	let _ = events.recv().await.unwrap();
	let guard = client
		.server_request_guard(
			&super::super::RequestId::String("approval".into()),
			"mcpServer/elicitation/request",
			&params,
		)
		.unwrap();
	let settings =
		client.app_link_settings("/fixture", "app.with.dot", " work.\"link\\one ").await.unwrap();
	assert!(!guard.is_live());
	assert!(matches!(
		client
			.write_app_link_setting_guarded(
				&settings,
				AppLinkSettingEdit::ApprovalMode(Some("auto".into())),
				guard
			)
			.await,
		Err(ClientError::StaleRequest)
	));
	server.await.unwrap();
}

pub(crate) async fn native(home: &Path) -> (AppServerClient, tokio::process::Child) {
	let binary = std::env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit installed binary");
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
	let first = client.app_link_settings(cwd, "app.with.dot", " work.\"link\\one ").await.unwrap();
	assert_eq!(first.user_mode, None);
	let saved = client
		.write_app_link_setting(&first, AppLinkSettingEdit::ApprovalMode(Some("prompt".into())))
		.await
		.unwrap();
	assert_eq!(
		client
			.app_link_settings(cwd, "app.with.dot", " work.\"link\\one ")
			.await
			.unwrap()
			.user_mode
			.as_deref(),
		Some("prompt")
	);
	assert!(!saved.overridden);
	assert!(matches!(
		client
			.write_app_link_setting(&first, AppLinkSettingEdit::ApprovalMode(Some("auto".into())))
			.await,
		Err(ClientError::Remote(_))
	));
	let other = client.app_link_settings(cwd, "app.with.dot", "personal").await.unwrap();
	client
		.write_app_link_setting(&other, AppLinkSettingEdit::Reviewer(Some("auto_review".into())))
		.await
		.unwrap();
	child.kill().await.unwrap();
	child.wait().await.unwrap();
	let (client, mut child) = native(home.path()).await;
	let cold = client.app_link_settings(cwd, "app.with.dot", " work.\"link\\one ").await.unwrap();
	assert_eq!(cold.effective_mode.as_deref(), Some("prompt"));
	let catalog = client.saved_app_link_settings(cwd).await.unwrap();
	assert_eq!(catalog.entries.len(), 2);
	let saved = catalog.entries.iter().find(|s| s.link_id() == " work.\"link\\one ").unwrap();
	assert_eq!(saved.config_version(), cold.config_version());
	let _cleared = client
		.write_saved_app_link_setting(
			saved,
			AppLinkSettingEdit::ApprovalMode(None),
			client.history_guard(client.history_revision()).unwrap(),
		)
		.await
		.unwrap();
	assert_eq!(
		client
			.app_link_settings(cwd, "app.with.dot", " work.\"link\\one ")
			.await
			.unwrap()
			.user_mode,
		None
	);
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
	let (client, mut child) = native(home.path()).await;
	let catalog = client.saved_app_link_settings(cwd).await.unwrap();
	assert_eq!(catalog.entries.len(), 1);
	assert_eq!(catalog.entries[0].link_id(), "personal");
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
				.write_all(format!("{}\n", json!({"id":request["id"],"result":result})).as_bytes())
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
		.write_app_link_setting(&observed, AppLinkSettingEdit::ApprovalMode(Some("auto".into())))
		.await
		.unwrap();
	assert!(result.overridden);
	assert_eq!(result.version, "v2");
	let readback = client
		.app_link_settings("/fixture/project", "app.with.dot", " work.\"link\\one ")
		.await
		.unwrap();
	assert_eq!(readback.user_mode.as_deref(), Some("auto"));
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
		writer.write_all(format!("{}\n",json!({"id":request["id"],"error":{"code":-32600,"message":"conflict","data":{"config_write_error_code":"configVersionConflict"}}})).as_bytes()).await.unwrap();
		assert!(tokio::time::timeout(Duration::from_millis(50), lines.next_line()).await.is_err());
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
			.write_app_link_setting(&observed, AppLinkSettingEdit::Reviewer(Some("invalid".into())))
			.await,
		Err(ClientError::InvalidFrame)
	));
	assert!(matches!(
		client.write_app_link_setting(&observed, AppLinkSettingEdit::Reviewer(None)).await,
		Err(ClientError::Remote(_))
	));
	server.await.unwrap();
}

#[tokio::test]
async fn acknowledged_save_remains_distinct_from_failed_followup_read() {
	let (local, remote) = tokio::io::duplex(8192);
	let (r, w) = tokio::io::split(local);
	let (client, _events) = AppServerClient::from_io(r, w);
	let backend = tokio::spawn(async move {
		let (r, mut w) = tokio::io::split(remote);
		let mut lines = BufReader::new(r).lines();
		for index in 0..3 {
			let request: Value =
				serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
			let reply = match index {
				0 => json!({"id":request["id"],"result":config("prompt","v1")}),
				1 => {
					assert_eq!(request["method"], "config/batchWrite");
					json!({"id":request["id"],"result":{"status":"ok","filePath":"/fixture/config.toml","version":"v2"}})
				},
				_ => {
					assert_eq!(request["method"], "config/read");
					json!({"id":request["id"],"error":{"code":-32600,"message":"readback unavailable"}})
				},
			};
			w.write_all(format!("{reply}\n").as_bytes()).await.unwrap();
		}
	});
	let review =
		client.app_link_settings("/fixture", "app.with.dot", " work.\"link\\one ").await.unwrap();
	let receipt = client
		.write_app_link_setting(&review, AppLinkSettingEdit::ApprovalMode(Some("auto".into())))
		.await
		.unwrap();
	assert_eq!(receipt.version, "v2");
	assert!(
		client.app_link_settings("/fixture", "app.with.dot", " work.\"link\\one ").await.is_err()
	);
	assert!(!receipt.overridden);
	backend.await.unwrap();
}

async fn catalog_from(response: Value) -> Result<AppLinkSettingsCatalog, ClientError> {
	let (local, remote) = tokio::io::duplex(1024 * 1024);
	let (r, w) = tokio::io::split(local);
	let (client, _events) = AppServerClient::from_io(r, w);
	let backend = tokio::spawn(async move {
		let (r, mut w) = tokio::io::split(remote);
		let mut lines = BufReader::new(r).lines();
		let request: Value =
			serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
		assert_eq!(request["method"], "config/read");
		assert_eq!(request["params"], json!({"cwd":"/fixture","includeLayers":true}));
		w.write_all(format!("{}\n", json!({"id":request["id"],"result":response})).as_bytes())
			.await
			.unwrap();
	});
	let result = client.saved_app_link_settings("/fixture").await;
	backend.await.unwrap();
	result
}
#[tokio::test]
async fn saved_catalog_projects_only_the_active_writable_layer_and_preserves_native_identity() {
	let mut response = config("approve", "v1");
	response["layers"][0]["config"]["apps"]["calendar"] = json!({"links":{"empty":{},"null":{"approvals_reviewer":null},"personal":{"approvals_reviewer":"user"}}});
	response["config"]["apps"]["calendar"] = json!({"links":{"personal":{"approvals_reviewer":"auto_review"},"managed-only":{"default_tools_approval_mode":"prompt"}}});
	response["layers"].as_array_mut().unwrap().push(json!({"name":{"type":"user","file":"/other/config.toml"},"version":"wrong","config":{"apps":{"other":{"links":{"excluded":{"approvals_reviewer":"user"}}}}}}));
	let catalog = catalog_from(response).await.unwrap();
	assert_eq!(catalog.config_file(), "/fixture/config.toml");
	assert_eq!(catalog.config_version(), "v1");
	assert_eq!(catalog.entries.len(), 2);
	let first = &catalog.entries[0];
	assert_eq!(first.app_id(), "app.with.dot");
	assert_eq!(first.link_id(), " work.\"link\\one ");
	assert_eq!(first.user_mode.as_deref(), Some("approve"));
	assert_eq!(first.user_reviewer.as_deref(), Some("future_reviewer"));
	let second = &catalog.entries[1];
	assert_eq!(second.link_id(), "personal");
	assert_eq!(second.user_reviewer.as_deref(), Some("user"));
	assert_eq!(second.effective_reviewer.as_deref(), Some("auto_review"));
	assert!(!format!("{first:?}").contains("must-not-project"));
}
#[tokio::test]
async fn saved_catalog_distinguishes_empty_unreadable_and_oversized_configuration() {
	let mut response = config("approve", "v1");
	response["layers"][0]["config"] = json!({});
	assert!(catalog_from(response.clone()).await.unwrap().entries.is_empty());
	response["layers"][0]["disabledReason"] = json!("managed");
	assert!(matches!(catalog_from(response).await, Err(ClientError::InvalidFrame)));
	for invalid in [
		json!([]),
		json!({"calendar":{"links":[]}}),
		json!({"calendar":{"links":{"work":{"approvals_reviewer":true}}}}),
	] {
		let mut response = config("approve", "v1");
		response["layers"][0]["config"]["apps"] = invalid;
		assert!(matches!(catalog_from(response).await, Err(ClientError::InvalidFrame)));
	}
	let links: serde_json::Map<String, Value> =
		(0..2100).map(|i| (format!("link-{i}"), json!({"approvals_reviewer":"user"}))).collect();
	let mut response = config("approve", "v1");
	response["layers"][0]["config"]["apps"] = json!({"calendar":{"links":links}});
	assert!(matches!(catalog_from(response).await, Err(ClientError::CapacityExceeded)));
}
#[tokio::test]
async fn saved_setting_write_rejects_a_revoked_history_before_transport() {
	let (local, remote) = tokio::io::duplex(65536);
	let (r, w) = tokio::io::split(local);
	let (client, _events) = AppServerClient::from_io(r, w);
	let guard = client.history_guard(0).unwrap();
	let backend = tokio::spawn(async move {
		let (r, mut w) = tokio::io::split(remote);
		let mut lines = BufReader::new(r).lines();
		let request: Value =
			serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
		w.write_all(
			format!("{}\n", json!({"method":"thread/reverted","params":{"threadId":"thread"}}))
				.as_bytes(),
		)
		.await
		.unwrap();
		w.write_all(
			format!("{}\n", json!({"id":request["id"],"result":config("approve","v1")})).as_bytes(),
		)
		.await
		.unwrap();
		assert!(tokio::time::timeout(Duration::from_millis(50), lines.next_line()).await.is_err());
	});
	let catalog = client.saved_app_link_settings("/fixture").await.unwrap();
	assert!(!guard.is_live());
	assert!(matches!(
		client
			.write_saved_app_link_setting(
				&catalog.entries[0],
				AppLinkSettingEdit::ApprovalMode(None),
				guard
			)
			.await,
		Err(ClientError::StaleHistory)
	));
	backend.await.unwrap();
}
