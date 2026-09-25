use super::*;
use crate::chief_usage_estimate::SourceKey;
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
	let (local, remote) = tokio::io::duplex(8192);
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
					json!({"error":{"code":-32603,"message":"read unavailable"}}),
				"config/read" =>
					json!({"result":{"config":{"realtime":{"voice":"maple"}},"layers":[{
						"name":{"type":"user","file":"/home/config.toml"},"version":version,
						"config":{"realtime":{"voice":if saved {"juniper"} else {"cove"}}}
					}]}}),
				"thread/realtime/listVoices" =>
					json!({"result":{"voices":{"v1":["juniper","maple","cove"],"defaultV1":"cove"}}}),
				"config/batchWrite" => {
					assert_eq!(request["params"]["expectedVersion"], version);
					assert_eq!(request["params"]["reloadUserConfig"], false);
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
async fn voice_settings_bind_source_version_and_report_uncertain_readback() {
	for fail_readback in [false, true] {
		let (client, writes, task) = fixture(fail_readback);
		let read_source = || std::future::ready(Some(source(client.clone(), 1)));
		let ChiefVoiceSettingsResult::Available { review_token, .. } = read(read_source).await
		else {
			panic!("settings")
		};
		assert!(
			write(
				|| std::future::ready(Some(source(client.clone(), 2))),
				review_token.as_str(),
				"juniper"
			)
			.await
			.is_err()
		);
		assert_eq!(writes.load(Ordering::SeqCst), 0);
		let result = write(read_source, review_token.as_str(), "juniper").await;
		assert_eq!(result.is_ok(), !fail_readback);
		assert_eq!(writes.load(Ordering::SeqCst), 1);
		assert!(write(read_source, review_token.as_str(), "juniper").await.is_err());
		assert_eq!(writes.load(Ordering::SeqCst), 1);
		if !fail_readback {
			let ChiefVoiceSettingsResult::Available { effective, preference, .. } =
				read(read_source).await
			else {
				panic!("readback")
			};
			assert_eq!(effective.unwrap().as_str(), "maple");
			assert_eq!(preference.unwrap().as_str(), "juniper");
		}
		task.abort();
	}
}

#[tokio::test]
async fn voice_settings_discard_observation_when_source_changes_during_read() {
	let (client, writes, task) = fixture(false);
	let calls = AtomicUsize::new(0);
	let result = read(|| {
		std::future::ready(Some(source(client.clone(), calls.fetch_add(1, Ordering::SeqCst))))
	})
	.await;
	assert_eq!(result, ChiefVoiceSettingsResult::Unavailable);
	assert_eq!(writes.load(Ordering::SeqCst), 0);
	task.abort();
}

#[tokio::test]
async fn replaced_connection_cannot_reuse_review_or_publish_old_observation() {
	let (first, first_writes, first_task) = fixture(false);
	let (second, second_writes, second_task) = fixture(false);
	let before = || std::future::ready(Some(source(first.clone(), 1)));
	let ChiefVoiceSettingsResult::Available { review_token, .. } = read(before).await else {
		panic!("voice review")
	};
	assert!(
		write(
			|| std::future::ready(Some(source(second.clone(), 1))),
			review_token.as_str(),
			"juniper"
		)
		.await
		.is_err()
	);
	let calls = AtomicUsize::new(0);
	let state = read(|| {
		let client =
			if calls.fetch_add(1, Ordering::SeqCst) == 0 { first.clone() } else { second.clone() };
		std::future::ready(Some(source(client, 1)))
	})
	.await;
	assert_eq!(state, ChiefVoiceSettingsResult::Unavailable);
	assert_eq!(first_writes.load(Ordering::SeqCst), 0);
	assert_eq!(second_writes.load(Ordering::SeqCst), 0);
	first_task.abort();
	second_task.abort();
}
