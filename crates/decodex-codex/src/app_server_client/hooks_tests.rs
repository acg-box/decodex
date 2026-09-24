use super::*;
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};

fn review() -> HookSettingsReview {
	HookSettingsReview {
		version: "version-one".into(),
		inventory: json!({"cwd":"/repo","warnings":["partial"],"errors":[],"hooks":[{"key":"plugin.\"quoted\"\\path","currentHash":"hash-one","isManaged":false,"enabled":true,"trustStatus":"untrusted","eventName":"UserPromptSubmit","handlerType":"command","command":"echo fixture","sourcePath":"/repo/hooks.json"}]}),
	}
}
#[test]
fn edits_preserve_exact_keys_and_cannot_expand_into_other_config() {
	let review = review();
	validate_inventory(&review.inventory).unwrap();
	let key = review.inventory["hooks"][0]["key"].as_str().unwrap();
	let params = review.change(key, HookSettingsChange::Trust).unwrap();
	assert_eq!(params["edits"][0]["value"], "hash-one");
	assert!(is_hook_settings_write(&params));
	for (field, value) in [
		("filePath", json!("/other")),
		("reloadUserConfig", json!(false)),
		("expectedVersion", Value::Null),
	] {
		let mut invalid = params.clone();
		invalid[field] = value;
		assert!(!is_hook_settings_write(&invalid));
	}
	for path in [
		"hooks.state.unquoted.enabled",
		"hooks.state.\"key\".bypass_hook_trust",
		"model",
		"hooks.state.\"key\".enabled.extra",
	] {
		let mut invalid = params.clone();
		invalid["edits"][0]["keyPath"] = json!(path);
		assert!(!is_hook_settings_write(&invalid));
	}
	let mut managed = review.clone();
	managed.inventory["hooks"][0]["isManaged"] = json!(true);
	assert!(managed.change(key, HookSettingsChange::Enabled(false)).is_err());
	let mut duplicate = review.inventory.clone();
	duplicate["hooks"].as_array_mut().unwrap().push(review.inventory["hooks"][0].clone());
	assert!(validate_inventory(&duplicate).is_err());
	assert_eq!(review.inventory["warnings"], json!(["partial"]));
}

#[tokio::test]
async fn config_write_preserves_override_and_does_not_replay_lost_reply() {
	for status in [Some("okOverridden"), None] {
		let (local, remote) = tokio::io::duplex(8192);
		let (r, w) = tokio::io::split(local);
		let (client, _events) = AppServerClient::from_io(r, w);
		let guard = client.thread_settings_guard("task").unwrap();
		let review = review();
		let params = review
			.change(
				review.inventory["hooks"][0]["key"].as_str().unwrap(),
				HookSettingsChange::Enabled(false),
			)
			.unwrap();
		let backend = tokio::spawn(async move {
			let (r, mut w) = tokio::io::split(remote);
			let mut lines = BufReader::new(r).lines();
			let request: Value =
				serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
			assert_eq!(request["method"], "config/batchWrite");
			assert_eq!(request["params"]["expectedVersion"], "version-one");
			assert_eq!(request["params"]["edits"][0]["value"], false);
			if let Some(status) = status {
				w.write_all(format!("{}\n",json!({"id":request["id"],"result":{"status":status,"version":"two","filePath":"/home/config.toml"}})).as_bytes()).await.unwrap();
				assert!(
					tokio::time::timeout(std::time::Duration::from_millis(50), lines.next_line())
						.await
						.is_err()
				);
			}
		});
		let result = client.write_hook_settings(params, guard).await;
		if status.is_some() {
			assert_eq!(result.unwrap(), HookSettingsWrite::Overridden);
		} else {
			assert!(result.is_err());
		}
		backend.await.unwrap();
	}
}
