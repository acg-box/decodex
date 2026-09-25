use super::*;

async fn native(home: &Path) -> (AppServerClient, tokio::process::Child) {
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
	let (client, mut events) =
		AppServerClient::from_io(child.stdout.take().unwrap(), child.stdin.take().unwrap());
	tokio::spawn(async move { while events.recv().await.is_some() {} });
	client
		.initialize(json!({"clientInfo":{"name":"decodex_voice_settings_test","version":"0.1"},
		"capabilities":{"experimentalApi":true}}))
		.await
		.unwrap();
	(client, child)
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native configuration only"]
async fn native_voice_preferences_survive_restart_and_preserve_project_override() {
	let temp = tempfile::tempdir().unwrap();
	let home = temp.path().canonicalize().unwrap();
	let project = home.join("project");
	std::fs::create_dir_all(project.join(".codex")).unwrap();
	std::fs::write(project.join(".codex/config.toml"), "[realtime]\nvoice = \"maple\"\n").unwrap();
	std::fs::write(home.join("config.toml"), format!(
		"model = \"gpt-5.6-sol\"\n[realtime]\nvoice = \"juniper\"\n[projects.{:?}]\ntrust_level = \"trusted\"\n", project.to_str().unwrap())).unwrap();
	let (client, mut child) = native(&home).await;
	let first = client.realtime_voice_settings(project.to_str().unwrap()).await.unwrap();
	assert_eq!(first.effective.as_deref(), Some("maple"));
	assert_eq!(first.preference.as_deref(), Some("juniper"));
	assert!(first.voices.iter().any(|v| v == "cove"));
	let (peer, mut peer_child) = native(&home).await;
	let peer_before = peer.realtime_voice_settings(project.to_str().unwrap()).await.unwrap();
	let saved = client.write_realtime_voice(&first, "cove").await.unwrap();
	assert_eq!(saved.preference.as_deref(), Some("cove"));
	assert_eq!(saved.effective.as_deref(), Some("maple"));
	assert_ne!(saved.fingerprint(), first.fingerprint());
	assert!(matches!(
		peer.write_realtime_voice(&peer_before, "sol").await,
		Err(ClientError::Remote(_))
	));
	peer_child.kill().await.unwrap();
	peer_child.wait().await.unwrap();
	assert!(matches!(
		client.write_realtime_voice(&first, "sol").await,
		Err(ClientError::Remote(_))
	));
	assert!(matches!(
		client.write_realtime_voice(&saved, "not-advertised").await,
		Err(ClientError::InvalidFrame)
	));
	child.kill().await.unwrap();
	child.wait().await.unwrap();
	let (client, mut child) = native(&home).await;
	assert!(matches!(
		client.write_realtime_voice(&saved, "sol").await,
		Err(ClientError::InvalidFrame)
	));
	let cold = client.realtime_voice_settings(project.to_str().unwrap()).await.unwrap();
	assert_eq!(cold.preference.as_deref(), Some("cove"));
	assert_eq!(cold.effective.as_deref(), Some("maple"));
	let global = client.realtime_voice_settings(home.to_str().unwrap()).await.unwrap();
	assert_eq!(global.effective.as_deref(), Some("cove"));
	child.kill().await.unwrap();
	child.wait().await.unwrap();
}

#[test]
fn voice_write_bridge_accepts_only_one_conditional_preference() {
	let valid = json!({"filePath":"/tmp/config.toml","expectedVersion":"v1","reloadUserConfig":false,
		"edits":[{"keyPath":"realtime.voice","value":"juniper","mergeStrategy":"replace"}]});
	assert!(is_realtime_voice_write(&valid));
	for (pointer, value) in [
		("/reloadUserConfig", json!(true)),
		("/filePath", json!("relative")),
		("/expectedVersion", json!(null)),
		("/edits/0/keyPath", json!("sandbox_mode")),
		("/edits/0/value", json!(null)),
		("/edits/0/mergeStrategy", json!("upsert")),
	] {
		let mut changed = valid.clone();
		*changed.pointer_mut(pointer).unwrap() = value;
		assert!(!is_realtime_voice_write(&changed));
	}
	let mut extra = valid.clone();
	extra["edits"].as_array_mut().unwrap().push(valid["edits"][0].clone());
	assert!(!is_realtime_voice_write(&extra));
}

#[tokio::test]
async fn lost_voice_write_reply_does_not_repeat_the_native_edit() {
	use tokio::io::{AsyncBufReadExt as _, BufReader};
	let (local, remote) = tokio::io::duplex(8192);
	let (read, write) = tokio::io::split(local);
	let (client, _events) = AppServerClient::from_io(read, write);
	let observed = NativeVoiceSettings {
		connection: client.outbound.clone(),
		cwd: "/project".into(),
		file: "/home/config.toml".into(),
		version: "reviewed-version".into(),
		voices: vec!["juniper".into()],
		effective: Some("maple".into()),
		preference: Some("maple".into()),
	};
	let server = tokio::spawn(async move {
		let (read, _write) = tokio::io::split(remote);
		let line = BufReader::new(read).lines().next_line().await.unwrap().unwrap();
		let request: Value = serde_json::from_str(&line).unwrap();
		assert_eq!(request["method"], "config/batchWrite");
		assert!(is_realtime_voice_write(&request["params"]));
		assert_eq!(request["params"]["expectedVersion"], "reviewed-version");
		// Disconnect after the write may have reached disk; no receipt is available.
	});
	assert!(client.write_realtime_voice(&observed, "juniper").await.is_err());
	server.await.unwrap();
}
