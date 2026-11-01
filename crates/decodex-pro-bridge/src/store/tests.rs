use crate::{Answer, Config, Phase, ProBridge, Request};

use super::Store;

fn request(id: &str) -> Request {
	Request {
		id: id.into(),
		prompt: "A harmless test question.".into(),
		previous: None,
		project: None,
		context: None,
		delivery: Default::default(),
	}
}

#[test]
fn continuations_inherit_project_identity_and_reject_cross_project_reuse() {
	let root = tempfile::tempdir().unwrap();
	let store = Store::open(root.path()).unwrap();
	let project = crate::Project { id: "native-project-one".into(), name: "Shared name".into() };
	let mut first = request("first-project");
	first.project = Some(project.clone());
	let mut parent = store.prepare(first).unwrap();
	parent.receipt.phase = Phase::Complete;
	parent.receipt.project_url = Some("https://chatgpt.com/g/g-p-one/project".into());
	store.save(&parent).unwrap();
	let mut follow = request("follow-project");
	follow.previous = Some(parent.request.id.clone());
	let saved = store.prepare(follow.clone()).unwrap();
	assert_eq!(saved.project, Some(project.clone()));
	assert_eq!(saved.receipt.project_url, parent.receipt.project_url);
	follow.id = "other-project".into();
	follow.project = Some(crate::Project { id: "native-project-two".into(), ..project });
	assert!(store.prepare(follow).unwrap_err().contains("cannot change its project"));
}

#[test]
fn large_unicode_material_is_immutable_without_a_fixed_prompt_limit() {
	let root = tempfile::tempdir().expect("private root");
	let store = Store::open(root.path()).expect("store");
	let mut input = request("large-unicode");
	input.prompt = "PRIVATE_PAYLOAD_研究资料_🧪\n".repeat(70_000);
	assert!(input.prompt.len() > 2 * 1024 * 1024);
	let record = store.prepare(input).expect("no arbitrary 64 KiB cap");
	assert_eq!(record.receipt.delivery, crate::Delivery::Attachment);
	assert!(record.wire_prompt().len() < 1024);
	let attachment = store.attachment(&record).unwrap().unwrap();
	assert_eq!(std::fs::read_to_string(&attachment.path).unwrap(), record.request.prompt);
	assert_eq!(record.receipt.attachment.as_ref().unwrap().sha256, attachment.sha256);
	let material = store.material(&record).expect("private immutable material");
	assert_eq!(std::fs::read_to_string(&material.path).expect("exact text"), record.wire_prompt());
	assert_eq!(material.bytes, record.wire_prompt().len());
	assert_eq!(store.material(&record).expect("stable reference").sha256, material.sha256);
	std::fs::write(&material.path, "changed after preparation").expect("corruption fixture");
	assert!(store.material(&record).is_err());
	assert_eq!(
		store.load(&record.request.id).expect("receipt preserved").receipt.phase,
		Phase::Prepared
	);
	let mut empty = request("empty");
	empty.prompt.clear();
	assert!(store.prepare(empty).is_err());
}

#[test]
fn delivery_uses_utf8_bytes_and_keeps_context_and_explicit_parts_immutable() {
	let root = tempfile::tempdir().unwrap();
	let store = Store::open(root.path()).unwrap();
	let mut exact = request("at-boundary");
	exact.prompt = "Q".into();
	exact.context = Some("x".repeat(super::INLINE_BYTES - 1));
	let inline = store.prepare(exact.clone()).unwrap();
	assert_eq!(inline.receipt.delivery, crate::Delivery::Inline);
	assert!(inline.receipt.attachment.is_none());
	assert!(inline.wire_prompt().ends_with(exact.context.as_deref().unwrap()));
	exact.id = "above-boundary".into();
	exact.context.as_mut().unwrap().push('研');
	let attached = store.prepare(exact.clone()).unwrap();
	assert_eq!(attached.receipt.delivery, crate::Delivery::Attachment);
	assert!(!attached.receipt.attachment.as_ref().unwrap().includes_question);
	let source = store.attachment(&attached).unwrap().unwrap();
	assert_eq!(std::fs::read_to_string(&source.path).unwrap(), exact.context.as_deref().unwrap());
	assert!(!attached.wire_prompt().contains("[Selected context]"));
	let mut changed = exact.clone();
	changed.context.as_mut().unwrap().push('!');
	assert!(store.prepare(changed).is_err());
	exact.id = "explicit-part".into();
	exact.delivery = crate::Delivery::Inline;
	let part = store.prepare(exact).unwrap();
	assert_eq!(part.receipt.delivery, crate::Delivery::Inline);
	assert!(part.wire_prompt().len() > super::INLINE_BYTES);
	std::fs::write(source.path, "changed cached attachment").unwrap();
	assert!(store.attachment(&attached).is_err());
}

#[test]
fn existing_plain_text_records_keep_their_saved_delivery_after_upgrade() {
	let root = tempfile::tempdir().unwrap();
	let store = Store::open(root.path()).unwrap();
	let mut input = request("existing-inline");
	input.prompt = "原始正文".repeat(30_000);
	input.delivery = crate::Delivery::Inline;
	let record = store.prepare(input).unwrap();
	let original = record.wire_prompt();
	let mut old = serde_json::to_value(record).unwrap();
	for key in ["delivery", "context"] {
		old["request"].as_object_mut().unwrap().remove(key);
	}
	for key in ["delivery", "attachment"] {
		old["receipt"].as_object_mut().unwrap().remove(key);
	}
	std::fs::write(root.path().join("existing-inline.json"), serde_json::to_vec(&old).unwrap())
		.unwrap();
	let restored = store.load("existing-inline").unwrap();
	assert_eq!(restored.receipt.delivery, crate::Delivery::Inline);
	assert_eq!(restored.wire_prompt(), original);
	assert!(store.attachment(&restored).unwrap().is_none());
}

