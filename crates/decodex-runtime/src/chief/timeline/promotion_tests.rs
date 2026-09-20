use super::*;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

fn page() -> ChiefTimelinePage {
	super::super::project("thread", &json!({"data":[
		{"type":"realtime","position":5,"item":{"type":"bemItemPromoted","id":"first","realtimeSessionId":"voice","turnId":"old-turn","itemId":"message","presentation":{"type":"inlineMarkdown"}}},
		{"type":"realtime","position":6,"item":{"type":"bemItemPromoted","id":"second","realtimeSessionId":"voice","turnId":"old-turn","itemId":"image","presentation":{"type":"wholeItem"}}}
	],"nextCursor":"older","activeRealtimeSessionAtPageStart":"voice"})).unwrap()
}

#[tokio::test]
async fn off_page_references_share_one_native_read_and_keep_exact_media_indices() {
	let (local, remote) = tokio::io::duplex(65536);
	let (reader, writer) = tokio::io::split(local);
	let (client, _events) = AppServerClient::from_io(reader, writer);
	let server = tokio::spawn(async move {
		let (reader, mut writer) = tokio::io::split(remote);
		let mut lines = BufReader::new(reader).lines();
		for include in [false, true] {
			let request: Value =
				serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
			assert_eq!(request["method"], "thread/read");
			assert_eq!(request["params"]["threadId"], "thread");
			assert_eq!(request["params"]["includeTurns"].as_bool().unwrap_or(false), include);
			let result = json!({"thread":{"id":"thread","turns":[{"id":"old-turn","items":[
				{"type":"agentMessage","id":"message","text":"An older exact result"},
				{"type":"dynamicToolCall","id":"image","tool":"image","status":"completed","success":true,"contentItems":[{"type":"inputText","text":"private raw tool output"},{"type":"inputImage","imageUrl":"data:image/png;base64,PRIVATE"}]}
			]}]}});
			writer
				.write_all(format!("{}\n", json!({"id":request["id"],"result":result})).as_bytes())
				.await
				.unwrap();
		}
	});
	let mut page = page();
	enrich(&client, &mut page).await;
	server.await.unwrap();
	assert_eq!(page.next_cursor.as_deref(), Some("older"));
	let Content::Promotion { resolved: Some(first), item_id, .. } = &page.entries[0].content else {
		panic!("missing message")
	};
	assert_eq!(first.text, "An older exact result");
	assert_eq!(item_id, "first");
	let Content::Promotion { resolved: Some(second), .. } = &page.entries[1].content else {
		panic!("missing tool")
	};
	assert_eq!(second.attachments[0].index, 1);
	assert!(second.text.is_empty());
	let serialized = serde_json::to_string(&page).unwrap();
	assert!(!serialized.contains("PRIVATE"));
	assert!(!serialized.contains("private raw tool output"));
}

#[test]
fn exact_reference_rejects_ambiguous_turns_items_and_wrong_thread() {
	let item = json!({"id":"message","type":"agentMessage","text":"Exact"});
	let turn = json!({"id":"old-turn","items":[item]});
	for history in [
		json!({"thread":{"id":"other","turns":[turn]}}),
		json!({"thread":{"id":"thread","turns":[turn,turn]}}),
		json!({"thread":{"id":"thread","turns":[{"id":"old-turn","items":[item,item]}]}}),
		json!({"thread":{"id":"thread","turns":[{"id":"other-turn","items":[item]}]}}),
	] {
		assert!(exact_item(&history, "thread", "old-turn", "message").is_none());
	}
}

#[tokio::test]
async fn loaded_reference_does_not_need_transport_and_failure_preserves_reference() {
	let (local, remote) = tokio::io::duplex(64);
	drop(remote);
	let (reader, writer) = tokio::io::split(local);
	let (client, _events) = AppServerClient::from_io(reader, writer);
	let mut page = page();
	page.entries.push(decodex_protocol::ChiefTimelineEntry {
		position: 7,
		content: ordinary(
			&json!({"turnId":"old-turn","item":{"id":"message","type":"agentMessage","text":"Loaded"}}),
		)
		.unwrap(),
	});
	enrich(&client, &mut page).await;
	assert!(
		matches!(&page.entries[0].content, Content::Promotion { resolved: Some(content), .. } if content.text=="Loaded")
	);
	assert!(
		matches!(&page.entries[1].content, Content::Promotion { resolved: None, agent_item_id, .. } if agent_item_id=="image")
	);
}

