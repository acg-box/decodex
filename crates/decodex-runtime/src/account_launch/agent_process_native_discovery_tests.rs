//! Retain independent native discovery and directory policy evidence.
use std::{env, fs};

use tokio::time;

use crate::account_launch::agent_process::native_tests::{
	Child, ClientError, Duration, NativeSession, Value,
};

struct NativeChild(Child);
impl Drop for NativeChild {
	fn drop(&mut self) {
		let _ = self.0.kill();
		let _ = self.0.wait();
	}
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native MCP discovery"]
async fn installed_native_mcp_capabilities_survive_tool_discovery_failure() {
	let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");

	assert!(std::path::Path::new(&binary).is_absolute());

	let home = tempfile::tempdir().unwrap();
	let fixture = home.path().join("mcp.py");

	fs::write(&fixture, include_str!("native_mcp_capabilities.py")).unwrap();

	let mut config = String::from("cli_auth_credentials_store = \"file\"\n");

	for mode in ["healthy", "tools-error", "init-error"] {
		config.push_str(&format!(
			"[mcp_servers.{mode}]\ncommand = \"/usr/bin/python3\"\nargs = [{}, \"{mode}\"]\n",
			serde_json::to_string(&fixture).unwrap()
		));
	}

	fs::write(home.path().join("config.toml"), config).unwrap();

	for _ in 0..2 {
		let session = NativeSession::start(&binary, home.path());

		time::timeout(Duration::from_secs(40), async {
			let started = session
				.client
				.thread_start(
					serde_json::json!({"cwd":home.path(),"approvalPolicy":"never","sandbox":"read-only"}),
				)
				.await
				.unwrap();
			let id = started["thread"]["id"].as_str().unwrap();
			let rows = session.client.mcp_server_statuses(id).await.unwrap();

			assert_eq!(rows.len(), 3);

			let mut paged = Vec::new();
			let mut cursor = Value::Null;

			for page_index in 0..3 {
				let page = session
					.client
					.request(
						"mcpServerStatus/list",
						serde_json::json!({
							"threadId":id,"detail":"full","limit":1,"cursor":cursor,
						}),
					)
					.await
					.unwrap();
				let data = page["data"].as_array().unwrap();

				assert_eq!(data.len(), 1);

				paged.extend(data.iter().cloned());

				cursor = page["nextCursor"].clone();

				assert_eq!(cursor.is_null(), page_index == 2);
			}

			assert_eq!(paged, rows, "pagination must preserve independent server metadata");

			for mode in ["healthy", "tools-error", "init-error"] {
				let row = rows.iter().find(|row| row["name"] == mode).unwrap();

				if mode == "init-error" {
					assert!(row["serverCapabilities"].is_null());
				} else {
					assert_eq!(
						row["serverCapabilities"]["extensions"]["openai/settings"]["readTool"],
						"settings.read"
					);
					assert_eq!(row["serverCapabilities"]["tools"], serde_json::json!({}));
				}
				if mode == "tools-error" {
					assert!(row["toolsError"].as_str().is_some());
					assert_eq!(row["tools"], serde_json::json!({}));
				}
			}
		})
		.await
		.unwrap();
	}
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; native profile catalog path context"]
async fn installed_native_permission_catalog_uses_each_requested_working_directory() {
	let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");
	let root = tempfile::tempdir().unwrap();
	let home = root.path().join("home");
	let ordinary = root.path().join("workspace");
	let brackets = root.path().join("[workspace]");

	for path in [&home, &ordinary, &brackets] {
		fs::create_dir(path).unwrap();
	}

	let config = r#"default_permissions = ":workspace"
[permissions.scoped.workspace_roots]
"private/*.env" = true
[permissions.scoped.filesystem]
":workspace_roots" = "write"
"#;

	fs::write(home.join("config.toml"), config).unwrap();

	let session = NativeSession::start(&binary, &home.canonicalize().unwrap());

	for (directory, allowed) in [(&brackets, false), (&ordinary, true), (&brackets, false)] {
		let directory = directory.canonicalize().unwrap();
		let profiles =
			session.client.permission_profiles(directory.to_str().unwrap()).await.unwrap();

		assert_eq!(
			profiles.iter().find(|profile| profile.id == "scoped").unwrap().allowed,
			allowed
		);
	}

	assert_eq!(fs::read_to_string(home.join("config.toml")).unwrap(), config);
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated enterprise MCP policy"]
async fn installed_native_enterprise_mcp_requires_thread_and_rejects_project_downgrade() {
	let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");
	let root = tempfile::tempdir().unwrap();
	let home = root.path().join("home");
	let project = root.path().join("project");

	fs::create_dir(&home).unwrap();
	fs::create_dir_all(project.join(".codex")).unwrap();
	fs::create_dir(project.join(".git")).unwrap();

	let project = project.canonicalize().unwrap();
	let config = format!(
		r#"
cli_auth_credentials_store = "file"
[projects.{}]
trust_level = "trusted"
[features]
use_xaa = true
[mcp_enterprise_managed_auth.idp]
issuer = "https://idp.invalid"
client_id = "synthetic-enterprise"
[mcp_servers.enterprise]
url = "https://resource.invalid/mcp"
auth = "ema_auth"
"#,
		serde_json::to_string(&project).unwrap()
	);

	fs::write(home.join("config.toml"), &config).unwrap();

	for _ in 0..2 {
		fs::write(project.join(".codex/config.toml"), "").unwrap();

		let session = NativeSession::start(&binary, &home);

		time::timeout(Duration::from_secs(30), async {
			let login = session.client.request("mcpServer/oauth/login", serde_json::json!({"name":"enterprise"})).await;

			assert!(matches!(&login, Err(ClientError::Remote(error)) if error.code == -32_600 && error.message.contains("requires a connected thread")), "missing thread: {login:?}");

			for change in [
				"auth = \"oauth\"",
				"auth = \"chatgpt\"",
				"url = \"https://other.invalid/mcp\"",
				"oauth_resource = \"https://other.invalid\"",
				"scopes = [\"admin\"]",
				"oauth.client_id = \"other-client\"",
				"oauth.authorization_server_issuer = \"https://other.invalid\"",
			] {
				fs::write(project.join(".codex/config.toml"), format!("[mcp_servers.enterprise]\n{change}\n")).unwrap();

				let result = session.client.thread_start(serde_json::json!({"cwd":project,"approvalPolicy":"never","sandbox":"read-only"})).await;

				assert!(matches!(&result, Err(ClientError::Remote(error)) if error.message.contains("one non-project config layer")), "project override {change}: {result:?}");
			}
		}).await.unwrap();
	}

	assert_eq!(fs::read_to_string(home.join("config.toml")).unwrap(), config);
}
