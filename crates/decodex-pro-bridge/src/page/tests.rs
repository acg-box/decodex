use super::conversation_url;

#[tokio::test]
async fn disabled_send_reports_the_page_reason_without_clicking() {
	use futures_util::{SinkExt as _, StreamExt as _};
	use serde_json::{Value, json};
	use tokio_tungstenite::tungstenite::Message;
	let root = tempfile::tempdir().unwrap();
	let listener =
		tokio::net::UnixListener::bind(crate::native_host::socket_path(root.path())).unwrap();
	let server = tokio::spawn(async move {
		let (stream, _) = listener.accept().await.unwrap();
		let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
		let mut moves = 0;
		while let Some(Ok(Message::Text(text))) = socket.next().await {
			let request: Value = serde_json::from_str(&text).unwrap();
			let result = match request["method"].as_str().unwrap() {
				"Decodex.ping" =>
					json!({"protocol":1,"extensionId":crate::native_host::EXTENSION_ID}),
				"Runtime.evaluate" => {
					let point = request["params"]["expression"]
						.as_str()
						.unwrap()
						.contains("getBoundingClientRect");
					json!({"result":{"value":if point {json!({"x":25,"y":35})}else{json!("Message is too long")}}})
				},
				"Input.dispatchMouseEvent" => {
					assert_eq!(request["params"]["type"], "mouseMoved");
					assert_eq!(request["params"]["buttons"], 0);
					moves += 1;
					json!({})
				},
				other => panic!("Unexpected browser action: {other}"),
			};
			socket
				.send(Message::Text(json!({"id":request["id"],"result":result}).to_string().into()))
				.await
				.unwrap();
		}
		assert_eq!(moves, 1);
	});
	let mut browser = super::Chrome::connect(root.path()).await.unwrap();
	assert_eq!(
		super::send_block_reason(&mut browser).await.unwrap().as_deref(),
		Some("Message is too long")
	);
	drop(browser);
	server.await.unwrap();
}

#[test]
fn declared_markdown_user_text_decodes_the_entire_input_and_keeps_link_destinations() {
	let original = "[Decodex Pro request: binding]\n\nUse [Reference](https://example.com/).\nKeep every line.";
	let encoded = "[Decodex Pro request: binding]\n\nUse [Reference]\\([https://example.com/](https://example.com/)).\nKeep every line.";
	assert_eq!(super::markdown_user_text(encoded).as_deref(), Some(original));
	for unsafe_body in [
		"[https://example.com/](https://other.example/)",
		"[https://example.com/](https://example.com/ \"hidden title\")",
		"Keep<!-- hidden text --> every line.",
		"![image](https://example.com/)",
	] {
		assert!(super::markdown_user_text(unsafe_body).is_none(), "{unsafe_body}");
	}
	let native = crate::identity::NativeMessage {
		id: "user".into(),
		role: "user".into(),
		text: encoded.into(),
		complete: true,
		markdown: false,
	};
	let mut rendered = super::Message {
		id: "user".into(),
		role: "user".into(),
		text: String::new(),
		text_sha256: None,
		markdown_text_sha256: None,
		complete: true,
		code_blocks: vec![],
		links: vec![],
	};
	super::apply_native_message(&mut rendered, &native);
	assert!(rendered.markdown_text_sha256.is_none(), "an undeclared format must remain verbatim");
	super::apply_native_message(
		&mut rendered,
		&crate::identity::NativeMessage { markdown: true, ..native },
	);
	assert_eq!(
		rendered.markdown_text_sha256.as_deref(),
		Some(crate::store::digest(original.as_bytes()).as_str())
	);
}

