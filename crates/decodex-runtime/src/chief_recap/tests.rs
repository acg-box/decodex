use super::*;
use decodex_codex::app_server_client::AppServerClient;
use decodex_core::{AccountId, ProcessGenerationId};
use serde_json::json;

fn source() -> (Source, tokio::io::DuplexStream, mpsc::Receiver<ServerEvent>) {
	let (local, remote) = tokio::io::duplex(4096);
	let (read, write) = tokio::io::split(local);
	let (client, events) = AppServerClient::from_io(read, write);
	(
		Source {
			client,
			key: crate::chief_usage_estimate::SourceKey {
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
	parse(r#"{"summary":"Tested but not installed", "next_action":null}"#).expect("valid recap")
}

#[test]
fn recap_requires_nullable_next_action_and_character_bounds() {
	assert!(parse(r#"{"summary":"done"}"#).is_none());
	assert!(parse(r#"{"summary":"done","next_action":null,"extra":true}"#).is_none());
	let long = json!({"summary":"字".repeat(700),"next_action":"步".repeat(200)}).to_string();
	assert!(parse(&long).is_some());
	assert!(parse(&json!({"summary":"字".repeat(701),"next_action":null}).to_string()).is_none());
	assert!(parse(r#"{"summary":"  ","next_action":null}"#).is_none());
	assert_eq!(
		parse(r#"{"summary":" done ","next_action":" "}"#).expect("trimmed recap").next_action,
		None
	);
}

#[tokio::test]
async fn exact_cancel_and_source_changes_never_replace_a_newer_request() {
	let (source, _remote, _events) = source();
	let recaps = Recaps::default();
	let cancel = recaps.start(copy(&source), "one").expect("first request");
	recaps.cancel_request("work", "other");
	assert!(!*cancel.borrow());
	recaps.cancel_request("work", "one");
	assert!(*cancel.borrow());
	assert!(recaps.start(copy(&source), "two").is_err());
	recaps.finish("work", "one", None, Some(result()));
	assert_eq!(
		recaps.status(EntityId::new("work").expect("id"), Some(&source)).phase,
		Phase::Cancelled
	);
	let _second = recaps.start(copy(&source), "two").expect("cleanup finished");
	recaps.finish("work", "one", None, Some(result()));
	assert_eq!(
		recaps.status(EntityId::new("work").expect("id"), Some(&source)).phase,
		Phase::Pending
	);
	recaps.finish("work", "two", None, Some(result()));
	assert_eq!(
		recaps.status(EntityId::new("work").expect("id"), Some(&source)).phase,
		Phase::Ready
	);
	let mut changed = copy(&source);
	changed.key.revision += 1;
	let state = recaps.status(EntityId::new("work").expect("id"), Some(&changed));
	assert_eq!(state.phase, Phase::Cancelled);
	assert!(state.recap.is_none());
	assert!(state.is_valid());
}

#[tokio::test]
async fn temporary_events_are_private_and_new_user_input_invalidates_only_its_source() {
	let (source, _remote, _events) = source();
	let recaps = Recaps::default();
	let cancelled = recaps.start(copy(&source), "one").expect("request");
	let mut routed = recaps.register("temporary").expect("route");
	let event = |thread: &str, method: &str| ServerEvent::Notification {
		method: method.into(),
		params: json!({"threadId":thread,"item":{"type":"userMessage"}}),
	};
	assert!(recaps.route(event("temporary", "turn/completed")).is_none());
	assert!(routed.recv().await.is_some());
	assert!(!*cancelled.borrow());
	assert!(recaps.route(event("other", "thread/reverted")).is_some());
	assert!(!*cancelled.borrow());
	assert!(recaps.route(event("native", "item/completed")).is_some());
	assert!(*cancelled.borrow());
}

#[tokio::test]
async fn restarting_the_transient_owner_does_not_restore_or_replay_a_recap() {
	let (source, _remote, _events) = source();
	let recaps = Recaps::default();
	let _cancel = recaps.start(copy(&source), "one").expect("request");
	let mut other = copy(&source);
	other.key.work = "other".into();
	assert!(recaps.start(other, "two").is_err());
	recaps.finish("work", "one", None, Some(result()));
	let restarted = Recaps::default();
	let state = restarted.status(EntityId::new("work").expect("id"), Some(&source));
	assert_eq!(state.phase, Phase::Idle);
	assert!(state.recap.is_none());
	assert!(state.is_valid());
}

#[tokio::test]
async fn transport_observed_changes_hide_ready_results_before_service_event_delivery() {
	use tokio::io::AsyncWriteExt as _;
	let (source, mut remote, mut events) = source();
	let recaps = Recaps::default();
	let cancelled = recaps.start(copy(&source), "one").expect("request");
	recaps.finish("work", "one", None, Some(result()));
	remote.write_all(format!("{}\n",json!({"method":"turn/started","params":{"threadId":"native","turn":{"id":"new","status":"inProgress"}}})).as_bytes()).await.expect("native notification");
	let _queued = events.recv().await.expect("transport observed event");
	// The service has not routed this event. Its readonly query still rejects the stale result.
	let status = recaps.status(EntityId::new("work").expect("id"), Some(&source));
	assert_eq!(status.phase, Phase::Cancelled);
	assert!(status.recap.is_none());
	assert!(!*cancelled.borrow(), "query did not send a cancellation effect");
}
