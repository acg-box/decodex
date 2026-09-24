//! Native web actions and opaque results survive the exact-item detail projection.
use super::*;

fn detail(mut item: Value) -> (String, bool) {
	item["id"] = json!("i");
	item["type"] = json!("webSearch");
	let history = json!({"thread":{"id":"t","turns":[{"id":"u","items":[item]}]}});
	let Some(ChiefActivityDetailResult::Available { text, truncated, .. }) =
		page(&project_text(&history, "t", "u", "i").unwrap(), "scope", None)
	else {
		panic!("web detail")
	};
	(text, truncated)
}

#[test]
fn actions_preserve_queries_urls_and_find_patterns() {
	for (action, expected) in [
		(json!({"type":"search","query":"one","queries":["one","two",""]}), "one\n\ntwo"),
		(
			json!({"type":"openPage","url":"https://example.com/full/path"}),
			"https://example.com/full/path",
		),
		(
			json!({"type":"findInPage","url":"https://example.com","pattern":"界"}),
			"https://example.com\n\n界",
		),
		(json!({"type":"other"}), "legacy"),
	] {
		let (text, truncated) = detail(json!({"query":"legacy","action":action}));
		assert_eq!(text, format!("{expected}\n\nResults not reported."));
		assert!(!truncated);
	}
}

#[test]
fn results_keep_unknown_empty_error_and_future_fields_distinct() {
	assert_eq!(detail(json!({"results":null})).0, "Results not reported.");
	assert_eq!(detail(json!({"results":[]})).0, "No results returned.");
	let result =
		json!({"url":"https://example.com","error":{"status":404},"future":{"content":"kept"}});
	assert_eq!(detail(json!({"results":[result.clone()]})).0, result.to_string());
}

#[test]
fn results_are_redacted_before_utf8_display_limit() {
	let (text, _) = detail(
		json!({"results":[{"url":"https://example.com/?token=synthetic-secret"},{"content":"public result"}]}),
	);
	assert!(!text.contains("synthetic-secret"));
	assert!(text.contains("public result"));
	let (text, truncated) = detail(json!({"results":[{"content":"界".repeat(10000)}]}));
	assert!(truncated);
	assert!(text.len() <= 24 * 1024);
}

#[tokio::test]
async fn paginated_native_history_preserves_page_action_and_result_error() {
	use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
	let (local, remote) = tokio::io::duplex(65536);
	let (reader, writer) = tokio::io::split(local);
	let (client, _events) = AppServerClient::from_io(reader, writer);
	let server = tokio::spawn(async move {
		let (reader, mut writer) = tokio::io::split(remote);
		let mut lines = BufReader::new(reader).lines();
		for (method, result) in [
			("thread/read", json!({"thread":{"id":"thread","historyMode":"paginated"}})),
			("thread/turns/list", json!({"data":[{"id":"turn"}],"nextCursor":null})),
			(
				"thread/items/list",
				json!({"data":[{"turnId":"turn","item":{"id":"web","type":"webSearch","query":"","action":{"type":"findInPage","url":"https://example.com","pattern":"needle"},"results":[{"error":{"status":404}}]}}],"nextCursor":null}),
			),
		] {
			let request: Value =
				serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
			assert_eq!(request["method"], method);
			assert_eq!(request["params"]["threadId"], "thread");
			writer
				.write_all(format!("{}\n", json!({"id":request["id"],"result":result})).as_bytes())
				.await
				.unwrap();
		}
	});
	let ChiefActivityDetailResult::Available { text, truncated, .. } =
		read(&client, "thread", "turn", "web").await
	else {
		panic!("native detail")
	};
	assert!(text.contains("https://example.com"));
	assert!(text.contains("needle"));
	assert!(text.contains("404"));
	assert!(!truncated);
	server.await.unwrap();
}

#[test]
fn complete_patch_pages_preserve_unicode_and_reject_changed_evidence() {
	let diff = format!("{}\nfinal patch line", "+界🙂e\u{301}\n".repeat(9000));
	let history = json!({"thread":{"id":"thread","turns":[{"id":"turn","items":[{
		"id":"patch","type":"fileChange","changes":[{"path":"file.rs","kind":{"type":"update"},"diff":diff}]
	}]}]}});
	let text = project_text(&history, "thread", "turn", "patch").unwrap();
	let mut cursor = None;
	let mut complete = String::new();
	loop {
		let result = page(&text, "source-a", cursor.as_ref()).unwrap();
		assert!(serde_json::to_vec(&result).unwrap().len() < 60 * 1024);
		let ChiefActivityDetailResult::Available { text: portion, offset, next, truncated } =
			result
		else {
			panic!("page");
		};
		assert_eq!(offset as usize, complete.len());
		assert_eq!(truncated, next.is_some());
		complete.push_str(&portion);
		let Some(next) = next else {
			break;
		};
		assert_eq!(next.offset as usize, complete.len());
		assert!(page(&text, "source-b", Some(&next)).is_none());
		assert!(page(&format!("{text}changed"), "source-a", Some(&next)).is_none());
		let mut invalid = next.clone();
		invalid.offset = u32::try_from(text.len() + 1).unwrap();
		assert!(page(&text, "source-a", Some(&invalid)).is_none());
		invalid.offset = u32::try_from(text.find('界').unwrap() + 1).unwrap();
		assert!(page(&text, "source-a", Some(&invalid)).is_none());
		cursor = Some(next);
	}
	assert_eq!(complete, text);
	assert!(complete.ends_with("final patch line"));
}
