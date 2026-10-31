use super::conversation_url;

#[test]
fn native_message_binding_preserves_source_instead_of_display_whitespace() {
	let text = "**Context**\n\n\nDo not alter this.";
	let native = crate::identity::NativeMessage {
		id: "u1".into(),
		role: "user".into(),
		text: text.into(),
		complete: true,
	};
	let mut rendered = super::Message {
		id: "u1".into(),
		role: "user".into(),
		text: "Context\n\n\n\n\nDo not alter this.".into(),
		text_sha256: None,
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
	};
	let mut rendered = super::Message {
		id: "a1".into(),
		role: "assistant".into(),
		text: "Partial display".into(),
		text_sha256: None,
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
	] {
		assert!(conversation_url(url).is_err(), "{url}");
	}
}
