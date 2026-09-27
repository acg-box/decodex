//! Native wire publications reach the existing durable settings owners without dispatch.
use super::*;

fn facts(model: &str) -> Value {
	json!({"model":model,"modelProvider":"fixture","effort":"high","serviceTier":null,
        "cwd":"/fixture","activePermissionProfile":{"id":model},"approvalPolicy":"on-request",
        "approvalsReviewer":"user","sandboxPolicy":{"type":"readOnly"},
        "disabledPluginIds":[format!("{model}@market")],
        "collaborationMode":{"settings":{"developer_instructions":"private instruction sentinel"}}})
}

struct Wire {
	remote: tokio::io::DuplexStream,
	events: tokio::sync::mpsc::Receiver<ServerEvent>,
}
impl Wire {
	fn attach(chief: &mut ChiefCoordinator) -> Self {
		let (local, remote) = tokio::io::duplex(32768);
		let (reader, writer) = tokio::io::split(local);
		let (client, events) = AppServerClient::from_io(reader, writer);
		chief.client = client;
		Self { remote, events }
	}

	async fn publish(&mut self, chief: &mut ChiefCoordinator, thread: &str, settings: Value) {
		let event = json!({"method":"thread/settings/updated","params":{"threadId":thread,"threadSettings":settings}});
		self.remote.write_all(format!("{event}\n").as_bytes()).await.unwrap();
		let event = tokio::time::timeout(std::time::Duration::from_secs(2), self.events.recv())
			.await
			.unwrap()
			.unwrap();
		chief.handle_event(event).await.unwrap();
	}

	async fn assert_no_requests(&mut self) {
		use tokio::io::AsyncReadExt as _;
		let mut byte = [0];
		assert!(
			tokio::time::timeout(std::time::Duration::from_millis(20), self.remote.read(&mut byte))
				.await
				.is_err(),
			"settings observation must not write to the native process"
		);
	}
}
async fn records(
	store: &SqliteStore,
	thread: &str,
) -> [Option<decodex_database::ChiefTaskSettingsObservation>; 3] {
	[
		store.chief_task_models("chief".into(), thread.into(), None).await.unwrap(),
		store.chief_task_permissions("chief".into(), thread.into(), None).await.unwrap(),
		store.chief_task_plugins("chief".into(), thread.into(), None).await.unwrap(),
	]
}

#[tokio::test]
async fn wire_settings_preserve_transitions_privacy_and_reopen_without_dispatch() {
	let (mut chief, mut sent, directory) = fixture().await;
	chief.start_chief("chief", "Coordinate").await.unwrap();
	while sent.try_recv().is_ok() {}
	let thread =
		chief.store.get_chief_work_item("chief".into()).await.unwrap().codex_thread_id.unwrap();
	let mut wire = Wire::attach(&mut chief);
	wire.publish(&mut chief, "foreign", facts("first")).await;
	assert!(records(&chief.store, &thread).await.iter().all(Option::is_none));
	wire.publish(&mut chief, &thread, facts("first")).await;
	let first = records(&chief.store, &thread).await.map(Option::unwrap);
	chief.persist_task_settings(&thread).await.unwrap();
	assert_eq!(
		records(&chief.store, &thread).await,
		first.clone().map(Some),
		"same source read must deduplicate"
	);
	wire.publish(&mut chief, &thread, facts("first")).await;
	let repeated = records(&chief.store, &thread).await.map(Option::unwrap);
	for i in 0..3 {
		assert!(repeated[i].id > first[i].id, "a new wire revision must invalidate old reviews");
		assert_eq!(repeated[i].settings_json, first[i].settings_json);
	}
	wire.publish(&mut chief, &thread, facts("second")).await;
	let second = records(&chief.store, &thread).await.map(Option::unwrap);
	wire.publish(&mut chief, &thread, facts("first")).await;
	let restored = records(&chief.store, &thread).await.map(Option::unwrap);
	for i in 0..3 {
		assert!(first[i].id < second[i].id && second[i].id < restored[i].id);
		assert_eq!(first[i].settings_json, restored[i].settings_json);
		let event = chief.store.get_chief_inbox_event(restored[i].id).await.unwrap();
		assert!(event.disposition.is_some());
		assert!(!event.payload.contains("private instruction sentinel"));
	}
	let mut incomplete = facts("first");
	for field in ["serviceTier", "approvalsReviewer", "disabledPluginIds"] {
		incomplete.as_object_mut().unwrap().remove(field);
	}
	wire.publish(&mut chief, &thread, incomplete).await;
	let invalid = records(&chief.store, &thread).await.map(Option::unwrap);
	for i in 0..3 {
		assert!(invalid[i].id > restored[i].id);
		assert!(invalid[i].settings_json.is_none());
	}
	assert!(chief.store.list_pending_chief_events(100).await.unwrap().is_empty());
	assert!(chief.store.list_chief_wake_events("chief".into(), 100).await.unwrap().is_empty());
	assert!(sent.try_recv().is_err());
	wire.assert_no_requests().await;
	let root =
		decodex_core::DecodexRoot::new(directory.path().canonicalize().unwrap().join("root"))
			.unwrap();
	let reopened = SqliteStore::open(&root.paths()).unwrap();
	assert_eq!(records(&reopened, &thread).await, invalid.map(Some));
	assert!(
		reopened
			.chief_task_models("chief".into(), thread, Some("foreign-generation".into()))
			.await
			.unwrap()
			.is_none()
	);
}

#[tokio::test]
async fn queued_notification_payload_cannot_replace_newer_wire_settings() {
	let (mut chief, mut sent, _directory) = fixture().await;
	chief.start_chief("chief", "Coordinate").await.unwrap();
	while sent.try_recv().is_ok() {}
	let thread =
		chief.store.get_chief_work_item("chief".into()).await.unwrap().codex_thread_id.unwrap();
	let mut wire = Wire::attach(&mut chief);
	wire.publish(&mut chief, &thread, facts("current")).await;
	let current = records(&chief.store, &thread).await;
	chief
		.handle_event(ServerEvent::Notification {
			method: "thread/settings/updated".into(),
			params: json!({"threadId":thread,"threadSettings":facts("stale")}),
		})
		.await
		.unwrap();
	assert_eq!(records(&chief.store, &thread).await, current);
	wire.assert_no_requests().await;
	assert!(sent.try_recv().is_err());
}

#[test]
fn hydrated_settings_accept_native_changes_but_not_foreign_or_malformed_replies() {
	let valid = json!({"thread":{"id":"exact"},"model":"native-replacement","reasoningEffort":"future-effort"});
	assert!(ChiefCoordinator::hydrated_thread_matches(&valid, "exact"));
	assert!(!ChiefCoordinator::hydrated_thread_matches(&valid, "foreign"));
	for (field, value) in [
		("model", Value::Null),
		("model", json!("")),
		("model", json!("\n")),
		("reasoningEffort", json!(42)),
		("reasoningEffort", json!("\n")),
	] {
		let mut bad = valid.clone();
		bad[field] = value;
		assert!(!ChiefCoordinator::hydrated_thread_matches(&bad, "exact"));
	}
	let mut unset = valid.clone();
	unset["reasoningEffort"] = Value::Null;
	assert!(ChiefCoordinator::hydrated_thread_matches(&unset, "exact"));
	unset.as_object_mut().unwrap().remove("reasoningEffort");
	assert!(!ChiefCoordinator::hydrated_thread_matches(&unset, "exact"));
}
