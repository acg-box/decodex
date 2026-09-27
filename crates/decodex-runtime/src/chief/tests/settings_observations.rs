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

	async fn reply(
		&mut self,
		chief: &ChiefCoordinator,
		method: &'static str,
		thread: &str,
		response: Value,
	) {
		let client = chief.client.clone();
		let params = json!({"threadId":thread});
		let pending = tokio::spawn(async move {
			match method {
				"thread/resume" => client.thread_resume(params).await,
				"thread/read" => client.thread_read(params).await,
				_ => panic!("fixture method"),
			}
		});
		let mut line = String::new();
		tokio::time::timeout(
			std::time::Duration::from_secs(2),
			BufReader::new(&mut self.remote).read_line(&mut line),
		)
		.await
		.unwrap()
		.unwrap();
		let request: Value = serde_json::from_str(&line).unwrap();
		assert_eq!(request["method"], method);
		assert_eq!(request["params"]["threadId"], thread);
		self.remote
			.write_all(format!("{}\n", json!({"id":request["id"],"result":response})).as_bytes())
			.await
			.unwrap();
		pending.await.unwrap().unwrap();
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

fn assert_projections(values: &[decodex_database::ChiefTaskSettingsObservation; 3]) {
	let model: Value = serde_json::from_str(values[0].settings_json.as_ref().unwrap()).unwrap();
	assert_eq!(
		model,
		json!({"model":"first","modelProvider":"fixture","effort":"high","serviceTier":null})
	);
	let permissions: decodex_codex::app_server_client::NativeTaskPermissions =
		serde_json::from_str(values[1].settings_json.as_ref().unwrap()).unwrap();
	assert_eq!(permissions.profile_id.as_deref(), Some("first"));
	let plugins: Value = serde_json::from_str(values[2].settings_json.as_ref().unwrap()).unwrap();
	assert_eq!(plugins, json!({"disabledPluginIds":["first@market"]}));
	assert_ne!(values[0].id, values[1].id);
	assert_ne!(values[1].id, values[2].id);
}

async fn assert_foreign_generation(
	store: &SqliteStore,
	thread: &str,
	first: &decodex_database::ChiefTaskSettingsObservation,
) {
	let foreign = Some("foreign-generation".to_owned());
	assert!(
		store
			.chief_task_models("chief".into(), thread.into(), foreign.clone())
			.await
			.unwrap()
			.is_none()
	);
	assert!(
		store
			.chief_task_permissions("chief".into(), thread.into(), foreign.clone())
			.await
			.unwrap()
			.is_none()
	);
	assert!(
		store
			.chief_task_plugins("chief".into(), thread.into(), foreign.clone())
			.await
			.unwrap()
			.is_none()
	);
	assert!(
		store
			.record_chief_task_models(
				thread.into(),
				foreign,
				first.settings_json.clone(),
				first.source_digest.clone()
			)
			.await
			.unwrap()
			.is_none()
	);
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
	assert_projections(&first);
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
	let mut empty_plugins = facts("second");
	empty_plugins["disabledPluginIds"] = json!([]);
	wire.publish(&mut chief, &thread, empty_plugins).await;
	let second = records(&chief.store, &thread).await.map(Option::unwrap);
	assert_eq!(
		serde_json::from_str::<Value>(second[2].settings_json.as_ref().unwrap()).unwrap(),
		json!({"disabledPluginIds":[]})
	);
	wire.publish(&mut chief, &thread, facts("first")).await;
	let restored = records(&chief.store, &thread).await.map(Option::unwrap);
	for i in 0..3 {
		assert!(first[i].id < second[i].id && second[i].id < restored[i].id);
		assert_eq!(first[i].settings_json, restored[i].settings_json);
		let event = chief.store.get_chief_inbox_event(restored[i].id).await.unwrap();
		assert!(event.disposition.is_some());
		assert!(!event.payload.contains("private instruction sentinel"));
	}
	let mut malformed_plugins = facts("first");
	malformed_plugins["disabledPluginIds"] = Value::Null;
	wire.publish(&mut chief, &thread, malformed_plugins).await;
	assert!(records(&chief.store, &thread).await[2].as_ref().unwrap().settings_json.is_none());
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
	assert_foreign_generation(&reopened, &thread, &first[0]).await;
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

fn response(thread: &str, model: &str) -> Value {
	let mut value = facts(model);
	value["thread"] = json!({"id":thread});
	value["reasoningEffort"] = Value::Null;
	value.as_object_mut().unwrap().remove("effort");
	value["sandbox"] = value["sandboxPolicy"].take();
	value
}

#[tokio::test]
async fn resumed_wire_settings_require_exact_thread_and_complete_model_facts() {
	let (mut chief, mut sent, _directory) = fixture().await;
	chief.start_chief("chief", "Coordinate").await.unwrap();
	while sent.try_recv().is_ok() {}
	let thread =
		chief.store.get_chief_work_item("chief".into()).await.unwrap().codex_thread_id.unwrap();
	let mut wire = Wire::attach(&mut chief);
	wire.reply(&chief, "thread/resume", &thread, response("foreign", "resumed")).await;
	chief.persist_task_settings(&thread).await.unwrap();
	assert!(records(&chief.store, &thread).await[0].is_none());
	let mut resumed = response(&thread, "resumed");
	wire.reply(&chief, "thread/resume", &thread, resumed.clone()).await;
	chief.persist_task_settings(&thread).await.unwrap();
	let observed = records(&chief.store, &thread).await[0].clone().unwrap();
	let settings: decodex_codex::app_server_client::NativeTaskModelSettings =
		serde_json::from_str(observed.settings_json.as_ref().unwrap()).unwrap();
	assert_eq!(settings.model, "resumed");
	assert_eq!(settings.effort, None);
	assert_eq!(settings.service_tier, None);
	resumed.as_object_mut().unwrap().remove("reasoningEffort");
	wire.reply(&chief, "thread/resume", &thread, resumed).await;
	chief.persist_task_settings(&thread).await.unwrap();
	let invalid = records(&chief.store, &thread).await[0].clone().unwrap();
	assert!(invalid.id > observed.id && invalid.settings_json.is_none());
	wire.assert_no_requests().await;
	assert!(sent.try_recv().is_err());
}

#[tokio::test]
async fn permission_receipt_requires_current_wire_facts_not_queued_payload_or_history() {
	let (mut chief, mut sent, directory) = fixture().await;
	chief.start_chief("chief", "Coordinate").await.unwrap();
	let work = chief.store.get_chief_work_item("chief".into()).await.unwrap();
	let thread = work.codex_thread_id.unwrap();
	chief.store.complete_chief_turn("chief".into(), work.active_turn_id.unwrap()).await.unwrap();
	while sent.try_recv().is_ok() {}
	let mut wire = Wire::attach(&mut chief);
	wire.publish(&mut chief, &thread, facts("readonly")).await;
	let observed = records(&chief.store, &thread).await[1].clone().unwrap();
	let attempt = decodex_database::ChiefPermissionAttempt {
		work: "chief".into(),
		thread: thread.clone(),
		generation: None,
		settings_event: observed.id,
		profile: "scoped".into(),
		review_token: "a".repeat(64),
		attempt_id: "review".into(),
	};
	let id =
		chief.store.reserve_chief_permission_selection(attempt.clone()).await.unwrap().unwrap();
	assert!(
		chief.store.finish_chief_permission_selection(id, attempt, "queued".into()).await.unwrap()
	);
	chief
		.handle_event(ServerEvent::Notification {
			method: "thread/settings/updated".into(),
			params: json!({"threadId":thread,"threadSettings":facts("scoped")}),
		})
		.await
		.unwrap();
	assert_eq!(
		chief
			.store
			.chief_permission_receipt("chief".into(), thread.clone())
			.await
			.unwrap()
			.unwrap()
			.state,
		"queued"
	);
	wire.reply(&chief, "thread/read", &thread, response(&thread, "scoped")).await;
	chief.persist_task_settings(&thread).await.unwrap();
	assert_eq!(
		chief
			.store
			.chief_permission_receipt("chief".into(), thread.clone())
			.await
			.unwrap()
			.unwrap()
			.state,
		"queued"
	);
	assert!(chief.store.begin_chief_dispatch("chief".into()).await.is_err());
	wire.publish(&mut chief, &thread, facts("scoped")).await;
	let root =
		decodex_core::DecodexRoot::new(directory.path().canonicalize().unwrap().join("root"))
			.unwrap();
	let reopened = SqliteStore::open(&root.paths()).unwrap();
	assert_eq!(
		reopened.chief_permission_receipt("chief".into(), thread).await.unwrap().unwrap().state,
		"target_observed"
	);
	assert!(reopened.begin_chief_dispatch("chief".into()).await.is_ok());
	wire.assert_no_requests().await;
	assert!(sent.try_recv().is_err());
}
