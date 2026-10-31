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
	record.receipt.execution = Some(crate::ExecutionIdentity {
		message_id: "assistant-one".into(),
		model: "gpt-test-pro".into(),
		mode: None,
	});
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
	record.receipt.execution = Some(crate::ExecutionIdentity {
		message_id: "answer-one".into(),
		model: "gpt-test-pro".into(),
		mode: None,
	});
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

#[test]
fn pro_label_and_another_turns_identity_cannot_qualify_a_completed_answer() {
	let root = tempfile::tempdir().unwrap();
	let store = Store::open(root.path()).unwrap();
	let mut record = store
		.prepare(Request { id: "identity".into(), prompt: "question".into(), previous: None })
		.unwrap();
	record.receipt.phase = Phase::Sending;
	record.receipt.model = Some("Pro".into());
	let observed = observation(
		vec![
			message("user", "u", &record.wire_prompt(), true),
			message("assistant", "a", "Full answer", true),
		],
		false,
	);
	assert!(reconcile(&mut record, &observed).unwrap_err().contains("unconfirmed"));
	assert_eq!(record.answer.as_ref().unwrap().text, "Full answer");
	assert_eq!(record.receipt.phase, Phase::Submitted);
	record.receipt.execution = Some(crate::ExecutionIdentity {
		message_id: "another-answer".into(),
		model: "gpt-5-pro".into(),
		mode: None,
	});
	assert!(reconcile(&mut record, &observed).is_err());
	record.receipt.execution.as_mut().unwrap().message_id = "a".into();
	assert!(reconcile(&mut record, &observed).unwrap());
}

#[tokio::test]
async fn queued_research_keeps_the_journal_available_and_deduplicates_workers() {
	use std::time::Duration;
	use tokio::time::{Instant, sleep};
	let root = tempfile::tempdir().unwrap();
	let store = Store::open(root.path()).unwrap();
	let bridge = crate::ProBridge::new(crate::Config::default(), root.path()).unwrap();
	bridge
		.prepare(Request { id: "queued".into(), prompt: "first".into(), previous: None })
		.unwrap();
	let execution = store.execution_lease().unwrap().unwrap();
	let runner = bridge.clone();
	let worker = tokio::spawn(async move { runner.resume("queued").await });
	let deadline = Instant::now() + Duration::from_secs(3);
	loop {
		if store.worker_lease("queued").unwrap().is_none() {
			break;
		}
		assert!(Instant::now() < deadline, "Worker did not join the queue.");
		sleep(Duration::from_millis(10)).await;
	}
	assert_eq!(bridge.resume("queued").await.unwrap().phase, Phase::Prepared);
	assert_eq!(
		bridge
			.prepare(Request { id: "second".into(), prompt: "second".into(), previous: None })
			.unwrap()
			.phase,
		Phase::Prepared
	);
	drop(execution);
	assert!(worker.await.unwrap().unwrap_err().contains("not connected"));
	assert_eq!(bridge.status("queued").unwrap().phase, Phase::Prepared);
	assert!(bridge.status("queued").unwrap().last_error.unwrap().contains("not connected"));
}

#[test]
fn result_waits_for_final_text_code_and_links_to_stop_changing() {
	let mut previous = None;
	let mut first = observation(vec![message("assistant", "a", "Partial", true)], false);
	super::wait_for_stable_answer(&mut first, &mut previous);
	assert!(!first.messages[0].complete);
	let mut final_text =
		observation(vec![message("assistant", "a", "Partial and final", true)], false);
	super::wait_for_stable_answer(&mut final_text, &mut previous);
	assert!(!final_text.messages[0].complete);
	let mut final_links =
		observation(vec![message("assistant", "a", "Partial and final", true)], false);
	final_links.messages[0].code_blocks.push("full code\n".into());
	final_links.messages[0]
		.links
		.push(crate::Link { label: "Source".into(), url: "https://example.com/".into() });
	let mut stable = final_links.clone();
	super::wait_for_stable_answer(&mut final_links, &mut previous);
	assert!(!final_links.messages[0].complete);
	super::wait_for_stable_answer(&mut stable, &mut previous);
	assert!(stable.messages[0].complete);
}