#[tokio::test]
async fn duplicate_loaded_identity_never_selects_an_arbitrary_message() {
	let (local, remote) = tokio::io::duplex(64);
	drop(remote);
	let (reader, writer) = tokio::io::split(local);
	let (client, _events) = AppServerClient::from_io(reader, writer);
	let mut page = page();
	for (position, text) in [(7, "First"), (8, "Second"), (9, "Third")] {
		page.entries.push(decodex_protocol::ChiefTimelineEntry {
			position,
			content: ordinary(
				&json!({"turnId":"old-turn","item":{"id":"message","type":"agentMessage","text":text}}),
			)
			.unwrap(),
		});
	}
	enrich(&client, &mut page).await;
	assert!(matches!(&page.entries[0].content, Content::Promotion { resolved: None, .. }));
}

#[tokio::test]
async fn enriched_pages_shrink_on_same_cursor_and_recheck_source_after_reference_read() {
	use std::sync::atomic::{AtomicUsize, Ordering};
	for changed in [false, true] {
		let (local, remote) = tokio::io::duplex(512 * 1024);
		let (reader, writer) = tokio::io::split(local);
		let (client, _events) = AppServerClient::from_io(reader, writer);
		let server = tokio::spawn(async move {
			let (reader, mut writer) = tokio::io::split(remote);
			let mut lines = BufReader::new(reader).lines();
			for limit in if changed { vec![30] } else { vec![30, 15, 7] } {
				for step in 0..4 {
					let request: Value =
						serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
					let result = if step == 1 {
						assert_eq!(request["method"], "thread/timeline/list");
						assert_eq!(request["params"]["limit"], limit);
						assert_eq!(request["params"]["cursor"], "same-cursor");
						let data = (0..limit).map(|n| json!({"type":"realtime","position":n,"item":{"type":"bemItemPromoted","id":format!("p{n}"),"realtimeSessionId":"voice","turnId":"old-turn","itemId":"message","presentation":{"type":"inlineMarkdown"}}})).collect::<Vec<_>>();
						json!({"data":data,"nextCursor":"older","activeRealtimeSessionAtPageStart":"voice"})
					} else {
						assert_eq!(request["method"], "thread/read");
						assert_eq!(request["params"]["threadId"], "thread");
						assert_eq!(
							request["params"]["includeTurns"].as_bool().unwrap_or(false),
							step == 3
						);
						json!({"thread":{"id":"thread","turns":[{"id":"old-turn","items":[{"id":"message","type":"agentMessage","text":"x".repeat(8192)}]}]}})
					};
					writer
						.write_all(
							format!("{}\n", json!({"id":request["id"],"result":result})).as_bytes(),
						)
						.await
						.unwrap();
				}
			}
		});
		let calls = AtomicUsize::new(0);
		let result = super::super::read(
			None,
			|| {
				let revision =
					if changed && calls.fetch_add(1, Ordering::SeqCst) >= 3 { 2 } else { 1 };
				let client = client.clone();
				async move {
					Some(crate::chief_usage_estimate::Source {
						client,
						key: crate::chief_usage_estimate::SourceKey {
							generation: decodex_core::ProcessGenerationId::new(
								"10000000-0000-4000-8000-000000000001",
							)
							.unwrap(),
							account: decodex_core::AccountId::new(
								"30000000-0000-4000-8000-000000000003",
							)
							.unwrap(),
							revision,
							work: "work".into(),
							thread: "thread".into(),
						},
					})
				}
			},
			Some("same-cursor"),
		)
		.await;
		server.await.unwrap();
		if changed {
			assert_eq!(result, decodex_protocol::ChiefTimelineResult::Unavailable);
		} else {
			let decodex_protocol::ChiefTimelineResult::Available { page, .. } = result else {
				panic!("missing adaptive page")
			};
			assert_eq!(page.entries.len(), 7);
			assert!(serde_json::to_vec(&page).unwrap().len() <= 60 * 1024);
		}
	}
}
