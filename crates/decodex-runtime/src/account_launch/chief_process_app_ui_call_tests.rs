//! Real store plus native wire proves review/confirmation dispatches at most once.
use super::*;
use crate::chief_app_ui_call::{execute, read};
use decodex_protocol::{ChiefAppUiCall, ChiefAppUiCallReview, EntityId, WireText};
use serde_json::{Value, json};
use std::sync::{
	Arc,
	atomic::{AtomicUsize, Ordering},
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

async fn server(remote: tokio::io::DuplexStream, calls: Arc<AtomicUsize>, lost: bool) {
	let (reader, mut writer) = tokio::io::split(remote);
	let mut lines = BufReader::new(reader).lines();
	while let Some(line) = lines.next_line().await.unwrap() {
		let request: Value = serde_json::from_str(&line).unwrap();
		let result = match request["method"].as_str().unwrap() {
			"thread/read" => json!({"thread":{"id":"thread","historyMode":"paginated"}}),
			"thread/turns/list" => json!({"data":[{"id":"turn"}],"nextCursor":null}),
			"thread/items/list" =>
				json!({"data":[{"turnId":"turn","item":{"id":"widget","type":"mcpToolCall","server":"widget","mcpAppUi":{"resourceUri":"ui://fixture/view"}}}],"nextCursor":null}),
			"mcpServerStatus/list" =>
				json!({"data":[{"name":"widget","runtimeStatus":"connected","tools":{"counter":{"name":"counter","title":"Counter","inputSchema":{"type":"object"}}},"authStatus":"unsupported","resources":[],"resourceTemplates":[]}],"nextCursor":null}),
			"mcpServer/tool/call" => {
				calls.fetch_add(1, Ordering::SeqCst);
				assert_eq!(
					request["params"],
					json!({"threadId":"thread","server":"widget","tool":"counter","arguments":{"value":7}})
				);
				if lost {
					return;
				}
				json!({"content":[],"structuredContent":{"value":7},"_meta":{"view":"retained"}})
			},
			other => panic!("unexpected native operation {other}"),
		};
		writer
			.write_all(format!("{}\n", json!({"id":request["id"],"result":result})).as_bytes())
			.await
			.unwrap();
	}
}

#[tokio::test]
async fn app_ui_confirmation_preserves_exact_intent_and_lost_reply_without_replay() {
	for mode in ["success", "lost", "source-changed"] {
		let lost = mode == "lost";
		let home = tempfile::tempdir().unwrap();
		let (local, remote) = tokio::io::duplex(65536);
		let (reader, writer) = tokio::io::split(local);
		let (client, _events) = AppServerClient::from_io(reader, writer);
		let calls = Arc::new(AtomicUsize::new(0));
		let backend = tokio::spawn(server(remote, calls.clone(), lost));
		let owner = OwnedReviewer::new(home.path(), &client, "thread", "turn").await;
		let call = ChiefAppUiCall {
			work_id: EntityId::new("root").unwrap(),
			thread_id: EntityId::new("thread").unwrap(),
			turn_id: EntityId::new("turn").unwrap(),
			item_id: EntityId::new("widget").unwrap(),
			source_fingerprint: crate::chief::timeline::app_ui::source_fingerprint(&owner.key),
			operation_id: EntityId::new("call-1").unwrap(),
			tool: WireText::new("counter").unwrap(),
			arguments: json!({"value":7}),
		};
		let ChiefAppUiCallReview::Available { review_token, pending_operation, .. } =
			read(&owner.store, || async { Some(owner.source(&owner.key)) }, &call).await
		else {
			panic!("review")
		};
		assert!(pending_operation.is_none());
		assert_eq!(calls.load(Ordering::SeqCst), 0);
		let mut changed = call.clone();
		changed.arguments = json!({"value":8});
		assert!(
			execute(
				&owner.store,
				|| async { Some(owner.source(&owner.key)) },
				&changed,
				&review_token
			)
			.await
			.is_err()
		);
		assert_eq!(calls.load(Ordering::SeqCst), 0);
		assert!(
			owner
				.store
				.chief_app_ui_call_receipt("root".into(), "call-1".into())
				.await
				.unwrap()
				.is_none()
		);
		let observations = AtomicUsize::new(0);
		let outcome = execute(
			&owner.store,
			|| {
				let changed =
					mode == "source-changed" && observations.fetch_add(1, Ordering::SeqCst) >= 2;
				std::future::ready((!changed).then(|| owner.source(&owner.key)))
			},
			&call,
			&review_token,
		)
		.await;
		let expected_calls = usize::from(mode != "source-changed");
		assert_eq!(outcome.is_ok(), mode == "success");
		assert_eq!(calls.load(Ordering::SeqCst), expected_calls);
		let reopened = SqliteStore::open(&owner.root.paths()).unwrap();
		let receipt = reopened
			.chief_app_ui_call_receipt("root".into(), "call-1".into())
			.await
			.unwrap()
			.unwrap();
		assert_eq!(receipt.attempt.arguments, call.arguments);
		assert_eq!(
			receipt.state,
			match mode {
				"lost" => "unknown",
				"source-changed" => "unsent",
				_ => "completed",
			}
		);
		if mode == "success" {
			assert_eq!(receipt.result.unwrap()["structuredContent"]["value"], 7);
		}
		assert!(
			execute(&reopened, || std::future::ready(None), &call, &review_token).await.is_err()
		);
		assert_eq!(calls.load(Ordering::SeqCst), expected_calls);
		backend.abort();
		let _ = backend.await;
	}
}
