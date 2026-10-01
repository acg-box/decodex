use std::fs;

use tokio::io::{self, AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};

use crate::app_server_client::{
	app_link_settings::tests,
	app_tool_exposure::{self, AppServerClient, ClientError, Value},
};

fn config(preference: Value, version: &str) -> Value {
	let user = serde_json::json!({"apps":{"connector.with.dot":{"omit_tools_from":preference,
  "links":{"work":{"default_tools_approval_mode":"prompt"}}}},"secret":"private-fixture-value"});

	serde_json::json!({"config":{"apps":{"connector.with.dot":{"omit_tools_from":["direct"]}}},
  "layers":[{"name":{"type":"user","file":"/fixture/config.toml"},"version":version,"config":user}]})
}

#[test]
fn connector_scope_cannot_escape_into_account_policy_or_unknown_writes() {
	let params = serde_json::json!({"filePath":"/fixture/config.toml","expectedVersion":"v1","reloadUserConfig":true,
  "edits":[{"keyPath":"apps.\"a.\\\"quoted\\\\id\".omit_tools_from","value":[],"mergeStrategy":"replace"}]});

	assert!(app_tool_exposure::is_app_tool_exposure_write(&params));

	for path in [
		"apps._default.omit_tools_from",
		"apps.\"_default\".omit_tools_from",
		"apps.\"a\".links.\"work\".omit_tools_from",
		"apps.\"a\".enabled",
	] {
		let mut changed = params.clone();

		changed["edits"][0]["keyPath"] = serde_json::json!(path);

		assert!(!app_tool_exposure::is_app_tool_exposure_write(&changed));
	}
	for value in [
		serde_json::json!(["direct", "direct"]),
		serde_json::json!(["future"]),
		serde_json::json!(true),
	] {
		let mut changed = params.clone();

		changed["edits"][0]["value"] = value;

		assert!(!app_tool_exposure::is_app_tool_exposure_write(&changed));
	}

	let mut missing = params.clone();

	missing["edits"][0].as_object_mut().unwrap().remove("value");

	assert!(!app_tool_exposure::is_app_tool_exposure_write(&missing));
	assert_eq!(
		app_tool_exposure::omissions(
			&serde_json::json!({"apps":{"a":{"omit_tools_from":["future"]}}}),
			"a"
		)
		.unwrap(),
		Some(vec!["future".into()])
	);

	for malformed in [
		serde_json::json!({"apps":false}),
		serde_json::json!({"apps":{"a":[]}}),
		serde_json::json!({"apps":{"a":{"omit_tools_from":[false]}}}),
	] {
		assert!(app_tool_exposure::omissions(&malformed, "a").is_err());
	}
}

#[tokio::test]
async fn connector_edits_keep_inheritance_separate_and_preserve_native_scope() {
	for target in [None, Some(vec![]), Some(vec!["deferred".to_owned()])] {
		let (local, remote) = io::duplex(65_536);
		let (reader, writer) = io::split(local);
		let (client, _events) = AppServerClient::from_io(reader, writer);
		let expected = target.clone();
		let server = tokio::spawn(async move {
			let (reader, mut writer) = io::split(remote);
			let mut lines = BufReader::new(reader).lines();

			for index in 0..3 {
				let request: Value =
					serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
				let result = if index == 1 {
					assert_eq!(request["method"], "config/batchWrite");
					assert!(app_tool_exposure::is_app_tool_exposure_write(&request["params"]));
					assert_eq!(
						request["params"],
						serde_json::json!({"filePath":"/fixture/config.toml","expectedVersion":"v1","reloadUserConfig":true,
      "edits":[{"keyPath":"apps.\"connector.with.dot\".omit_tools_from","value":expected,"mergeStrategy":"replace"}]})
					);

					serde_json::json!({"status":"okOverridden","filePath":"/fixture/config.toml","version":"v2"})
				} else {
					assert_eq!(request["method"], "config/read");
					assert_eq!(
						request["params"],
						serde_json::json!({"cwd":"/fixture","includeLayers":true})
					);

					if index == 0 {
						config(serde_json::json!(["code_mode"]), "v1")
					} else {
						config(serde_json::json!(expected), "v2")
					}
				};

				writer
					.write_all(
						format!("{}\n", serde_json::json!({"id":request["id"],"result":result}))
							.as_bytes(),
					)
					.await
					.unwrap();
			}
		});
		let observed = client.app_tool_exposure("/fixture", "connector.with.dot").await.unwrap();
		let first_fingerprint = observed.fingerprint();

		assert!(!format!("{observed:?}").contains("private-fixture-value"));

		let guard = client.history_guard(client.history_revision()).unwrap();
		let saved = client.write_app_tool_exposure(&observed, target.clone(), guard).await.unwrap();

		assert_eq!(saved.settings.preference, target);
		assert_eq!(saved.settings.effective, Some(vec!["direct".to_owned()]));
		assert!(saved.overridden);
		assert_ne!(first_fingerprint, saved.settings.fingerprint());

		server.await.unwrap();
	}
}

