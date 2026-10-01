use super::*;
use crate::agent_usage_estimate::SourceKey;
use decodex_codex::app_server_client::AppServerClient;
use decodex_core::{AccountId, ProcessGenerationId};
use std::sync::{
	Arc,
	atomic::{AtomicUsize, Ordering},
};
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};

fn source(client: AppServerClient, revision: usize) -> Source {
	Source {
		client,
		key: SourceKey {
			generation: ProcessGenerationId::new("10000000-0000-4000-8000-000000000001").unwrap(),
			account: AccountId::new("30000000-0000-4000-8000-000000000003").unwrap(),
			revision: revision as i64,
			history_revision: 0,
			thread: "thread".into(),
			work: "work".into(),
		},
	}
}

fn fixture(
	fail_readback: bool,
) -> (AppServerClient, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
	let (local, remote) = tokio::io::duplex(8_192);
	let (read, write) = tokio::io::split(local);
	let (client, _events) = AppServerClient::from_io(read, write);
	let writes = Arc::new(AtomicUsize::new(0));
	let count = writes.clone();
	let task = tokio::spawn(async move {
		let (read, mut write) = tokio::io::split(remote);
		let mut lines = BufReader::new(read).lines();

		while let Some(line) = lines.next_line().await.unwrap() {
			let request: serde_json::Value = serde_json::from_str(&line).unwrap();
			let saved = count.load(Ordering::SeqCst) > 0;
			let version = if saved { "v2" } else { "v1" };
			let mut response = match request["method"].as_str().unwrap() {
				"thread/read" => json!({"result":{"thread":{"id":"thread","cwd":"/project"}}}),
				"config/read" if saved && fail_readback =>
					json!({"error":{"code":-32_603,"message":"read unavailable"}}),
				"config/read" => json!({"result":{"config":{"web_search":"live"},"layers":[{
					"name":{"type":"user","file":"/home/config.toml"},"version":version,
					"config":{"web_search":if saved {"indexed"} else {"cached"}}
				}]}}),
				"configRequirements/read" =>
					json!({"result":{"requirements":{"allowedWebSearchModes":["disabled","cached","indexed","future-mode"]}}}),
				"config/batchWrite" => {
					assert_eq!(request["params"]["expectedVersion"], version);
					assert_eq!(request["params"]["reloadUserConfig"], false);
					assert_eq!(
						request["params"]["edits"],
						json!([{"keyPath":"web_search","value":"indexed","mergeStrategy":"replace"}])
					);
					assert!(decodex_codex::app_server_client::is_search_mode_write(
						&request["params"]
					));

					count.fetch_add(1, Ordering::SeqCst);

					json!({"result":{"filePath":"/home/config.toml","version":"v2","status":"okOverridden"}})
				},
				other => panic!("unexpected native action {other}"),
			};

			response["id"] = request["id"].clone();

			if write.write_all(format!("{response}\n").as_bytes()).await.is_err() {
				break;
			}
		}
	});

	(client, writes, task)
}

#[tokio::test]
async fn search_settings_bind_source_version_and_report_uncertain_readback() {
	for fail_readback in [false, true] {
		let (client, writes, task) = fixture(fail_readback);
		let read_source = || std::future::ready(Some(source(client.clone(), 1)));
		let AgentSearchSettingsResult::Available { review_token, modes, .. } =
			read(read_source).await
		else {
			panic!("settings")
		};

		assert_eq!(
			modes.iter().map(WireText::as_str).collect::<Vec<_>>(),
			vec!["disabled", "cached", "indexed"]
		);
		assert!(
			write(read_source, review_token.as_str(), "live").await.is_err(),
			"native requirements exclude live mode"
		);
		assert!(
			write(
				|| std::future::ready(Some(source(client.clone(), 2))),
				review_token.as_str(),
				"indexed"
			)
			.await
			.is_err()
		);
		assert_eq!(writes.load(Ordering::SeqCst), 0);

		let result = write(read_source, review_token.as_str(), "indexed").await;

		assert_eq!(result.is_ok(), !fail_readback);
		assert_eq!(writes.load(Ordering::SeqCst), 1);
		assert!(write(read_source, review_token.as_str(), "indexed").await.is_err());
		assert_eq!(writes.load(Ordering::SeqCst), 1);

		if !fail_readback {
			let AgentSearchSettingsResult::Available { effective, preference, .. } =
				read(read_source).await
			else {
				panic!("readback")
			};

			assert_eq!(effective.unwrap().as_str(), "live");
			assert_eq!(preference.unwrap().as_str(), "indexed");
		}

		task.abort();
	}
}
