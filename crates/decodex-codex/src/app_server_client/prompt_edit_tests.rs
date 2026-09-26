use super::*;
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};

fn user(id: &str, content: Value) -> Value {
	json!({"type":"userMessage","id":id,"content":content})
}
fn page(items: Vec<Value>, next: Option<&str>) -> Value {
	json!({"data":items.into_iter().map(|item| json!({"turnId":"target","item":item})).collect::<Vec<_>>(),"nextCursor":next})
}
async fn read(
	mode: &str,
	status: &str,
	pages: Vec<Value>,
	previous_review: bool,
	changed: bool,
	reverted: bool,
) -> (Result<Option<PromptEditCandidate>, ClientError>, Vec<Value>, bool) {
	let (local, remote) = tokio::io::duplex(64 * 1024);
	let (r, w) = tokio::io::split(local);
	let (client, _events) = AppServerClient::from_io(r, w);
	let (done, mut stop) = tokio::sync::oneshot::channel();
	let mode = mode.to_owned();
	let status = status.to_owned();
	let server = tokio::spawn(async move {
		let (r, mut w) = tokio::io::split(remote);
		let mut lines = BufReader::new(r).lines();
		let mut requests = Vec::new();
		loop {
			let line = tokio::select! { _ = &mut stop => break, line = lines.next_line() => line.unwrap().unwrap() };
			let request: Value = serde_json::from_str(&line).unwrap();
			assert_eq!(request["params"]["threadId"], "thread");
			let result = match request["method"].as_str().unwrap() {
				"thread/read" => json!({"thread":{"id":"thread","historyMode":mode}}),
				"thread/turns/list" =>
					if request["params"]["limit"] == 1 {
						if reverted {
							w.write_all(b"{\"method\":\"thread/reverted\",\"params\":{\"threadId\":\"thread\"}}\n").await.unwrap();
						}
						json!({"data":[{"id":if changed {"changed"} else {"target"},"status":status}],"nextCursor":null})
					} else {
						json!({"data":[{"id":"target","status":status,"completedAt":null},{"id":"previous","status":"completed"}],"nextCursor":null})
					},
				"thread/items/list" => {
					assert_eq!(request["params"]["sortDirection"], "asc");
					if request["params"]["turnId"] == "previous" {
						let items = if previous_review {
							vec![
								json!({"type":"enteredReviewMode","id":"enter"}),
								json!({"type":"exitedReviewMode","id":"exit"}),
							]
						} else {
							vec![]
						};
						json!({"data":items.into_iter().map(|item|json!({"turnId":"previous","item":item})).collect::<Vec<_>>(),"nextCursor":null})
					} else {
						assert_eq!(request["params"]["turnId"], "target");
						pages[if request["params"]["cursor"].is_null() { 0 } else { 1 }].clone()
					}
				},
				other => panic!("selection must not mutate or infer: {other}"),
			};
			w.write_all(format!("{}\n", json!({"id":request["id"],"result":result})).as_bytes())
				.await
				.unwrap();
			requests.push(request);
		}
		requests
	});
	let result = client.prompt_edit_candidate("thread", "target", "selected").await;
	let live = result
		.as_ref()
		.ok()
		.and_then(Option::as_ref)
		.is_some_and(|candidate| candidate.guard.is_live());
	done.send(()).unwrap();
	(result, server.await.unwrap(), live)
}
#[tokio::test]
async fn full_input_preserves_native_text_spans_mentions_and_attachments() {
	let content = json!([
		{"type":"text","text":"use $skill @sample","text_elements":[{"byteRange":{"start":4,"end":10},"placeholder":"$skill"}]},
		{"type":"skill","name":"skill","path":"/fixture/skills/skill/SKILL.md"},
		{"type":"mention","name":"Sample Plugin","path":"plugin://sample@test"},
		{"type":"localImage","path":"/fixture/image.png","detail":null},
		{"type":"image","fileId":"native-file","detail":null}
	]);
	let pages = vec![
		page(vec![json!({"type":"agentMessage","id":"earlier","text":"Context"})], Some("next")),
		page(vec![user("selected", content.clone())], None),
	];
	let (result, requests, live) = read("paginated", "completed", pages, false, false, false).await;
	let candidate = result.unwrap().unwrap();
	assert!(live);
	assert_eq!(candidate.thread_id, "thread");
	assert_eq!(candidate.before_turn_id, "target");
	assert_eq!(candidate.latest_turn_id, "target");
	assert_eq!(candidate.item_id, "selected");
	assert_eq!(candidate.turn_ids.last().map(String::as_str), Some("target"));
	assert_eq!(Value::Array(candidate.content), content);
	assert_eq!(requests.iter().filter(|r| r["method"] == "thread/items/list").count(), 2);
}
#[tokio::test]
async fn clipped_steer_is_not_an_independent_prompt() {
	let input = json!([{"type":"text","text":"Input"}]);
	let pages = vec![
		page(vec![user("first", input.clone())], Some("next")),
		page(vec![user("selected", input)], None),
	];
	let (result, requests, _) = read("paginated", "completed", pages, false, false, false).await;
	assert!(result.unwrap().is_none());
	assert_eq!(requests.iter().filter(|r| r["method"] == "thread/items/list").count(), 2);
}
#[tokio::test]
async fn legacy_and_running_turns_do_not_offer_an_edit() {
	for (mode, status) in [("legacy", "completed"), ("paginated", "inProgress")] {
		let (result, requests, _) = read(mode, status, vec![], false, false, false).await;
		assert!(result.unwrap().is_none());
		assert!(!requests.iter().any(|r| r["method"] == "thread/items/list"));
	}
}
#[tokio::test]
async fn hidden_inline_and_nested_review_inputs_are_not_editable() {
	let input = json!([{"type":"text","text":"Review input"}]);
	let inline = vec![page(
		vec![json!({"type":"enteredReviewMode","id":"enter"}), user("selected", input.clone())],
		None,
	)];
	assert!(read("paginated", "completed", inline, false, false, false).await.0.unwrap().is_none());
	let nested = vec![page(vec![user("selected", input.clone()), user("duplicate", input)], None)];
	assert!(
		read("paginated", "interrupted", nested.clone(), true, false, false)
			.await
			.0
			.unwrap()
			.is_none()
	);
	assert!(
		read("paginated", "interrupted", nested, false, false, false).await.0.unwrap().is_some()
	);
}
#[tokio::test]
async fn concurrent_new_turn_or_revert_rejects_the_observation() {
	for (changed, reverted) in [(true, false), (false, true)] {
		let pages =
			vec![page(vec![user("selected", json!([{"type":"text","text":"Keep"}]))], None)];
		assert!(matches!(
			read("paginated", "completed", pages, false, changed, reverted).await.0,
			Err(ClientError::InvalidFrame)
		));
	}
}
