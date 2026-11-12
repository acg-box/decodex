use tokio::io;
#[cfg(test)] use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};

use crate::agent_message_history::{
	self as history, AppServerClient, Exchange, Recent, Value, excerpts,
};

fn render(exchanges: &[Exchange]) -> String {
	excerpts::render_budget(exchanges, 24 * 1024)
}

#[test]
fn native_answer_display_applies_to_history_and_recap() {
	for display in [
		serde_json::json!(null),
		serde_json::json!({"version":1,"mode":"hide","replacement":{"kind":"wellbeingPromptHelpline"}}),
		serde_json::json!({"version":1,"mode":"block","reason":{"kind":"protection","protectionType":"omnimod"}}),
		serde_json::json!({"version":2,"mode":"show","trustedContacts":"skip"}),
		serde_json::json!({"version":1,"mode":"unknown"}),
	] {
		let item = serde_json::json!({"type":"agentMessage","id":"answer","text":"RESTRICTED_ANSWER",
			"moderation":{"moderated":true,"display":display}});
		assert!(history::visible(std::slice::from_ref(&item)).unwrap().is_empty());
		let projected =
			crate::agent::timeline::ordinary(&serde_json::json!({"turnId":"turn","item":item}))
				.unwrap();
		assert!(!serde_json::to_string(&projected).unwrap().contains("RESTRICTED_ANSWER"));
	}
	for moderation in [
		serde_json::json!(null),
		serde_json::json!({"moderated":false,"display":null}),
		serde_json::json!({"moderated":true,"display":{"version":1,"mode":"show","trustedContacts":"skip"}}),
	] {
		let item = serde_json::json!({"type":"agentMessage","text":"Allowed answer","moderation":moderation});
		assert_eq!(history::visible(&[item]).unwrap()[0].text, "Allowed answer");
	}
}

#[test]
fn latest_eight_answered_exchanges_and_pending_correction_exclude_tool_and_reasoning_text() {
	let mut items = Vec::new();

	for n in 0..10 {
		items.push(
			serde_json::json!({"type":"userMessage","content":[{"type":"text","text":format!("question-{n}")}]}),
		);
		items.push(serde_json::json!({"type":"functionCallOutput","text":"PRIVATE_TOOL"}));
		items.push(serde_json::json!({"type":"reasoning","text":"PRIVATE_REASONING"}));
		items.push(serde_json::json!({"type":"agentMessage","text":format!("answer-{n}")}));
	}

	items.push(
		serde_json::json!({"type":"userMessage","content":[{"type":"text","text":"latest correction"}]}),
	);
	items.push(
		serde_json::json!({"type":"userMessage","content":[{"type":"text","text":"steer detail"}]}),
	);

	let mut messages = history::visible(&items).expect("visible fixture");

	messages.reverse();

	let result = history::finish(history::select(&messages), Some("latest".into()))
		.expect("bounded history");

	assert!(!render(&result.exchanges).contains("question-1"));
	assert!(render(&result.exchanges).contains("question-2"));
	assert!(
		render(&result.exchanges)
			.contains("Pending user request: latest correction\n\nsteer detail")
	);
	assert!(!render(&result.exchanges).contains("PRIVATE_"));
}

#[test]
fn media_is_described_without_payloads_and_internal_voice_handoff_is_not_summarized() {
	let image = serde_json::json!({"type":"userMessage","content":[{"type":"image","url":"data:PRIVATE_IMAGE"}]});
	let projected = history::visible(&[image]).expect("visible media placeholder");

	assert_eq!(projected[0].text, "[Image attachment]");

	let handoff = serde_json::json!({"type":"userMessage","content":[{"type":"text","text":"<realtime_delegation><input>PRIVATE_INTERNAL_HANDOFF</input></realtime_delegation>","textElements":[]}]});

	assert!(history::visible(&[handoff]).is_err());
}

#[test]
fn voice_history_skips_internal_handoff_but_keeps_native_task_output_and_anchor() {
	let items = serde_json::json!([
	 {"type":"userMessage","content":[{"type":"text","text":"<realtime_delegation><input>PRIVATE_INTERNAL_HANDOFF</input></realtime_delegation>"}]},
	 {"type":"agentMessage","text":"The task was tested, not installed."}
	]);
	let mut recent = Recent { voice: true, ..Default::default() };

	recent.push(&serde_json::json!({"id":"native-output","status":"completed"}), &items).unwrap();

	let history = recent.finish().unwrap();
	let rendered = render(&history.exchanges);

	assert!(rendered.contains("The task was tested, not installed."));
	assert!(rendered.contains("native-output"));
	assert!(!rendered.contains("PRIVATE_INTERNAL_HANDOFF"));
	assert!(!rendered.contains("User:"));
}

