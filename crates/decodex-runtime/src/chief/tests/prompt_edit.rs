use super::*;
use std::sync::{Arc, Mutex};

type NativeHistory = Arc<Mutex<Vec<Value>>>;
fn history() -> NativeHistory {
	Arc::new(Mutex::new(["prefix","selected","suffix"].iter().map(|id| json!({"id":id,"status":"completed","items":[{"type":"userMessage","id":format!("{id}-input"),"content":[{"type":"text","text":format!("Input {id}"),"text_elements":[]},{"type":"mention","name":"Sample","path":"plugin://sample@test"}]}]})).collect()))
}
fn transport(
	mode: &'static str,
	history: NativeHistory,
) -> (AppServerClient, tokio::sync::mpsc::UnboundedReceiver<Value>, tokio::task::JoinHandle<()>) {
	let (incoming, frames) = tokio::sync::mpsc::channel(16);
	let (outgoing, mut writes) = tokio::sync::mpsc::channel::<Value>(16);
	let (client, _events) = AppServerClient::from_framed(1, frames, outgoing).unwrap();
	let (log, reads) = tokio::sync::mpsc::unbounded_channel();
	let server = tokio::spawn(async move {
		let mut selected_reads = 0;
		while let Some(request) = writes.recv().await {
			log.send(request.clone()).unwrap();
			assert_eq!(request["params"]["threadId"], "opaque thread/1");
			let method = request["method"].as_str().unwrap();
			if method == "thread/revert" {
				assert_eq!(
					request["params"],
					json!({"threadId":"opaque thread/1","beforeTurnId":"selected"})
				);
				if mode != "validation" {
					history.lock().unwrap().truncate(1);
				}
				if mode == "lost-reply" {
					return;
				}
				if mode == "validation" || mode == "internal-after-commit" {
					incoming.send(Ok(json!({"id":request["id"],"error":{"code":if mode=="validation" {-32602} else {-32603},"message":"fixture"}}))).await.unwrap();
					continue;
				}
			}
			let result = match method {
				"thread/read" | "thread/revert" =>
					json!({"thread":{"id":"opaque thread/1","historyMode":"paginated","turns":[]}}),
				"thread/turns/list" => {
					let mut turns = history.lock().unwrap().clone();
					turns.reverse();
					turns.truncate(request["params"]["limit"].as_u64().unwrap() as usize);
					if request["params"]["itemsView"] != "full" {
						for t in &mut turns {
							t["items"] = json!([]);
						}
					}
					json!({"data":turns,"nextCursor":null})
				},
				"thread/items/list" => {
					let turn = request["params"]["turnId"].as_str().unwrap();
					let mut items =
						history.lock().unwrap().iter().find(|t| t["id"] == turn).unwrap()["items"]
							.as_array()
							.unwrap()
							.clone();
					if turn == "selected" {
						selected_reads += 1;
						if mode == "changed-content" && selected_reads > 1 {
							items[0]["content"][0]["text"] = json!("Different persisted input");
						}
					}
					json!({"data":items.into_iter().map(|item|json!({"turnId":turn,"item":item})).collect::<Vec<_>>(),"nextCursor":null})
				},
				other => panic!("unexpected native effect: {other}"),
			};
			if incoming.send(Ok(json!({"id":request["id"],"result":result}))).await.is_err() {
				break;
			}
		}
	});
	(client, reads, server)
}

#[tokio::test]
async fn prompt_edit_submits_once_and_recovers_lost_or_postcommit_replies_without_replay() {
	for mode in ["success", "lost-reply", "internal-after-commit", "validation"] {
		let (mut chief, _old_reads, _directory) = fixture().await;
		chief.start_chief("chief", "Start").await.unwrap();
		complete(&mut chief, "chief").await;
		let native = history();
		let (client, mut reads, server) = transport(mode, native.clone());
		chief.client = client;
		let review = chief
			.prepare_prompt_edit("chief", "opaque thread/1", "selected", "selected-input", "review")
			.await
			.unwrap()
			.unwrap();
		let content = review.evidence().content.clone();
		assert_eq!(review.evidence().turn_ids, vec!["prefix", "selected", "suffix"]);
		let receipt = chief.confirm_prompt_edit(review).await.unwrap();
		let requests = std::iter::from_fn(|| reads.try_recv().ok()).collect::<Vec<_>>();
		assert_eq!(requests.iter().filter(|r| r["method"] == "thread/revert").count(), 1);
		assert_eq!(receipt.attempt.content, content);
		if mode == "validation" {
			assert_eq!(receipt.state, "not_submitted");
			assert_eq!(native.lock().unwrap().len(), 3);
		} else {
			assert!(chief.store.begin_chief_dispatch("chief".into()).await.is_err());
			assert_eq!(receipt.state, if mode == "lost-reply" { "reserved" } else { "applied" });
			let store = chief.store.clone();
			let config = chief.config.clone();
			drop(chief);
			let (fresh, mut recovery_reads, recovery_server) = transport("success", native.clone());
			let mut recovered = ChiefCoordinator::new(store, fresh, config).unwrap();
			let receipt =
				recovered.recover_prompt_edit("chief", "opaque thread/1").await.unwrap().unwrap();
			assert_eq!(receipt.state, "applied");
			assert_eq!(receipt.attempt.content, content);
			assert!(
				recovered.store.begin_chief_dispatch("chief".into()).await.is_err(),
				"draft acknowledgement remains required"
			);
			assert!(
				std::iter::from_fn(|| recovery_reads.try_recv().ok())
					.all(|r| r["method"] != "thread/revert")
			);
			recovery_server.abort();
			let _ = recovery_server.await;
		}
		server.abort();
		let _ = server.await;
	}
}

