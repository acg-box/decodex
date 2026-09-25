use super::*;

#[test]
fn latest_eight_answered_exchanges_and_pending_correction_exclude_tool_and_reasoning_text() {
	let mut items = Vec::new();
	for n in 0..10 {
		items.push(
			json!({"type":"userMessage","content":[{"type":"text","text":format!("question-{n}")}]}),
		);
		items.push(json!({"type":"functionCallOutput","text":"PRIVATE_TOOL"}));
		items.push(json!({"type":"reasoning","text":"PRIVATE_REASONING"}));
		items.push(json!({"type":"agentMessage","text":format!("answer-{n}")}));
	}
	items
		.push(json!({"type":"userMessage","content":[{"type":"text","text":"latest correction"}]}));
	items.push(json!({"type":"userMessage","content":[{"type":"text","text":"steer detail"}]}));
	let mut messages = visible(&items).expect("visible fixture");
	messages.reverse();
	let result = finish(select(&messages), Some("latest".into())).expect("bounded history");
	assert!(!result.prompt.contains("question-1"));
	assert!(result.prompt.contains("question-2"));
	assert!(result.prompt.contains("Pending user request: latest correction\n\nsteer detail"));
	assert!(!result.prompt.contains("PRIVATE_"));
}

#[test]
fn unicode_excerpt_keeps_answer_and_latest_correction_ends_within_full_prompt_budget() {
	let exchange = |user: String, assistant: String| Exchange { user, assistant };
	let exchanges = vec![
		exchange("old".repeat(20000), "old answer".into()),
		exchange(
			format!("START{}END", "中".repeat(30000)),
			format!("FIXED{}NOT INSTALLED", "汉".repeat(30000)),
		),
		exchange(format!("LATEST{}CORRECTION", "字".repeat(20000)), String::new()),
	];
	let history = super::super::excerpts::render(&exchanges);
	let prompt = super::super::prompt::build(&history);
	assert!(prompt.len() <= super::super::prompt::MAX_BYTES);
	for value in [
		"Earlier exchanges omitted",
		"START",
		"END",
		"FIXED",
		"NOT INSTALLED",
		"LATEST",
		"CORRECTION",
	] {
		assert!(prompt.contains(value), "missing {value}");
	}
	assert!(!prompt.contains("old answer"));
}

#[test]
fn media_is_described_without_payloads_and_internal_voice_handoff_is_not_summarized() {
	let image =
		json!({"type":"userMessage","content":[{"type":"image","url":"data:PRIVATE_IMAGE"}]});
	let projected = visible(&[image]).expect("visible media placeholder");
	assert_eq!(projected[0].text, "[Image attachment]");
	let handoff = json!({"type":"userMessage","content":[{"type":"text","text":"<realtime_delegation><input>PRIVATE_INTERNAL_HANDOFF</input></realtime_delegation>","textElements":[]}]});
	assert!(visible(&[handoff]).is_err());
}

#[tokio::test]
async fn native_turn_and_item_pages_are_joined_without_model_requests() {
	use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};
	let (local, remote) = tokio::io::duplex(16384);
	let (read, write) = tokio::io::split(local);
	let (client, _events) = AppServerClient::from_io(read, write);
	let server = tokio::spawn(async move {
		let (read, mut write) = tokio::io::split(remote);
		let mut lines = BufReader::new(read).lines();
		let replies = [
			("thread/read", json!({"thread":{"id":"source","historyMode":"paginated"}})),
			("thread/turns/list", json!({"data":[],"nextCursor":"older"})),
			(
				"thread/turns/list",
				json!({"data":[{"id":"turn","status":"completed"}],"nextCursor":null}),
			),
			(
				"thread/items/list",
				json!({"data":[{"turnId":"turn","item":{"id":"user","type":"userMessage","content":[{"type":"text","text":"Keep the current goal"}]}}],"nextCursor":"more"}),
			),
			(
				"thread/items/list",
				json!({"data":[{"turnId":"turn","item":{"id":"answer","type":"agentMessage","text":"Tested; not deployed"}}],"nextCursor":null}),
			),
		];
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
				.write_all(format!("{}\n", json!({"id":request["id"],"result":result})).as_bytes())
				.await
				.expect("reply");
		}
	});
	let prepared = super::read(&client, "source").await.expect("complete native pages");
	assert_eq!(prepared.latest_turn.as_deref(), Some("turn"));
	assert!(prepared.prompt.contains("Keep the current goal"));
	assert!(prepared.prompt.contains("Tested; not deployed"));
	server.await.expect("fixture");
}
