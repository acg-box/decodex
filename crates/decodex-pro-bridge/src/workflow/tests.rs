use std::time::Duration;

use futures_util::{SinkExt as _, StreamExt as _};
use serde_json::Value;
use tokio::{
	net::UnixListener,
	time::{self, Instant},
};
use tokio_tungstenite::tungstenite;

use crate::{
	Config, Delivery, ExecutionIdentity, Link, Phase, ProBridge, Request,
	chrome::Chrome,
	native_host,
	page::Observation,
	store::{self, Store},
	workflow::{self},
};

fn message(role: &str, id: &str, text: &str, complete: bool) -> crate::page::Message {
	crate::page::Message {
		role: role.into(),
		id: id.into(),
		text: if role == "user" { String::new() } else { text.into() },
		text_sha256: (role == "user").then(|| store::digest(text.trim().as_bytes())),
		markdown_text_sha256: None,
		complete,
		code_blocks: vec![],
		links: vec![],
		attachments: None,
	}
}

fn observation(messages: Vec<crate::page::Message>, generating: bool) -> Observation {
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
		.prepare(Request {
			id: "one".into(),
			prompt: "question".into(),
			previous: None,
			project: None,
			context: None,
			delivery: Default::default(),
		})
		.expect("record");

	record.receipt.phase = Phase::Sending;
	record.receipt.model = Some("Pro".into());
	record.receipt.execution = Some(ExecutionIdentity {
		message_id: "assistant-one".into(),
		model: "gpt-test-pro".into(),
		mode: None,
	});

	assert!(
		!workflow::reconcile(
			&mut record,
			&observation(vec![message("assistant", "old", "stale", true)], false)
		)
		.expect("still pending")
	);
	assert_eq!(record.receipt.phase, Phase::Sending);

	// A URL saved before a large DOM read is only a recovery candidate.
	record.receipt.conversation_url = Some("https://chatgpt.com/c/owned".into());

	let mut user = message("user", "user-one", "Provider Markdown representation", true);

	user.markdown_text_sha256 = Some(store::digest(record.wire_prompt().trim().as_bytes()));

	let mut truncated = user.clone();

	truncated.text_sha256 = Some(store::digest(b"truncated question"));
	truncated.markdown_text_sha256 = Some(store::digest(b"truncated question"));

	assert!(
		!workflow::reconcile(&mut record, &observation(vec![truncated], false))
			.expect("wrong full-text hash stays fenced")
	);
	assert_eq!(record.receipt.phase, Phase::Sending);
	assert!(record.receipt.user_message_id.is_none());
	assert!(record.receipt.input_binding.is_none());

	let assistant = message("assistant", "assistant-one", "complete answer", true);

	assert!(
		!workflow::reconcile(
			&mut record,
			&observation(vec![user.clone(), assistant.clone()], true)
		)
		.expect("streaming")
	);
	assert_eq!(record.receipt.phase, Phase::Submitted);
	assert_eq!(record.receipt.input_binding.as_deref(), Some("markdown_text"));
	assert!(
		workflow::reconcile(&mut record, &observation(vec![user, assistant], false))
			.expect("completed")
	);
	assert_eq!(record.answer.expect("answer").text, "complete answer");
}

#[test]
fn first_submission_waits_until_chatgpt_publishes_its_conversation_url() {
	let root = tempfile::tempdir().expect("root");
	let store = Store::open(root.path()).expect("store");
	let mut record = store
		.prepare(Request {
			id: "one".into(),
			prompt: "question".into(),
			previous: None,
			project: None,
			context: None,
			delivery: Default::default(),
		})
		.expect("record");

	record.receipt.phase = Phase::Sending;
	record.receipt.model = Some("Pro".into());

	let user = message("user", "user-one", &record.wire_prompt(), true);
	let mut observed = observation(vec![user], true);

	observed.url = "https://chatgpt.com/".into();

	assert!(!workflow::reconcile(&mut record, &observed).expect("route is still pending"));
	assert_eq!(record.receipt.phase, Phase::Sending);
	assert!(record.receipt.conversation_url.is_none());

	observed.url = "https://chatgpt.com/c/owned".into();

	assert!(!workflow::reconcile(&mut record, &observed).expect("submission is confirmed"));
	assert_eq!(record.receipt.phase, Phase::Submitted);
	assert_eq!(record.receipt.user_message_id.as_deref(), Some("user-one"));

	let project = "https://chatgpt.com/g/g-p-6ae73c95938c819187c369cc68995079/project";

	record.receipt.project_url = Some(project.into());
	observed.url =
		"https://chatgpt.com/g/g-p-6ae73c95938c819187c369cc68995079-trial/c/owned".into();

	assert!(
		!workflow::reconcile(&mut record, &observed)
			.expect("a friendly project suffix keeps the submission identity")
	);

	observed.url =
		"https://chatgpt.com/g/g-p-6ae73c95938c819187c369cc68995070-trial/c/owned".into();

	assert!(workflow::reconcile(&mut record, &observed).unwrap_err().contains("different project"));
}