#[tokio::test]
async fn prompt_edit_revalidates_content_before_reservation_or_native_mutation() {
	let (mut chief, _old_reads, _directory) = fixture().await;
	chief.start_chief("chief", "Start").await.unwrap();
	complete(&mut chief, "chief").await;
	let native = history();
	let (client, mut reads, server) = transport("changed-content", native);
	chief.client = client;
	let review = chief
		.prepare_prompt_edit("chief", "opaque thread/1", "selected", "selected-input", "review")
		.await
		.unwrap()
		.unwrap();
	assert!(chief.confirm_prompt_edit(review).await.is_err());
	assert!(
		chief
			.store
			.chief_prompt_edit_receipt("chief".into(), "opaque thread/1".into())
			.await
			.unwrap()
			.is_none()
	);
	assert!(std::iter::from_fn(|| reads.try_recv().ok()).all(|r| r["method"] != "thread/revert"));
	server.abort();
	let _ = server.await;
}

#[tokio::test]
async fn canonical_input_queue_preserves_parts_and_settings_without_sending_the_preview() {
	let (mut chief, _old_reads, _directory) = fixture().await;
	chief.start_chief("chief", "Start").await.unwrap();
	complete(&mut chief, "chief").await;
	let (client, _reads, server) = transport("success", history());
	chief.client = client;
	let review = chief
		.prepare_prompt_edit(
			"chief",
			"opaque thread/1",
			"selected",
			"selected-input",
			"canonical-review",
		)
		.await
		.unwrap()
		.unwrap();
	let receipt = chief.confirm_prompt_edit(review).await.unwrap();
	let content = vec![
		json!({"type":"text","text":"Use $skill","text_elements":[{"byteRange":{"start":4,"end":10},"placeholder":"skill"}]}),
		json!({"type":"image","url":format!("data:image/png;base64,{}", "A".repeat(100000)),"detail":"original"}),
		json!({"type":"skill","name":"skill","path":"/fixture/SKILL.md"}),
	];
	let saved = chief
		.store
		.retain_chief_prompt_input(
			"chief".into(),
			"opaque thread/1".into(),
			receipt.id,
			content.clone(),
		)
		.await
		.unwrap();
	let event = decodex_database::EnqueueChiefEvent {
		source_event_id: "canonical-send".into(), work_item_id: "chief".into(), event_kind: "user_message".into(),
		payload: json!({"text":"BOUNDED PREVIEW ONLY","source":"user","options":{
			"canonicalInput":{"id":saved.id,"threadId":saved.thread,"editReceiptId":receipt.id,"sha256":saved.sha256},
			"execution":{"reasoning_effort":"high"},"attachments":[],"taskReferences":[]
		}}).to_string(),
	};
	assert!(
		chief.store.enqueue_chief_event(event.clone()).await.is_err(),
		"handback must be acknowledged"
	);
	assert!(chief.store.release_chief_prompt_edit_draft(receipt.id, None).await.unwrap());
	let queued = chief.store.enqueue_chief_event(event.clone()).await.unwrap();
	assert!(queued.payload.len() < 2048);
	assert_eq!(chief.store.enqueue_chief_event(event.clone()).await.unwrap().id, queued.id);
	let item = chief.store.get_chief_work_item("chief".into()).await.unwrap();
	let (params, _) =
		chief.dispatch_input(&item, "BOUNDED PREVIEW ONLY", &[queued.id], false).await.unwrap();
	assert_eq!(params["input"], json!(content));
	assert_eq!(params["effort"], "high");
	assert_eq!(params["turnTrigger"], "user");
	let mut crossed = item.clone();
	crossed.codex_thread_id = Some("replacement-thread".into());
	assert!(chief.dispatch_input(&crossed, "preview", &[queued.id], false).await.is_err());
	let mut foreign = event;
	foreign.source_event_id = "foreign".into();
	let mut payload: Value = serde_json::from_str(&foreign.payload).unwrap();
	payload["options"]["canonicalInput"]["sha256"] = json!("0".repeat(64));
	foreign.payload = payload.to_string();
	assert!(chief.store.enqueue_chief_event(foreign).await.is_err());
	server.abort();
	let _ = server.await;
}
