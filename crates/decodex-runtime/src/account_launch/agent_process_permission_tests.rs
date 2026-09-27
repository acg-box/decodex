//! Exercise the permission owner against a retained wire source and admitted database fixture.
use super::*;
use decodex_codex::app_server_client::ServerEvent;
use decodex_protocol::AgentPermissionState as State;
use serde_json::{Value, json};
use std::sync::{
	Arc,
	atomic::{AtomicUsize, Ordering},
};
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};

fn facts(profile: &str) -> Value {
	json!({"cwd":"/native","activePermissionProfile":{"id":profile},"approvalPolicy":"on-request","approvalsReviewer":"user","sandboxPolicy":{"type":"readOnly"}})
}
async fn save(owned: &OwnedReviewer, profile: &str) {
	let projected =
		decodex_codex::app_server_client::NativeTaskPermissions::from_notification(&facts(profile))
			.expect("valid permission fixture");
	owned
		.store
		.record_agent_task_permissions(
			"task".into(),
			Some(GENERATION.into()),
			Some(serde_json::to_string(&projected).expect("valid permission fixture")),
			DIGEST.into(),
		)
		.await
		.expect("valid permission fixture")
		.expect("valid permission fixture");
}

async fn serve_permission_fixture(
	remote: tokio::io::DuplexStream,
	count: Arc<AtomicUsize>,
	lost: bool,
) {
	let (reader, mut writer) = tokio::io::split(remote);
	writer.write_all(format!("{}\n",json!({"method":"thread/settings/updated","params":{"threadId":"task","threadSettings":facts("readonly")}})).as_bytes()).await.expect("valid permission wire fixture");
	let mut lines = BufReader::new(reader).lines();
	while let Some(line) = lines.next_line().await.expect("valid permission wire fixture") {
		let request: Value = serde_json::from_str(&line).expect("valid permission wire fixture");
		let result = match request["method"].as_str().expect("valid permission wire fixture") {
			"test/barrier" => json!({}),
			"permissionProfile/list" => {
				assert_eq!(request["params"]["cwd"], "/native");
				json!({"data":[{"id":"scoped","allowed":true},{"id":":full-access","allowed":false}]})
			},
			"thread/settings/update" => {
				count.fetch_add(1, Ordering::AcqRel);
				assert_eq!(request["params"], json!({"threadId":"task","permissions":"scoped"}));
				if lost {
					break;
				}
				json!({})
			},
			other => panic!("unexpected RPC {other}"),
		};
		writer
			.write_all(format!("{}\n", json!({"id":request["id"],"result":result})).as_bytes())
			.await
			.expect("valid permission wire fixture");
	}
}