#[tokio::test]
async fn source_changes_and_connection_loss_never_replay_a_connector_edit() {
	let (local, remote) = io::duplex(65_536);
	let (reader, writer) = io::split(local);
	let (client, mut events) = AppServerClient::from_io(reader, writer);
	let guard = client.history_guard(client.history_revision()).unwrap();
	let server = tokio::spawn(async move {
		let (reader, mut writer) = io::split(remote);
		let mut lines = BufReader::new(reader).lines();
		let request: Value =
			serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();

		writer
			.write_all(
				format!(
					"{}\n",
					serde_json::json!({"id":request["id"],"result":config(serde_json::json!(null),"v1")})
				)
				.as_bytes(),
			)
			.await
			.unwrap();
		writer
			.write_all(b"{\"method\":\"thread/reverted\",\"params\":{\"threadId\":\"changed\"}}\n")
			.await
			.unwrap();

		let request: Value =
			serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();

		assert_eq!(request["method"], "config/batchWrite");
		// Close after accepting the only valid write. No success receipt is sent.
	});
	let observed = client.app_tool_exposure("/fixture", "connector.with.dot").await.unwrap();
	let _ = events.recv().await.unwrap();

	assert!(!guard.is_live());
	assert!(matches!(
		client.write_app_tool_exposure(&observed, None, guard).await,
		Err(ClientError::StaleHistory)
	));

	let (other, _peer) = io::duplex(1_024);
	let (reader, writer) = io::split(other);
	let (other, _events) = AppServerClient::from_io(reader, writer);

	assert!(matches!(
		other.write_app_tool_exposure(&observed, None, other.history_guard(0).unwrap()).await,
		Err(ClientError::InvalidFrame)
	));

	let fresh = client.history_guard(client.history_revision()).unwrap();

	assert!(client.write_app_tool_exposure(&observed, None, fresh).await.is_err());

	server.await.unwrap();
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native connector configuration"]
async fn installed_native_connector_exposure_roundtrip_and_cold_conflict() {
	let home = tempfile::tempdir().unwrap();

	fs::write(home.path().join("config.toml"),"model = \"gpt-5.6-sol\"\n[features]\napps = false\n[apps.\"connector.with.dot\".links.work]\ndefault_tools_approval_mode = \"prompt\"\n").unwrap();

	let cwd = home.path().to_str().unwrap();
	let (client, mut child) = tests::native(home.path()).await;
	let observed = client.app_tool_exposure(cwd, "connector.with.dot").await.unwrap();

	assert_eq!(observed.preference, None);

	let saved = client
		.write_app_tool_exposure(
			&observed,
			Some(vec!["deferred".into()]),
			client.history_guard(client.history_revision()).unwrap(),
		)
		.await
		.unwrap();

	assert_eq!(saved.settings.preference, Some(vec!["deferred".into()]));

	let (other, mut other_child) = tests::native(home.path()).await;
	let current = other.app_tool_exposure(cwd, "connector.with.dot").await.unwrap();

	assert_eq!(current.preference, saved.settings.preference);

	let cleared = other
		.write_app_tool_exposure(
			&current,
			Some(vec![]),
			other.history_guard(other.history_revision()).unwrap(),
		)
		.await
		.unwrap();

	assert_eq!(cleared.settings.preference, Some(vec![]));
	assert!(matches!(
		client
			.write_app_tool_exposure(
				&saved.settings,
				Some(vec!["direct".into()]),
				client.history_guard(client.history_revision()).unwrap()
			)
			.await,
		Err(ClientError::Remote(_))
	));

	for child in [&mut child, &mut other_child] {
		child.kill().await.unwrap();
		child.wait().await.unwrap();
	}

	let (client, mut child) = tests::native(home.path()).await;
	let cold = client.app_tool_exposure(cwd, "connector.with.dot").await.unwrap();

	assert_eq!(cold.preference, Some(vec![]));

	let inherited = client
		.write_app_tool_exposure(
			&cold,
			None,
			client.history_guard(client.history_revision()).unwrap(),
		)
		.await
		.unwrap();

	assert_eq!(inherited.settings.preference, None);

	let link = client.app_link_settings(cwd, "connector.with.dot", "work").await.unwrap();

	assert_eq!(link.user_mode.as_deref(), Some("prompt"));

	child.kill().await.unwrap();
	child.wait().await.unwrap();
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native config parse failure"]
async fn installed_native_config_write_retains_parse_cause() {
	let home = tempfile::tempdir().unwrap();
	let path = home.path().join("config.toml");

	fs::write(&path, "[features]\napps = false\n").unwrap();

	let (client, mut child) = tests::native(home.path()).await;
	let observed =
		client.app_tool_exposure(home.path().to_str().unwrap(), "calendar").await.unwrap();

	fs::write(&path, "approvals_reviewer = [\n").unwrap();

	let result = client
		.write_app_tool_exposure(
			&observed,
			Some(vec![]),
			client.history_guard(client.history_revision()).unwrap(),
		)
		.await;

	child.kill().await.unwrap();
	child.wait().await.unwrap();

	let Err(ClientError::Remote(error)) = result else { panic!("native parse rejection") };

	assert!(error.message.contains("config.toml"));
	assert!(error.message.contains("unclosed array"));
	assert!(!format!("{error:?}").contains("config.toml"), "private payload stays out of Debug");
	assert_eq!(fs::read_to_string(path).unwrap(), "approvals_reviewer = [\n");
}