#[test]
fn async_preparation_is_durable_and_idempotent_while_execution_holds_the_lease() {
	let root = tempfile::tempdir().expect("private root");
	let bridge = ProBridge::new(Config::default(), root.path()).expect("bridge");
	assert_eq!(bridge.prepare(request("accepted")).expect("prepare").phase, Phase::Prepared);
	let store = Store::open(root.path()).expect("store");
	let _lease = store.lease().expect("observer lease");
	assert_eq!(bridge.prepare(request("accepted")).expect("replayed call").phase, Phase::Prepared);
	assert!(bridge.prepare(request("other")).is_err());
	let mut changed = request("accepted");
	changed.prompt.push('!');
	assert!(bridge.prepare(changed).is_err());
}

#[test]
fn request_identity_survives_restart_and_rejects_changed_input() {
	let root = tempfile::tempdir().expect("private root");
	let store = Store::open(root.path()).expect("store");
	store.prepare(request("one")).expect("prepare");
	let reopened = Store::open(root.path()).expect("reopen");
	assert_eq!(
		reopened.prepare(request("one")).expect("same input").receipt.phase,
		Phase::Prepared
	);
	let mut changed = request("one");
	changed.prompt.push('!');
	assert!(reopened.prepare(changed).is_err());
	assert!(store.prepare(request("../escape")).is_err());
}

#[test]
fn execution_lease_prevents_parallel_sends_and_releases_without_a_stale_pid_file() {
	let root = tempfile::tempdir().expect("private root");
	let first = Store::open(root.path()).expect("first");
	let second = Store::open(root.path()).expect("second");
	let lease = first.execution_lease().expect("lease").expect("available");
	assert!(second.execution_lease().expect("read lock").is_none());
	assert!(second.lease().is_ok(), "running research must not block journal access");
	drop(lease);
	assert!(second.execution_lease().expect("read lock").is_some());
}

#[tokio::test]
async fn stop_before_worker_start_does_not_lose_cancellation() {
	let root = tempfile::tempdir().expect("private root");
	let bridge = ProBridge::new(Config::default(), root.path()).expect("bridge");
	bridge.prepare(request("one")).expect("prepare");
	bridge.cancel();
	assert!(bridge.resume("one").await.expect_err("owner stopped").contains("owner was stopped"));
	assert_eq!(bridge.status("one").expect("saved receipt").phase, Phase::Prepared);
}

#[test]
fn a_corrupt_record_is_preserved_instead_of_becoming_a_new_request() {
	let root = tempfile::tempdir().expect("private root");
	let store = Store::open(root.path()).expect("store");
	std::fs::write(root.path().join("one.json"), "broken").expect("fixture");
	assert!(store.prepare(request("one")).is_err());
	assert_eq!(std::fs::read_to_string(root.path().join("one.json")).expect("read"), "broken");
}

#[tokio::test]
async fn uncertain_first_send_does_not_start_a_runtime_or_reset_the_fence() {
	let root = tempfile::tempdir().expect("private root");
	let store = Store::open(root.path()).expect("store");
	let mut record = store.prepare(request("one")).expect("record");
	record.receipt.phase = Phase::Sending;
	store.save(&record).expect("save fence");
	let bridge = ProBridge::new(Config::default(), root.path()).expect("bridge");
	let error = bridge.resume("one").await.expect_err("must stop");
	assert!(error.contains("uncertain outcome"));
	assert_eq!(store.load("one").expect("preserved").receipt.phase, Phase::Sending);
	assert!(bridge.resume_at("one", "https://other.test/c/wrong").await.is_err());
	let error = bridge
		.resume_at("one", "https://chatgpt.com/c/candidate")
		.await
		.expect_err("the missing runtime cannot verify this candidate");
	assert!(error.contains("not connected"));
	let fenced = store.load("one").expect("candidate did not change saved identity");
	assert_eq!(fenced.receipt.phase, Phase::Sending);
	assert!(fenced.receipt.conversation_url.is_none());
}

#[tokio::test]
async fn complete_unicode_code_and_links_survive_restart_without_another_model_turn() {
	let root = tempfile::tempdir().expect("private root");
	let store = Store::open(root.path()).expect("store");
	let mut record = store.prepare(request("one")).expect("record");
	let answer = Answer {
		message_id: "assistant-one".into(),
		text: "完整研究结果\n".repeat(10_000),
		code_blocks: vec!["one\ntwo\nthree".into()],
		links: vec![crate::Link {
			label: "source".into(),
			url: "https://example.com/source".into(),
		}],
	};
	record.receipt.phase = Phase::Complete;
	record.receipt.conversation_url = Some("https://chatgpt.com/c/owned-test".into());
	record.target_id = Some("owned-chrome-tab".into());
	record.answer = Some(answer.clone());
	store.save(&record).expect("save complete body");
	let bridge = ProBridge::new(Config::default(), root.path()).expect("bridge");
	assert_eq!(bridge.read("one").expect("full body"), answer);
	assert_eq!(bridge.ask(request("one")).await.expect("cached receipt").phase, Phase::Complete);
	let mut next = request("two");
	next.previous = Some("one".into());
	let continuation = store.prepare(next).expect("continuation");
	assert_eq!(continuation.parent_answer_id.as_deref(), Some("assistant-one"));
	assert_eq!(continuation.receipt.conversation_url, record.receipt.conversation_url);
	assert_eq!(continuation.target_id, record.target_id);
}