#[tokio::test]
async fn native_turn_and_item_pages_are_joined_without_model_requests() {
	let (local, remote) = io::duplex(16_384);
	let (read, write) = io::split(local);
	let (client, _events) = AppServerClient::from_io(read, write);
	let server = tokio::spawn(async move {
		let replies = [
			(
				"thread/read",
				serde_json::json!({"thread":{"id":"source","historyMode":"paginated"}}),
			),
			("thread/turns/list", serde_json::json!({"data":[],"nextCursor":"older"})),
			(
				"thread/turns/list",
				serde_json::json!({"data":[{"id":"turn","status":"completed"}],"nextCursor":null}),
			),
			(
				"thread/items/list",
				serde_json::json!({"data":[{"turnId":"turn","item":{"id":"user","type":"userMessage","content":[{"type":"text","text":"Keep the current goal"}]}}],"nextCursor":"more"}),
			),
			(
				"thread/items/list",
				serde_json::json!({"data":[{"turnId":"turn","item":{"id":"answer","type":"agentMessage","text":"Tested; not deployed"}}],"nextCursor":null}),
			),
		];
		let (read, mut write) = io::split(remote);
		let mut lines = BufReader::new(read).lines();

		for (index, (method, result)) in replies.into_iter().enumerate() {
			let request: Value =
				serde_json::from_str(&lines.next_line().await.expect("read").expect("request"))
					.expect("JSON");

			assert_eq!(request["method"], method);
			assert_eq!(request["params"]["threadId"], "source");

			if index == 2 {
				assert_eq!(request["params"]["cursor"], "older");
			}
			if index == 4 {
				assert_eq!(request["params"]["cursor"], "more");
			}

			write
				.write_all(
					format!("{}\n", serde_json::json!({"id":request["id"],"result":result}))
						.as_bytes(),
				)
				.await
				.expect("reply");
		}
	});
	let prepared = super::read(&client, "source", false).await.expect("complete native pages");

	assert_eq!(prepared.latest_turn.as_deref(), Some("turn"));
	assert!(render(&prepared.exchanges).contains("Keep the current goal"));
	assert!(render(&prepared.exchanges).contains("Tested; not deployed"));

	server.await.expect("fixture");
}

#[tokio::test]
async fn assistant_history_after_voice_keeps_public_exchange_without_internal_delegation() {
	let (local, remote) = io::duplex(16_384);
	let (read, write) = io::split(local);
	let (client, _events) = AppServerClient::from_io(read, write);
	let server = tokio::spawn(async move {
		let (read, mut write) = io::split(remote);
		let mut lines = BufReader::new(read).lines();
		for (method, result) in [
			("thread/read", serde_json::json!({"thread":{"id":"source","historyMode":"legacy"}})),
			(
				"thread/read",
				serde_json::json!({"thread":{"id":"source","turns":[{"id":"voice-task","status":"completed","items":[
					{"type":"userMessage","content":[{"type":"text","text":"Keep the current instructions"}]},
					{"type":"userMessage","content":[{"type":"text","text":"<realtime_delegation><input>PRIVATE_INTERNAL_HANDOFF</input></realtime_delegation>"}]},
					{"type":"agentMessage","phase":"commentary","text":"PRIVATE_INTERMEDIATE_TEXT"},
					{"type":"agentMessage","phase":"final_answer","text":"Verified public result"}
				]}]}}),
			),
		] {
			let request: Value =
				serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
			assert_eq!(request["method"], method);
			write
				.write_all(
					format!("{}\n", serde_json::json!({"id":request["id"],"result":result}))
						.as_bytes(),
				)
				.await
				.unwrap();
		}
	});
	let prepared = history::read_for_assistant(&client, "source").await.unwrap();
	let text = render(&prepared.exchanges);
	assert!(text.contains("Keep the current instructions"));
	assert!(text.contains("Verified public result"));
	assert!(text.contains("voice-task"));
	assert!(!text.contains("PRIVATE_"));
	server.await.unwrap();
}