#[tokio::test]
async fn permission_owner_reviews_allowed_profiles_and_never_replays_queued_or_lost_ack() {
	for lost in [false, true] {
		let home = tempfile::tempdir().unwrap();
		let (local, remote) = tokio::io::duplex(65536);
		let (reader, writer) = tokio::io::split(local);
		let (client, _events) = AppServerClient::from_io(reader, writer);
		let writes = Arc::new(AtomicUsize::new(0));
		let count = writes.clone();
		let server = tokio::spawn(serve_permission_fixture(remote, count, lost));
		client.request("test/barrier", json!({})).await.unwrap();
		let owned = OwnedReviewer::new(home.path(), &client, "task", "turn").await;
		owned.store.complete_agent_turn("root".into(), "turn".into()).await.unwrap();
		save(&owned, "readonly").await;
		let state = crate::agent_permissions::read(&owned.store, || async {
			Some(owned.source(&owned.key))
		})
		.await;
		let State::Available { review_token, profiles, can_update, .. } = state else {
			panic!("{state:?}")
		};
		assert!(can_update);
		let mut changed = owned.key.clone();
		changed.revision += 1;
		assert!(
			crate::agent_permissions::write(
				&owned.store,
				|| async { Some(owned.source(&changed)) },
				"task",
				review_token.as_str(),
				"scoped",
				"changed-account"
			)
			.await
			.is_err()
		);
		assert!(
			crate::agent_permissions::write(
				&owned.store,
				|| async { Some(owned.source(&owned.key)) },
				"replacement",
				review_token.as_str(),
				"scoped",
				"changed-thread"
			)
			.await
			.is_err()
		);
		let reads = AtomicUsize::new(0);
		let changed_during_read = crate::agent_permissions::read(&owned.store, || async {
			Some(owned.source(if reads.fetch_add(1, Ordering::AcqRel) == 0 {
				&owned.key
			} else {
				&changed
			}))
		})
		.await;
		assert!(matches!(changed_during_read, State::Unavailable));

		assert!(!profiles[1].allowed);
		assert!(
			crate::agent_permissions::write(
				&owned.store,
				|| async { Some(owned.source(&owned.key)) },
				"task",
				review_token.as_str(),
				":full-access",
				"disabled"
			)
			.await
			.is_err()
		);
		assert_eq!(writes.load(Ordering::Acquire), 0);
		let result = crate::agent_permissions::write(
			&owned.store,
			|| async { Some(owned.source(&owned.key)) },
			"task",
			review_token.as_str(),
			"scoped",
			"selection",
		)
		.await;
		assert_eq!(result.is_ok(), !lost);
		assert_eq!(writes.load(Ordering::Acquire), 1);
		let state = crate::agent_permissions::read(&owned.store, || async {
			Some(owned.source(&owned.key))
		})
		.await;
		assert!(matches!(state, State::Pending { .. }), "{state:?}");
		assert!(
			crate::agent_permissions::write(
				&owned.store,
				|| async { Some(owned.source(&owned.key)) },
				"task",
				review_token.as_str(),
				"scoped",
				"another-key"
			)
			.await
			.is_err()
		);
		assert_eq!(writes.load(Ordering::Acquire), 1);
		assert!(owned.store.begin_agent_dispatch("root".into()).await.is_err());
		server.abort();
	}
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated installed-native permission controller"]
async fn installed_native_permission_controller_uses_retained_bridge_and_confirms_publication() {
	let binary = std::env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");
	let home = tempfile::tempdir().unwrap();
	let cwd = home.path().canonicalize().unwrap();
	std::fs::create_dir(cwd.join("writable")).unwrap();
	std::fs::write(cwd.join("config.toml"),format!("model = \"gpt-5.6-sol\"\ncli_auth_credentials_store = \"file\"\n[permissions.scoped.filesystem]\n\":root\" = \"read\"\n{} = \"write\"\n",json!(cwd.join("writable")))).unwrap();
	let mut session = super::super::NativeSession::start(&binary, &cwd);
	tokio::time::timeout(std::time::Duration::from_secs(30),async {
		let started=session.client.thread_start(json!({"cwd":cwd,"historyMode":"paginated","sandbox":"read-only","approvalPolicy":"on-request","approvalsReviewer":"user"})).await.unwrap();
		let thread=started["thread"]["id"].as_str().unwrap();
		let owned=OwnedReviewer::new(home.path(),&session.client,thread,"fixture-idle").await;
		owned.store.complete_agent_turn("root".into(),"fixture-idle".into()).await.unwrap();
		let (facts,guard)=session.client.observed_task_permissions(thread).unwrap();assert!(guard.is_live());
		owned.store.record_agent_task_permissions_publication(thread.into(),Some(GENERATION.into()),Some(serde_json::to_string(&facts).unwrap()),DIGEST.into()).await.unwrap().unwrap();
		let source=||async {let mut key=owned.key.clone();key.history_revision=session.client.history_revision();Some(owned.source(&key))};
		let state=crate::agent_permissions::read(&owned.store,source).await;
		let State::Available {review_token,profiles,can_update,..}=state else {panic!("{state:?}")};
		assert!(can_update);assert!(profiles.iter().any(|p|p.id.as_str()=="scoped"&&p.allowed));
		crate::agent_permissions::write(&owned.store,source,thread,review_token.as_str(),"scoped","native-selection").await.unwrap();
		assert_eq!(owned.store.agent_permission_receipt("root".into(),thread.into()).await.unwrap().unwrap().state,"queued");
		loop {
			let event=session.events.recv().await.unwrap();
			if let ServerEvent::Notification {method,params}=event && method=="thread/settings/updated" && params["threadId"]==thread {
				let Some(projected)=decodex_codex::app_server_client::NativeTaskPermissions::from_notification(&params["threadSettings"]) else {continue;};
				if projected.profile_id.as_deref()!=Some("scoped") {continue;}
				let (current,guard)=session.client.observed_task_permissions(thread).unwrap();assert!(guard.is_live());assert_eq!(projected,current);
				owned.store.record_agent_task_permissions_publication(thread.into(),Some(GENERATION.into()),Some(serde_json::to_string(&current).unwrap()),"b".repeat(64)).await.unwrap().unwrap();break;
			}
		}
		let State::Available {profile_id,last_outcome,cwd:observed_cwd,..}=crate::agent_permissions::read(&owned.store,source).await else {panic!("native current state")};
		assert_eq!(profile_id.unwrap().as_str(),"scoped");assert_eq!(last_outcome,Some(decodex_protocol::AgentPermissionOutcome::TargetObserved));assert_eq!(observed_cwd.as_str(),cwd.to_str().unwrap());
		assert!(crate::agent_permissions::write(&owned.store,source,thread,review_token.as_str(),"scoped","replay-key").await.is_err());
		match session.client.thread_latest_turn_id(thread).await {
                Ok(None)=>{},
                Err(decodex_codex::app_server_client::ClientError::Remote(error)) if error.code == -32600 && error.message.contains("is not materialized yet") && error.message.contains("before first user message")=>{},
                other=>panic!("selection unexpectedly created history or history check failed: {other:?}"),
            }
	}).await.unwrap();
}

// Upstream 7b6dd0c7: permission reload must retain the task's session flags.
#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; native session permission preservation"]
async fn installed_native_permission_switch_preserves_session_profiles_and_model() {
	use decodex_codex::app_server_client::ThreadPermissionSelection;
	let binary = std::env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");
	for (on_disk, top_level) in [(false, false), (true, false), (true, true)] {
		let home = tempfile::tempdir().unwrap();
		let config = if on_disk {
			"model=\"gpt-5.6-sol\"\n[permissions.audit]\nextends=\":read-only\"\n"
		} else {
			"model=\"gpt-5.6-sol\"\n"
		};
		std::fs::write(home.path().join("config.toml"), config).unwrap();
		let mut session = super::super::NativeSession::start(&binary, home.path());
		tokio::time::timeout(std::time::Duration::from_secs(30), async {
			let mut flags =
				json!({"features.guardian_approval":true,"model_reasoning_effort":"high"});
			if !on_disk {
				flags["permissions.audit"] = json!({"extends":":read-only"});
			}
			if !top_level {
				flags["default_permissions"] = json!("audit");
			}
			let mut params = json!({"cwd":home.path(),"model":"gpt-5.6-terra","config":flags});
			if top_level {
				params["permissions"] = json!("audit");
			}
			let start = session.client.thread_start(params).await.unwrap();
			let thread = start["thread"]["id"].as_str().unwrap();
			for profile in [":workspace", "audit"] {
				let selection = ThreadPermissionSelection::new(thread, profile).unwrap();
				let (_, guard) = session.client.observed_task_permissions(thread).unwrap();
				session.client.queue_thread_permission_selection(&selection, guard).await.unwrap();
				loop {
					if let ServerEvent::Notification { method, params } =
						session.events.recv().await.unwrap()
						&& method == "thread/settings/updated"
						&& params["threadId"] == thread
						&& params["threadSettings"]["activePermissionProfile"]["id"] == profile
					{
						let settings = &params["threadSettings"];
						assert_eq!(settings["model"], "gpt-5.6-terra");
						assert_eq!(settings["effort"], "high");
						break;
					}
				}
			}
			let independent = session
				.client
				.thread_start(
					json!({"cwd":home.path(),"approvalPolicy":"never","sandbox":"read-only"}),
				)
				.await
				.unwrap();
			assert_eq!(independent["model"], "gpt-5.6-sol");
			assert_ne!(independent["activePermissionProfile"]["id"], "audit");
		})
		.await
		.expect("native permission publication deadline");
		assert_eq!(std::fs::read_to_string(home.path().join("config.toml")).unwrap(), config);
	}
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated ordinary resume inheritance"]
async fn installed_native_ordinary_resume_preserves_saved_settings() {
	let binary = std::env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");
	let home = tempfile::tempdir().unwrap();
	let cwd = home.path().canonicalize().unwrap();
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
	let address = listener.local_addr().unwrap();
	let calls = Arc::new(AtomicUsize::new(0));
	let backend = tokio::spawn(super::super::serve_fixture(
		listener,
		calls.clone(),
		None,
		None,
		Some(json!({"input_tokens":0,"output_tokens":1,"total_tokens":1})),
		|serial| json!({"type":"message","role":"assistant","id":format!("done-{serial}"),"content":[{"type":"output_text","text":"Done"}]}),
	));
	std::fs::write(
		cwd.join("config.toml"),
		format!(
			r#"model = "gpt-5.6-sol"
model_provider = "fixture"
cli_auth_credentials_store = "file"
[model_providers.fixture]
name = "Isolated ordinary resume fixture"
base_url = "http://{address}"
wire_api = "responses"
requires_openai_auth = false
supports_websockets = false
"#
		),
	)
	.unwrap();
	let mut session = super::super::NativeSession::start(&binary, &cwd);
	tokio::time::timeout(std::time::Duration::from_secs(30), async {
		let start = decodex_codex::ConversationThreadStartRequest::new(
			"gpt-5.6-sol",
			cwd.to_str().unwrap(),
			"Keep the original instructions.",
		)
		.unwrap();
		let response =
			session.client.thread_start(serde_json::to_value(&start).unwrap()).await.unwrap();
		let started = decodex_codex::decode_conversation_thread_start_response(
			&start,
			&serde_json::to_vec(&response).unwrap(),
		)
		.unwrap();
		assert_eq!(started.model_provider(), "fixture");
		session
			.client
			.turn_start(
				json!({"threadId":started.thread_id().as_str(),"input":[{"type":"text","text":"Reply Done."}]}),
			)
			.await
			.unwrap();
		super::super::finish(&mut session.events).await;
		assert_eq!(calls.load(Ordering::Acquire), 1);
		let request = decodex_codex::ConversationThreadResumeRequest::new(
			started.thread_id().clone(),
			"stale-client-model",
			cwd.to_str().unwrap(),
			"Stale client instructions.",
		)
		.unwrap()
		.with_fast(true)
		.inherit_native_settings();
		let wire = serde_json::to_value(&request).unwrap();
		assert_eq!(wire, json!({"threadId":started.thread_id().as_str(),"excludeTurns":true}));
		let response = session.client.thread_resume(wire).await.unwrap();
		let resumed = decodex_codex::decode_conversation_thread_resume_response(
			&request,
			&serde_json::to_vec(&response).unwrap(),
		)
		.unwrap();
		assert_eq!(resumed.model().as_str(), "gpt-5.6-sol");
		assert_eq!(resumed.model_provider(), "fixture");
		assert_eq!(resumed.cwd().as_str(), cwd.to_str().unwrap());
		assert_ne!(response["serviceTier"], "priority");
		assert_eq!(calls.load(Ordering::Acquire), 1);
		drop(session);

		// An independent native process moves the saved task and changes its model.
		// Persist one real turn, then verify a third process observes those facts.
		let moved_cwd = cwd.join("moved-project");
		std::fs::create_dir(&moved_cwd).unwrap();
		let mut other = super::super::NativeSession::start(&binary, &cwd);
		other.client.thread_resume(json!({"threadId":started.thread_id().as_str(),"excludeTurns":true,"model":"gpt-5.5","cwd":moved_cwd})).await.unwrap();
		other.client.turn_start(json!({"threadId":started.thread_id().as_str(),"input":[{"type":"text","text":"Reply Done again."}]})).await.unwrap();
		super::super::finish(&mut other.events).await;
		let calls_before_cold_resume = calls.load(Ordering::Acquire);
		assert!(calls_before_cold_resume >= 2);
		drop(other);

		let cold = super::super::NativeSession::start(&binary, &cwd);
		let response = cold.client.thread_resume(serde_json::to_value(&request).unwrap()).await.unwrap();
		let bytes = serde_json::to_vec(&response).unwrap();
		assert_eq!(decodex_codex::decode_conversation_thread_resume_response(&request, &bytes).unwrap_err(), decodex_codex::ConversationContractError::CwdMismatch);
		let reconciled = decodex_codex::ConversationThreadResumeRequest::new(
			started.thread_id().clone(), "stale-client-model", moved_cwd.to_str().unwrap(), "Stale client instructions.",
		).unwrap().inherit_native_settings();
		let resumed = decodex_codex::decode_conversation_thread_resume_response(&reconciled, &bytes).unwrap();
		assert_eq!(resumed.model().as_str(), "gpt-5.5");
		assert_eq!(resumed.model_provider(), "fixture");
		assert_eq!(resumed.cwd().as_str(), moved_cwd.to_str().unwrap());
		assert_eq!(calls.load(Ordering::Acquire), calls_before_cold_resume);
	})
	.await
	.unwrap();
	backend.abort();
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; running builtin permission selection"]
async fn installed_native_running_builtin_permissions_preserve_pending_tools() {
	let binary = std::env::var_os("DECODEX_TEST_CODEX_BINARY").expect("native binary");
	let home = tempfile::tempdir().expect("home");
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("backend");
	let address = listener.local_addr().expect("address");
	let calls = Arc::new(AtomicUsize::new(0));
	let backend = tokio::spawn(super::super::serve_fixture(
		listener,
		calls.clone(),
		None,
		None,
		Some(json!({"input_tokens":1,"output_tokens":1,"total_tokens":2})),
		|n| {
			if n == 0 {
				json!({"type":"function_call","name":"pause_fixture","arguments":"{}","call_id":"pause"})
			} else {
				json!({"type":"message","role":"assistant","id":"done","content":[{"type":"output_text","text":"Done"}]})
			}
		},
	));
	let config = format!(
		"model=\"gpt-5.6-sol\"\nmodel_provider=\"fixture\"\n[permissions.audit]\nextends=\":read-only\"\n[features]\nenable_request_compression=false\n[model_providers.fixture]\nname=\"fixture\"\nbase_url=\"http://{address}\"\nwire_api=\"responses\"\nrequires_openai_auth=false\nsupports_websockets=false\n"
	);
	std::fs::write(home.path().join("config.toml"), &config).expect("config");
	let mut session = super::super::NativeSession::start(&binary, home.path());
	tokio::time::timeout(std::time::Duration::from_secs(30),async {
        let start=session.client.thread_start(json!({"cwd":home.path(),"approvalPolicy":"on-request","sandbox":"read-only","dynamicTools":[{"name":"pause_fixture","description":"Pause local fixture","inputSchema":{"type":"object","properties":{}}}]})).await.expect("start");
        let thread=start["thread"]["id"].as_str().expect("thread");
        let turn=session.client.turn_start(json!({"threadId":thread,"input":[{"type":"text","text":"Pause"}]})).await.expect("turn");
        let turn=turn["turn"]["id"].as_str().expect("turn ID");
        let (request,method,params)=super::super::next_request(&mut session.events).await;
        assert_eq!(method,"item/tool/call");
        let request_guard=session.client.server_request_guard(&request,&method,&params).expect("request guard");
        let owned=OwnedReviewer::new(home.path(),&session.client,thread,turn).await;
        let (facts,_)=session.client.configured_task_permissions(thread).expect("observed permissions");
        owned.store.record_agent_task_permissions_publication(thread.into(),Some(GENERATION.into()),Some(serde_json::to_string(&facts).expect("facts")),DIGEST.into()).await.expect("journal").expect("saved");
        let source=||async {let mut key=owned.key.clone();key.history_revision=session.client.history_revision();Some(owned.source(&key))};
        let State::Available {review_token,profiles,can_update,..}=crate::agent_permissions::read(&owned.store,source).await else {panic!("running state")};
        assert!(can_update);
        assert!(profiles.iter().any(|p|p.id.as_str()=="audit" && p.allowed && p.can_select));
        assert!(profiles.iter().any(|p|p.id.as_str()==":workspace" && p.allowed && p.can_select));
        assert!(owned.store.agent_permission_receipt("root".into(),thread.into()).await.expect("receipt").is_none());
        crate::agent_permissions::write(&owned.store,source,thread,review_token.as_str(),":workspace","builtin-running").await.expect("builtin queue");
        assert_eq!(calls.load(Ordering::Acquire),1,"selection must not release the pending tool");
        loop {
            if let ServerEvent::Notification {method,params}=session.events.recv().await.expect("event")
                && method=="thread/settings/updated" && params["threadId"]==thread
                && params["threadSettings"]["activePermissionProfile"]["id"]==":workspace" {
                let facts=decodex_codex::app_server_client::NativeTaskPermissions::from_notification(&params["threadSettings"]).expect("native permission publication");
                owned.store.record_agent_task_permissions_publication(thread.into(),Some(GENERATION.into()),Some(serde_json::to_string(&facts).expect("facts")),"b".repeat(64)).await.expect("journal").expect("saved");
                break;
            }
        }
        let receipt=SqliteStore::open(&owned.root.paths()).expect("reopen").agent_permission_receipt("root".into(),thread.into()).await.expect("receipt").expect("saved receipt");
        assert_eq!(receipt.state,"target_observed");
        assert_eq!(owned.store.get_agent_work_item("root".into()).await.expect("work").active_turn_id.as_deref(),Some(turn));
        session.client.respond_guarded(request,json!({"contentItems":[{"type":"inputText","text":"Continue"}],"success":true}),request_guard).await.expect("tool reply");
        super::super::finish(&mut session.events).await;
        assert_eq!(calls.load(Ordering::Acquire),2);
    }).await.expect("native permission deadline");
	assert_eq!(std::fs::read_to_string(home.path().join("config.toml")).expect("config"), config);
	drop(session);
	backend.abort();
}
