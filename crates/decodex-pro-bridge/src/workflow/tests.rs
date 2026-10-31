use crate::{
	Phase, Request,
	page::{Message, Observation},
	store::Store,
};

use super::{reconcile, validate_before_send};

fn message(role: &str, id: &str, text: &str, complete: bool) -> Message {
	Message {
		role: role.into(),
		id: id.into(),
		text: if role == "user" { String::new() } else { text.into() },
		text_sha256: (role == "user").then(|| crate::store::digest(text.trim().as_bytes())),
		complete,
		code_blocks: vec![],
		links: vec![],
	}
}

fn observation(messages: Vec<Message>, generating: bool) -> Observation {
	Observation {
		notices: Vec::new(),
		url: "https://chatgpt.com/c/owned".into(),
		model: "Pro".into(),
		generating,
		messages,
	}
}

#[test]
fn recovery_binds_the_exact_prompt_and_does_not_complete_from_a_stale_or_streaming_answer() {
	let root = tempfile::tempdir().expect("root");
	let store = Store::open(root.path()).expect("store");
	let mut record = store
		.prepare(Request { id: "one".into(), prompt: "question".into(), previous: None })
		.expect("record");
	record.receipt.phase = Phase::Sending;
	record.receipt.model = Some("Pro".into());
	assert!(
		!reconcile(
			&mut record,
			&observation(vec![message("assistant", "old", "stale", true)], false)
		)
		.expect("still pending")
	);
	assert_eq!(record.receipt.phase, Phase::Sending);
	// A URL saved before a large DOM read is only a recovery candidate.
	record.receipt.conversation_url = Some("https://chatgpt.com/c/owned".into());
	let user = message("user", "user-one", &record.wire_prompt(), true);
	let mut truncated = user.clone();
	truncated.text_sha256 = Some(crate::store::digest(b"truncated question"));
	assert!(
		!reconcile(&mut record, &observation(vec![truncated], false))
			.expect("wrong full-text hash stays fenced")
	);
	assert_eq!(record.receipt.phase, Phase::Sending);
	assert!(record.receipt.user_message_id.is_none());
	let assistant = message("assistant", "assistant-one", "complete answer", true);
	assert!(
		!reconcile(&mut record, &observation(vec![user.clone(), assistant.clone()], true))
			.expect("streaming")
	);
	assert_eq!(record.receipt.phase, Phase::Submitted);
	assert!(reconcile(&mut record, &observation(vec![user, assistant], false)).expect("completed"));
	assert_eq!(record.answer.expect("answer").text, "complete answer");
}

#[test]
fn first_submission_waits_until_chatgpt_publishes_its_conversation_url() {
	let root = tempfile::tempdir().expect("root");
	let store = Store::open(root.path()).expect("store");
	let mut record = store
		.prepare(Request { id: "one".into(), prompt: "question".into(), previous: None })
		.expect("record");
	record.receipt.phase = Phase::Sending;
	record.receipt.model = Some("Pro".into());
	let user = message("user", "user-one", &record.wire_prompt(), true);
	let mut observed = observation(vec![user], true);
	observed.url = "https://chatgpt.com/".into();
	assert!(!reconcile(&mut record, &observed).expect("route is still pending"));
	assert_eq!(record.receipt.phase, Phase::Sending);
	assert!(record.receipt.conversation_url.is_none());
	observed.url = "https://chatgpt.com/c/owned".into();
	assert!(!reconcile(&mut record, &observed).expect("submission is confirmed"));
	assert_eq!(record.receipt.phase, Phase::Submitted);
	assert_eq!(record.receipt.user_message_id.as_deref(), Some("user-one"));
}

#[test]
fn continuation_refuses_a_conversation_that_another_user_message_advanced() {
	let root = tempfile::tempdir().expect("root");
	let store = Store::open(root.path()).expect("store");
	let mut record = store
		.prepare(Request { id: "one".into(), prompt: "question".into(), previous: None })
		.expect("record");
	record.parent_answer_id = Some("saved-answer".into());
	record.receipt.conversation_url = Some("https://chatgpt.com/c/owned".into());
	let saved = message("assistant", "saved-answer", "answer", true);
	assert!(validate_before_send(&record, &observation(vec![saved.clone()], false)).is_ok());
	assert!(
		validate_before_send(
			&record,
			&observation(vec![saved, message("user", "other", "new input", true)], false)
		)
		.is_err()
	);
}

#[test]
fn recovery_accepts_only_the_exact_observed_terminal_hard_break_representation() {
	let root = tempfile::tempdir().expect("root");
	let store = Store::open(root.path()).expect("store");
	let mut record = store
		.prepare(Request { id: "hard-break".into(), prompt: "question\n".into(), previous: None })
		.expect("record");
	record.receipt.phase = Phase::Sending;
	record.receipt.model = Some("Pro".into());
	let rendered = format!("{}\\", record.wire_prompt().trim_end_matches('\n'));
	for changed in [
		rendered.replace("question", "changed"),
		format!("{rendered}\\"),
		rendered.replace("hard-break", "different-id"),
	] {
		assert!(
			!reconcile(
				&mut record,
				&observation(vec![message("user", "user-one", &changed, true)], false)
			)
			.expect("stay fenced")
		);
		assert_eq!(record.receipt.phase, Phase::Sending);
	}
	let original = record.request.prompt.clone();
	assert!(
		reconcile(
			&mut record,
			&observation(
				vec![
					message("user", "user-one", &rendered, true),
					message("assistant", "answer-one", "complete", true),
				],
				false
			)
		)
		.expect("recover without another Send")
	);
	assert_eq!(record.request.prompt, original);
	assert_eq!(record.receipt.user_message_id.as_deref(), Some("user-one"));
	assert_eq!(record.receipt.phase, Phase::Complete);
}

#[test]
fn a_literal_backslash_does_not_match_a_prompt_without_a_terminal_lf() {
	let root = tempfile::tempdir().expect("root");
	let store = Store::open(root.path()).expect("store");
	let mut record = store
		.prepare(Request { id: "no-break".into(), prompt: "question".into(), previous: None })
		.expect("record");
	record.receipt.phase = Phase::Sending;
	record.receipt.model = Some("Pro".into());
	let changed = format!("{}\\", record.wire_prompt());
	assert!(
		!reconcile(
			&mut record,
			&observation(vec![message("user", "user-one", &changed, true)], false)
		)
		.expect("stay fenced")
	);
	assert_eq!(record.receipt.phase, Phase::Sending);
}
