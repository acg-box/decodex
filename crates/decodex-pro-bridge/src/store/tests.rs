use crate::{Answer, Config, Phase, ProBridge, Request};

use super::Store;

fn request(id: &str) -> Request {
	Request { id: id.into(), prompt: "A harmless test question.".into(), previous: None }
}

#[test]
fn large_unicode_material_is_immutable_without_a_fixed_prompt_limit() {
	let root = tempfile::tempdir().expect("private root");
	let store = Store::open(root.path()).expect("store");
	let mut input = request("large-unicode");
	input.prompt = "PRIVATE_PAYLOAD_研究资料_🧪\n".repeat(70_000);
	assert!(input.prompt.len() > 2 * 1024 * 1024);
	let record = store.prepare(input).expect("no arbitrary 64 KiB cap");
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
fn async_preparation_is_durable_and_idempotent_while_execution_holds_the_lease() {
	let root = tempfile::tempdir().expect("private root");
	let bridge = ProBridge::new(Config::chrome("/missing-app"), root.path()).expect("bridge");
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
	let lease = first.lease().expect("lease");
	assert!(second.lease().is_err());
	drop(lease);
	assert!(second.lease().is_ok());
}

#[tokio::test]
async fn stop_before_worker_start_does_not_lose_cancellation() {
	let root = tempfile::tempdir().expect("private root");
	let bridge = ProBridge::new(Config::chrome("/missing-app"), root.path()).expect("bridge");
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
	let bridge = ProBridge::new(Config::chrome("/missing-app"), root.path()).expect("bridge");
	let error = bridge.resume("one").await.expect_err("must stop");
	assert!(error.contains("uncertain outcome"));
	assert_eq!(store.load("one").expect("preserved").receipt.phase, Phase::Sending);
	assert!(bridge.resume_at("one", "https://other.test/c/wrong").await.is_err());
	let error = bridge
		.resume_at("one", "https://chatgpt.com/c/candidate")
		.await
		.expect_err("the missing runtime cannot verify this candidate");
	assert!(error.contains("not installed"));
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
	let bridge = ProBridge::new(Config::chrome("/missing-app"), root.path()).expect("bridge");
	assert_eq!(bridge.read("one").expect("full body"), answer);
	assert_eq!(bridge.ask(request("one")).await.expect("cached receipt").phase, Phase::Complete);
	let mut next = request("two");
	next.previous = Some("one".into());
	let continuation = store.prepare(next).expect("continuation");
	assert_eq!(continuation.parent_answer_id.as_deref(), Some("assistant-one"));
	assert_eq!(continuation.receipt.conversation_url, record.receipt.conversation_url);
	assert_eq!(continuation.target_id, record.target_id);
}
