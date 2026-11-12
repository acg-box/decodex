#[cfg(test)] use tokio::io::AsyncWriteExt as _;
use tokio::io::{self, DuplexStream};

use crate::{
	agent_recap::{self, EntityId, Recaps, ServerEvent, Source, TaskRecap, TaskRecapPhase},
	agent_usage_estimate::SourceKey,
};
use decodex_codex::app_server_client::AppServerClient;
use decodex_core::{AccountId, ProcessGenerationId};
use tokio::sync::mpsc::Receiver;

fn source() -> (Source, DuplexStream, Receiver<ServerEvent>) {
	let (local, remote) = io::duplex(4_096);
	let (read, write) = io::split(local);
	let (client, events) = AppServerClient::from_io(read, write);

	(
		Source {
			client,
			key: SourceKey {
				generation: ProcessGenerationId::new("10000000-0000-4000-8000-000000000001")
					.expect("fixture id"),
				account: AccountId::new("20000000-0000-4000-8000-000000000002")
					.expect("fixture id"),
				revision: 1,
				history_revision: 0,
				thread: "native".into(),
				work: "work".into(),
			},
		},
		remote,
		events,
	)
}
fn copy(source: &Source) -> Source {
	Source { client: source.client.clone(), key: source.key.clone() }
}
fn result() -> TaskRecap {
	agent_recap::parse(r#"{"summary":"Tested but not installed", "next_action":null}"#)
		.expect("valid recap")
}

#[tokio::test]
async fn cold_recap_reads_native_metadata_without_resuming_the_parent() {
	use tokio::io::{AsyncBufReadExt as _, BufReader};

	let (source, remote, _events) = source();
	assert!(source.client.configured_task_permissions("native").is_none());
	let (finished, finish) = tokio::sync::oneshot::channel();
	let server = tokio::spawn(async move {
		let (read, mut write) = io::split(remote);
		let mut lines = BufReader::new(read).lines();
		let responses = [
			serde_json::json!({"thread":{"id":"native","model":"source-model","modelProvider":"source-provider","reasoningEffort":"high","cwd":"/tmp/recap-source"}}),
			serde_json::json!({"thread":{"id":"native","historyMode":"legacy"}}),
			serde_json::json!({"thread":{"id":"native","turns":[{"id":"turn-one","status":"completed","items":[
				{"type":"userMessage","content":[{"type":"text","text":"Fix the issue; do not install it."}]},
				{"type":"agentMessage","text":"The fix is tested. Installation remains pending."}
			]}]}}),
		];
		for response in responses {
			let request: serde_json::Value =
				serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
			assert_eq!(
				request["method"], "thread/read",
				"recap must not resume or change its source"
			);
			assert_eq!(request["params"]["threadId"], "native");
			write
				.write_all(
					format!("{}\n", serde_json::json!({"id":request["id"],"result":response}))
						.as_bytes(),
				)
				.await
				.unwrap();
		}
		let _ = finish.await;
	});
	let prepared =
		agent_recap::prepare(&source, None).await.expect("cold history can be summarized");
	assert_eq!(prepared.options.model, "source-model");
	assert_eq!(prepared.options.model_provider, "source-provider");
	assert_eq!(prepared.options.cwd, "/tmp/recap-source");
	assert_eq!(prepared.options.active_permission_profile, None);
	assert!(prepared.options.mcp_server_names.is_empty());
	assert!(prepared.prompt.contains("Installation remains pending"));
	assert!(prepared.guard.is_live());
	assert!(source.client.configured_task_permissions("native").is_none());
	let _ = finished.send(());
	server.await.unwrap();
}

#[test]
fn recap_requires_nullable_next_action_and_character_bounds() {
	assert!(agent_recap::parse(r#"{"summary":"done"}"#).is_none());
	assert!(agent_recap::parse(r#"{"summary":"done","next_action":null,"extra":true}"#).is_none());

	let long =
		serde_json::json!({"summary":"字".repeat(700),"next_action":"步".repeat(200)}).to_string();

	assert!(agent_recap::parse(&long).is_some());
	assert!(
		agent_recap::parse(
			&serde_json::json!({"summary":"字".repeat(701),"next_action":null}).to_string()
		)
		.is_none()
	);
	assert!(agent_recap::parse(r#"{"summary":"  ","next_action":null}"#).is_none());
	assert_eq!(
		agent_recap::parse(r#"{"summary":" done ","next_action":" "}"#)
			.expect("trimmed recap")
			.next_action,
		None
	);
}

#[tokio::test]
async fn exact_cancel_and_source_changes_never_replace_a_newer_request() {
	let (source, _remote, _events) = source();
	let recaps = Recaps::default();
	let cancel = recaps.start(copy(&source), "one", Default::default()).expect("first request");

	recaps.cancel_request("work", "other");

	assert!(!*cancel.borrow());

	recaps.cancel_request("work", "one");

	assert!(*cancel.borrow());
	assert!(recaps.start(copy(&source), "two", Default::default()).is_err());

	recaps.finish("work", "one", Some(result()));

	assert_eq!(
		recaps.status(EntityId::new("work").expect("id"), Some(&source)).phase,
		TaskRecapPhase::Cancelled
	);

	let _second = recaps.start(copy(&source), "two", Default::default()).expect("cleanup finished");

	recaps.finish("work", "one", Some(result()));

	assert_eq!(
		recaps.status(EntityId::new("work").expect("id"), Some(&source)).phase,
		TaskRecapPhase::Pending
	);

	recaps.finish("work", "two", Some(result()));

	assert_eq!(
		recaps.status(EntityId::new("work").expect("id"), Some(&source)).phase,
		TaskRecapPhase::Ready
	);

	let mut changed = copy(&source);

	changed.key.revision += 1;

	let state = recaps.status(EntityId::new("work").expect("id"), Some(&changed));

	assert_eq!(state.phase, TaskRecapPhase::Cancelled);
	assert!(state.recap.is_none());
	assert!(state.is_valid());
}

#[tokio::test]
async fn new_user_input_invalidates_only_its_recap_source() {
	let (source, _remote, _events) = source();
	let recaps = Recaps::default();
	let cancelled = recaps.start(copy(&source), "one", Default::default()).expect("request");
	let event = |thread: &str| ServerEvent::Notification {
		method: "item/completed".into(),
		params: serde_json::json!({"threadId":thread,"item":{"type":"userMessage"}}),
	};
	recaps.observe(&event("other"));
	assert!(!*cancelled.borrow());
	recaps.observe(&event("native"));
	assert!(*cancelled.borrow());
}

#[tokio::test]
async fn restarting_the_transient_owner_does_not_restore_or_replay_a_recap() {
	let (source, _remote, _events) = source();
	let recaps = Recaps::default();
	let _cancel = recaps.start(copy(&source), "one", Default::default()).expect("request");
	let mut other = copy(&source);

	other.key.work = "other".into();

	assert!(recaps.start(other, "two", Default::default()).is_err());

	recaps.finish("work", "one", Some(result()));

	let restarted = Recaps::default();
	let state = restarted.status(EntityId::new("work").expect("id"), Some(&source));

	assert_eq!(state.phase, TaskRecapPhase::Idle);
	assert!(state.recap.is_none());
	assert!(state.is_valid());
}

#[tokio::test]
async fn transport_observed_changes_hide_ready_results_before_service_event_delivery() {
	let (source, mut remote, mut events) = source();
	let recaps = Recaps::default();
	let cancelled = recaps.start(copy(&source), "one", Default::default()).expect("request");

	recaps.finish("work", "one", Some(result()));
	remote.write_all(format!("{}\n",serde_json::json!({"method":"turn/started","params":{"threadId":"native","turn":{"id":"new","status":"inProgress"}}})).as_bytes()).await.expect("native notification");

	let _queued = events.recv().await.expect("transport observed event");
	// The service has not routed this event. Its readonly query still rejects the stale result.
	let status = recaps.status(EntityId::new("work").expect("id"), Some(&source));

	assert_eq!(status.phase, TaskRecapPhase::Cancelled);
	assert!(status.recap.is_none());
	assert!(!*cancelled.borrow(), "query did not send a cancellation effect");
}

#[tokio::test]
async fn voice_transcripts_retire_a_recap_before_service_routing_without_a_native_turn() {
	for (method, role, field) in [
		("thread/realtime/transcript/delta", "user", "delta"),
		("thread/realtime/transcript/done", "user", "text"),
		("thread/realtime/transcript/done", "assistant", "text"),
	] {
		let (source, mut remote, mut events) = source();
		let recaps = Recaps::default();
		let cancelled =
			recaps.start(copy(&source), "voice-recap", Default::default()).expect("request");

		recaps.finish("work", "voice-recap", Some(result()));

		for (thread, text, expected) in [
			("another-thread", "Spoken correction", TaskRecapPhase::Ready),
			("native", "", TaskRecapPhase::Ready),
			("native", "Spoken correction", TaskRecapPhase::Cancelled),
		] {
			let mut params = serde_json::json!({"threadId":thread,"role":role});

			params[field] = serde_json::json!(text);

			remote
				.write_all(
					format!("{}\n", serde_json::json!({"method":method,"params":params}))
						.as_bytes(),
				)
				.await
				.expect("native voice notification");

			let event = events.recv().await.expect("transport observed caption");
			let state = recaps.status(EntityId::new("work").expect("work"), Some(&source));

			assert_eq!(state.phase, expected, "{method} {role} {thread}");
			assert!(!*cancelled.borrow(), "readonly status must not cancel native inference");
			recaps.observe(&event);
		}

		assert!(*cancelled.borrow(), "service routing retires the exact recap");
	}
}
#[test]
fn unicode_excerpt_keeps_answer_and_latest_correction_ends_within_full_prompt_budget() {
	use super::{excerpts, prompt};
	use crate::agent_message_history::excerpts::Exchange;
	let exchange = |user: String, assistant: String| Exchange { user, assistant };
	let exchanges = vec![
		exchange("old".repeat(20_000), "old answer".into()),
		exchange(
			format!("START{}END", "中".repeat(30_000)),
			format!("FIXED{}NOT INSTALLED", "汉".repeat(30_000)),
		),
		exchange(format!("LATEST{}CORRECTION", "字".repeat(20_000)), String::new()),
	];
	let history = excerpts::render(&exchanges);
	let prompt = prompt::build(&history);

	assert!(prompt.len() <= prompt::MAX_BYTES);

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
