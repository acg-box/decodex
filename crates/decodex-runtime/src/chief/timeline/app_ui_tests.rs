use super::*;
use decodex_codex::app_server_client::AppServerClient;
use decodex_core::{AccountId, ProcessGenerationId};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

fn key() -> SourceKey {
	SourceKey {
		generation: ProcessGenerationId::new("10000000-0000-4000-8000-000000000001").unwrap(),
		account: AccountId::new("30000000-0000-4000-8000-000000000003").unwrap(),
		revision: 1,
		history_revision: 0,
		work: "work".into(),
		thread: "thread".into(),
	}
}
fn request() -> ChiefAppUiRequest {
	ChiefAppUiRequest {
		work_id: EntityId::new("work").unwrap(),
		thread_id: EntityId::new("thread").unwrap(),
		turn_id: EntityId::new("turn").unwrap(),
		item_id: EntityId::new("call").unwrap(),
		offset: 0,
		fingerprint: None,
	}
}

#[test]
fn document_continuation_rejects_changed_resource_and_source() {
	let bytes = vec![b'x'; CHIEF_APP_UI_CHUNK_BYTES + 7];
	let mut request = request();
	let Result::Available { fingerprint, bytes: first, .. } =
		chunk(&key(), &request, bytes.clone())
	else {
		panic!("chunk")
	};
	assert_eq!(first.len(), CHIEF_APP_UI_CHUNK_BYTES);
	request.offset = CHIEF_APP_UI_CHUNK_BYTES as u32;
	request.fingerprint = Some(fingerprint);
	assert!(
		matches!(chunk(&key(),&request,bytes.clone()),Result::Available { bytes,.. } if bytes.len()==7)
	);
	let mut changed = bytes.clone();
	changed[0] = b'y';
	assert_eq!(chunk(&key(), &request, changed), Result::Unavailable);
	let mut source = key();
	source.revision += 1;
	assert_eq!(chunk(&source, &request, bytes), Result::Unavailable);
}

#[tokio::test]
async fn widget_document_is_discarded_after_account_process_or_history_changes() {
	for change in ["none", "account", "generation", "history", "revision", "closed"] {
		let (local, remote) = tokio::io::duplex(65536);
		let (reader, writer) = tokio::io::split(local);
		let (client, _events) = AppServerClient::from_io(reader, writer);
		let (release, hold) = tokio::sync::oneshot::channel::<()>();
		let server = tokio::spawn(async move {
			let (reader, mut writer) = tokio::io::split(remote);
			let mut lines = BufReader::new(reader).lines();
			for (method, result) in [
				("thread/read", json!({"thread":{"id":"thread","historyMode":"paginated"}})),
				("thread/turns/list", json!({"data":[{"id":"turn"}],"nextCursor":null})),
				(
					"thread/items/list",
					json!({"data":[{"turnId":"turn","item":{"id":"call","type":"mcpToolCall","server":"widget","mcpAppUi":{"resourceUri":"ui://fixture/view"}}}],"nextCursor":null}),
				),
				(
					"mcpServer/resource/read",
					json!({"contents":[{"uri":"ui://fixture/view","text":"<button>Test</button>","_meta":{"ui":{"csp":{"connectDomains":[]}}}}]}),
				),
			] {
				let request: serde_json::Value =
					serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
				assert_eq!(request["method"], method);
				writer
					.write_all(
						format!("{}\n", json!({"id":request["id"],"result":result})).as_bytes(),
					)
					.await
					.unwrap();
			}
			let _ = hold.await;
		});
		let calls = AtomicUsize::new(0);
		let result = read(
			|| {
				let mut source = key();
				if calls.fetch_add(1, Ordering::SeqCst) > 0 {
					match change {
						"account" =>
							source.account =
								AccountId::new("40000000-0000-4000-8000-000000000004").unwrap(),
						"generation" =>
							source.generation =
								ProcessGenerationId::new("20000000-0000-4000-8000-000000000002")
									.unwrap(),
						"history" => source.history_revision += 1,
						"revision" => source.revision += 1,
						"closed" => return std::future::ready(None),
						_ => {},
					}
				}
				std::future::ready(Some(Source { key: source, client: client.clone() }))
			},
			&request(),
		)
		.await;
		if change == "none" {
			let Result::Available { bytes, .. } = result else { panic!("available") };
			let document: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
			assert_eq!(document["resources"][0]["text"], "<button>Test</button>");
		} else {
			assert_eq!(result, Result::Unavailable, "{change}");
		}
		release.send(()).unwrap();
		server.await.unwrap();
	}
}

#[test]
fn displayed_source_identity_covers_each_owner_revision() {
	let original = key();
	let expected = source_fingerprint(&original);
	for field in ["account", "generation", "history", "revision", "thread", "work"] {
		let mut changed = original.clone();
		match field {
			"account" =>
				changed.account = AccountId::new("40000000-0000-4000-8000-000000000004").unwrap(),
			"generation" =>
				changed.generation =
					ProcessGenerationId::new("20000000-0000-4000-8000-000000000002").unwrap(),
			"history" => changed.history_revision += 1,
			"revision" => changed.revision += 1,
			"thread" => changed.thread = "other-thread".into(),
			"work" => changed.work = "other-work".into(),
			_ => unreachable!(),
		}
		assert_ne!(source_fingerprint(&changed), expected, "{field}");
	}
}
