use std::{
	future,
	sync::{
		Arc,
		atomic::{AtomicUsize, Ordering},
	},
};

use serde_json::Value;
use tokio::{
	io::{self, AsyncBufReadExt as _, AsyncWriteExt as _, BufReader},
	task::JoinHandle,
};

use crate::{
	agent_context_settings::{self, AgentContextFeature, AgentContextSettingsResult, Source},
	agent_usage_estimate::SourceKey,
};
use decodex_codex::app_server_client::{self, AppServerClient};
use decodex_core::{AccountId, ProcessGenerationId};

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
	table: bool,
) -> (AppServerClient, Arc<AtomicUsize>, JoinHandle<()>) {
	let (local, remote) = io::duplex(8_192);
	let (read, write) = io::split(local);
	let (client, _events) = AppServerClient::from_io(read, write);
	let writes = Arc::new(AtomicUsize::new(0));
	let count = writes.clone();
	let task = tokio::spawn(async move {
		let (read, mut write) = io::split(remote);
		let mut lines = BufReader::new(read).lines();

		while let Some(line) = lines.next_line().await.unwrap() {
			let request: Value = serde_json::from_str(&line).unwrap();
			let saved = count.load(Ordering::SeqCst) > 0;
			let version = if saved { "v2" } else { "v1" };
			let enabled = !saved;
			let user = if table {
				serde_json::json!({"enabled":enabled,"model_async_compact_threshold_percent":70})
			} else {
				serde_json::json!(enabled)
			};
			let mut response = match request["method"].as_str().unwrap() {
				"thread/read" =>
					serde_json::json!({"result":{"thread":{"id":"thread","cwd":"/project"}}}),
				"config/read" if saved && fail_readback =>
					serde_json::json!({"error":{"code":-32_603,"message":"read unavailable"}}),
				"config/read" =>
					serde_json::json!({"result":{"config":{"features":{"async_compaction":true,"retain_client_tool_outputs":false}},"layers":[{
						"name":{"type":"user","file":"/home/config.toml"},"version":version,
						"config":{"features":{"async_compaction":user,"retain_client_tool_outputs":false}}
					}]}}),
				"config/batchWrite" => {
					assert_eq!(request["params"]["expectedVersion"], version);
					assert_eq!(request["params"]["reloadUserConfig"], false);
					assert_eq!(
						request["params"]["edits"],
						serde_json::json!([{"keyPath":if table {"features.async_compaction.enabled"} else {"features.async_compaction"},"value":false,"mergeStrategy":"replace"}])
					);
					assert!(app_server_client::is_context_preference_write(&request["params"]));

					count.fetch_add(1, Ordering::SeqCst);

					serde_json::json!({"result":{"filePath":"/home/config.toml","version":"v2","status":"okOverridden"}})
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
async fn context_settings_bind_source_version_and_report_uncertain_readback() {
	for (fail_readback, table) in [(false, false), (false, true), (true, true)] {
		let (client, writes, task) = fixture(fail_readback, table);
		let read_source = || future::ready(Some(source(client.clone(), 1)));
		let AgentContextSettingsResult::Available { review_token, preferences, .. } =
			agent_context_settings::read(read_source).await
		else {
			panic!("settings")
		};

		assert!(preferences[0].effective);
		assert_eq!(preferences[1].preference, Some(false));
		assert!(
			agent_context_settings::write(
				|| future::ready(Some(source(client.clone(), 2))),
				review_token.as_str(),
				AgentContextFeature::AsyncCompaction,
				false
			)
			.await
			.is_err()
		);
		assert_eq!(writes.load(Ordering::SeqCst), 0);

		let result = agent_context_settings::write(
			read_source,
			review_token.as_str(),
			AgentContextFeature::AsyncCompaction,
			false,
		)
		.await;

		assert_eq!(result.is_ok(), !fail_readback);
		assert_eq!(writes.load(Ordering::SeqCst), 1);
		assert!(
			agent_context_settings::write(
				read_source,
				review_token.as_str(),
				AgentContextFeature::AsyncCompaction,
				false
			)
			.await
			.is_err()
		);
		assert_eq!(writes.load(Ordering::SeqCst), 1);

		if !fail_readback {
			let AgentContextSettingsResult::Available { preferences, .. } =
				agent_context_settings::read(read_source).await
			else {
				panic!("readback")
			};

			assert!(preferences[0].effective);
			assert_eq!(preferences[0].preference, Some(false));
		}

		task.abort();
	}
}