#[test]
fn native_message_binding_preserves_source_instead_of_display_whitespace() {
	let text = "**Context**\n\n\nDo not alter this.";
	let native = crate::identity::NativeMessage {
		id: "u1".into(),
		role: "user".into(),
		text: text.into(),
		complete: true,
		markdown: false,
	};
	let mut rendered = super::Message {
		id: "u1".into(),
		role: "user".into(),
		text: "Context\n\n\n\n\nDo not alter this.".into(),
		text_sha256: None,
		markdown_text_sha256: None,
		complete: true,
		code_blocks: vec![],
		links: vec![],
	};
	super::apply_native_message(&mut rendered, &native);
	assert_eq!(rendered.text, text);
	let other =
		crate::identity::NativeMessage { id: "other".into(), text: "Unrelated".into(), ..native };
	super::apply_native_message(&mut rendered, &other);
	assert_eq!(rendered.text, text);
}

#[test]
fn complete_original_markdown_preserves_code_that_a_virtualized_view_omits() {
	let code = (1..=60).map(|n| format!("TEST-{n:03}\n")).collect::<String>();
	let text = format!("Acknowledged\n\n```text\n{code}```\n\n[Example](https://example.com/)");
	let native = crate::identity::NativeMessage {
		id: "a1".into(),
		role: "assistant".into(),
		text: text.clone(),
		complete: false,
		markdown: false,
	};
	let mut rendered = super::Message {
		id: "a1".into(),
		role: "assistant".into(),
		text: "Partial display".into(),
		text_sha256: None,
		markdown_text_sha256: None,
		complete: false,
		code_blocks: vec![],
		links: vec![],
	};
	super::apply_native_message(&mut rendered, &native);
	assert_eq!(rendered.text, "Partial display");
	super::apply_native_message(
		&mut rendered,
		&crate::identity::NativeMessage { complete: true, ..native },
	);
	assert_eq!(rendered.text, text);
	assert_eq!(rendered.code_blocks, vec![code]);
	assert!(!rendered.complete, "The source does not skip the final render check");
	assert_eq!(super::markdown_code_blocks("~~~~text\n```\n~~~~\n"), vec!["```\n"]);
}

#[test]
fn recovery_accepts_only_exact_canonical_chatgpt_conversations() {
	let project = "https://chatgpt.com/g/g-p-6ae73c95938c819187c369cc68995079/project";
	let named =
		"https://chatgpt.com/g/g-p-6ae73c95938c819187c369cc68995079-research-trial/c/same-id";
	assert!(super::same_conversation("https://chatgpt.com/c/same-id", named));
	assert!(crate::projects::contains_url(project, named));
	let translated =
		"https://chatgpt.com/g/g-p-6ae73c95938c819187c369cc68995079-%E7%A0%94%E7%A9%B6/c/same-id";
	assert_eq!(super::conversation_url(translated).unwrap(), translated);
	assert!(crate::projects::contains_url(project, translated));
	assert!(!crate::projects::contains_url(
		"https://chatgpt.com/g/g-p-6ae73c95938c819187c369cc68995070/project",
		named
	));
	assert!(!super::same_conversation("https://chatgpt.com/c/other-id", named));
	assert_eq!(
		super::conversation_url("https://chatgpt.com/g/g-p-owned/c/thread-one").unwrap(),
		"https://chatgpt.com/g/g-p-owned/c/thread-one"
	);
	assert_eq!(
		conversation_url("https://chatgpt.com/c/owned-test").expect("owned"),
		"https://chatgpt.com/c/owned-test"
	);
	for url in [
		"https://chatgpt.com.evil.test/c/one",
		"http://chatgpt.com/c/one",
		"https://user@chatgpt.com/c/one",
		"https://chatgpt.com/c/one?model=pro",
		"https://chatgpt.com/c/one#part",
		"https://chatgpt.com:443/c/one",
		"https://chatgpt.com/",
		"https://chatgpt.com/g/custom-gpt/c/one",
		"https://chatgpt.com/g/g-p-owner/project",
		"https://chatgpt.com/g/g-p-owner/c/one/extra",
	] {
		assert!(conversation_url(url).is_err(), "{url}");
	}
}