#[test]
fn continuation_refuses_a_conversation_that_another_user_message_advanced() {
	let root = tempfile::tempdir().expect("root");
	let store = Store::open(root.path()).expect("store");
	let mut record = store
		.prepare(Request {
			id: "one".into(),
			prompt: "question".into(),
			previous: None,
			project: None,
			context: None,
			delivery: Default::default(),
		})
		.expect("record");

	record.parent_answer_id = Some("saved-answer".into());
	record.receipt.conversation_url = Some("https://chatgpt.com/c/owned".into());

	let saved = message("assistant", "saved-answer", "answer", true);

	assert!(
		workflow::validate_before_send(&record, &observation(vec![saved.clone()], false)).is_ok()
	);
	assert!(
		workflow::validate_before_send(
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
		.prepare(Request {
			id: "hard-break".into(),
			prompt: "question\n".into(),
			previous: None,
			project: None,
			context: None,
			delivery: Default::default(),
		})
		.expect("record");

	record.receipt.phase = Phase::Sending;
	record.receipt.model = Some("Pro".into());
	record.receipt.execution = Some(ExecutionIdentity {
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
			!workflow::reconcile(
				&mut record,
				&observation(vec![message("user", "user-one", &changed, true)], false)
			)
			.expect("stay fenced")
		);
		assert_eq!(record.receipt.phase, Phase::Sending);
	}

	let original = record.request.prompt.clone();

	assert!(
		workflow::reconcile(
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
		.prepare(Request {
			id: "no-break".into(),
			prompt: "question".into(),
			previous: None,
			project: None,
			context: None,
			delivery: Default::default(),
		})
		.expect("record");

	record.receipt.phase = Phase::Sending;
	record.receipt.model = Some("Pro".into());

	let changed = format!("{}\\", record.wire_prompt());

	assert!(
		!workflow::reconcile(
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
		.prepare(Request {
			id: "identity".into(),
			prompt: "question".into(),
			previous: None,
			project: None,
			context: None,
			delivery: Default::default(),
		})
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

	assert!(workflow::reconcile(&mut record, &observed).unwrap_err().contains("unconfirmed"));
	assert_eq!(record.answer.as_ref().unwrap().text, "Full answer");
	assert_eq!(record.receipt.phase, Phase::Submitted);

	record.receipt.execution = Some(ExecutionIdentity {
		message_id: "another-answer".into(),
		model: "gpt-5-pro".into(),
		mode: None,
	});

	assert!(workflow::reconcile(&mut record, &observed).is_err());

	record.receipt.execution.as_mut().unwrap().message_id = "a".into();

	assert!(workflow::reconcile(&mut record, &observed).unwrap());
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
		.push(Link { label: "Source".into(), url: "https://example.com/".into() });

	let mut stable = final_links.clone();

	super::wait_for_stable_answer(&mut final_links, &mut previous);

	assert!(!final_links.messages[0].complete);

	super::wait_for_stable_answer(&mut stable, &mut previous);

	assert!(stable.messages[0].complete);
}

#[test]
fn attachment_recovery_requires_the_exact_submitted_file_before_completing() {
	let root = tempfile::tempdir().unwrap();
	let store = Store::open(root.path()).unwrap();
	let mut saved = store
		.prepare(Request {
			id: "attached".into(),
			prompt: "Compare this source.".into(),
			previous: None,
			project: None,
			context: Some("private source".repeat(20_000)),
			delivery: Delivery::Auto,
		})
		.unwrap();

	saved.receipt.phase = Phase::Sending;
	saved.receipt.execution = Some(ExecutionIdentity {
		message_id: "answer".into(),
		model: "gpt-test-pro".into(),
		mode: None,
	});

	let file = saved.receipt.attachment.clone().unwrap();
	let answer = message("assistant", "answer", "Complete answer", true);
	let mut user = message("user", "submitted", &saved.wire_prompt(), true);
	let mut missing = saved.clone();

	assert!(
		!workflow::reconcile(&mut missing, &observation(vec![user.clone(), answer.clone()], false))
			.unwrap()
	);
	assert_eq!(missing.receipt.phase, Phase::Submitted);
	assert!(missing.answer.is_none());

	user.attachments = Some(vec![]);

	assert!(
		workflow::reconcile(
			&mut saved.clone(),
			&observation(vec![user.clone(), answer.clone()], false)
		)
		.unwrap_err()
		.contains("exactly the saved source")
	);

	user.attachments = Some(vec![crate::identity::SubmittedFile {
		id: "file-one".into(),
		name: "wrong.txt".into(),
		bytes: file.bytes,
	}]);

	assert!(
		workflow::reconcile(
			&mut saved.clone(),
			&observation(vec![user.clone(), answer.clone()], false)
		)
		.unwrap_err()
		.contains("differs from the saved manifest")
	);

	user.attachments.as_mut().unwrap()[0].name = file.name;

	assert!(
		workflow::reconcile(&mut saved, &observation(vec![user.clone(), answer.clone()], false))
			.unwrap()
	);
	assert_eq!(saved.receipt.attachment.as_ref().unwrap().file_id.as_deref(), Some("file-one"));

	store.save(&saved).unwrap();

	let mut recovered = store.load("attached").unwrap();

	user.attachments.as_mut().unwrap()[0].id = "file-replaced".into();

	assert!(
		workflow::reconcile(&mut recovered, &observation(vec![user, answer], false))
			.unwrap_err()
			.contains("differs from the saved manifest")
	);
}

#[tokio::test]
async fn continuation_waits_for_history_after_a_cold_composer_mount() {
	let root = tempfile::tempdir().unwrap();
	let store = Store::open(root.path()).unwrap();
	let mut record = store
		.prepare(Request {
			id: "cold".into(),
			prompt: "follow up".into(),
			previous: None,
			project: None,
			context: None,
			delivery: Default::default(),
		})
		.unwrap();

	record.parent_answer_id = Some("previous".into());
	record.receipt.conversation_url = Some("https://chatgpt.com/c/owned".into());

	let listener = UnixListener::bind(native_host::socket_path(root.path())).unwrap();
	let server = tokio::spawn(async move {
		let (stream, _) = listener.accept().await.unwrap();
		let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
		let mut reads = 0;

		while let Some(Ok(tungstenite::Message::Text(text))) = socket.next().await {
			let request: Value = serde_json::from_str(&text).unwrap();
			let result = match request["method"].as_str().unwrap() {
				"Decodex.ping" =>
					serde_json::json!({"protocol":1,"extensionId":crate::native_host::EXTENSION_ID}),
				"Runtime.evaluate" => {
					reads += 1;

					let messages = if reads < 3 {
						serde_json::json!([])
					} else {
						serde_json::json!([{"id":"previous","role":"assistant","text":"ready","complete":true,"codeBlocks":[],"links":[]}])
					};

					serde_json::json!({"result":{"value":{"url":"https://chatgpt.com/c/owned","generating":false,"messages":messages}}})
				},
				method => panic!("Cold restoration must not send input: {method}"),
			};

			socket
				.send(tungstenite::Message::Text(
					serde_json::json!({"id":request["id"],"result":result}).to_string().into(),
				))
				.await
				.unwrap();
		}

		reads
	});
	let mut browser = Chrome::connect(root.path()).await.unwrap();
	let restored = super::observe_before_send(&record, &mut browser).await.unwrap();

	assert_eq!(restored.messages.last().unwrap().id, "previous");

	drop(browser);

	assert_eq!(server.await.unwrap(), 3);
}

#[tokio::test]
async fn queued_research_keeps_the_journal_available_and_deduplicates_workers() {
	let root = tempfile::tempdir().unwrap();
	let store = Store::open(root.path()).unwrap();
	let bridge = ProBridge::new(Config::default(), root.path()).unwrap();

	bridge
		.prepare(Request {
			id: "queued".into(),
			prompt: "first".into(),
			previous: None,
			project: None,
			context: None,
			delivery: Default::default(),
		})
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

		time::sleep(Duration::from_millis(10)).await;
	}

	assert_eq!(bridge.resume("queued").await.unwrap().phase, Phase::Prepared);
	assert_eq!(
		bridge
			.prepare(Request {
				id: "second".into(),
				prompt: "second".into(),
				previous: None,
				project: None,
				context: None,
				delivery: Default::default(),
			})
			.unwrap()
			.phase,
		Phase::Prepared
	);

	drop(execution);

	assert!(worker.await.unwrap().unwrap_err().contains("not connected"));
	assert_eq!(bridge.status("queued").unwrap().phase, Phase::Prepared);
	assert!(bridge.status("queued").unwrap().last_error.unwrap().contains("not connected"));
}
