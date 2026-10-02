//! Native active-reviewer changes preserve pending requests and future defaults.
#[path = "agent_process_native_guardian_evidence_tests.rs"] mod evidence;
#[path = "agent_process_native_guardian_image_tests.rs"] mod image_evidence;
#[path = "agent_process_native_reviewer_store.rs"] mod store;

use std::{
	env, fs,
	path::Path,
	sync::{
		Arc, Mutex,
		atomic::{AtomicUsize, Ordering},
	},
	time::Duration,
};

use serde_json::Value;
use tokio::{net::TcpListener, sync::mpsc::Receiver, time};

use crate::account_launch::agent_process::native_tests::{self, NativeSession, serve_fixture};
use decodex_codex::{
	app_server_client::{
		AppServerClient, LiveReviewer, LiveSettingsOutcome, RequestId, ServerEvent,
	},
	guardian,
};
use decodex_protocol::AgentReviewer;
use store::OwnedReviewer;

pub(super) async fn select_permission(client: &AppServerClient, home: &Path, thread: &str) {
	let owned = OwnedReviewer::new(home, client, thread, "fixture-active").await;

	owned.select_permission().await;
}

pub(super) async fn trust_hook(client: &AppServerClient, home: &Path, thread: &str) {
	let owned = OwnedReviewer::new(home, client, thread, "fixture-active").await;

	owned.trust_hook().await;
}

pub(super) async fn select_task_model(
	client: &AppServerClient,
	home: &Path,
	thread: &str,
	model: &str,
	effort: Option<&str>,
) {
	let owned = OwnedReviewer::new(home, client, thread, "fixture-active").await;

	owned.select_task_model(model, effort).await;
}

fn output(serial: usize) -> Value {
	match serial {
		0 =>
			serde_json::json!({"type":"function_call","name":"pause_fixture","arguments":"{}","call_id":"pause"}),
		1 | 3 =>
			serde_json::json!({"type":"function_call","name":"exec_command","arguments":serde_json::json!({"cmd":"echo reviewer-test","sandbox_permissions":"require_escalated","justification":"isolated routing fixture"}).to_string(),"call_id":format!("command-{serial}")}),
		4 =>
			serde_json::json!({"type":"message","role":"assistant","id":"review","content":[{"type":"output_text","text":"{\"outcome\":\"deny\"}"}]}),
		_ =>
			serde_json::json!({"type":"message","role":"assistant","id":format!("done-{serial}"),"content":[{"type":"output_text","text":"Done"}]}),
	}
}

fn assert_guardian_tool_isolation(bodies: &[Value]) {
	fn advertised(body: &Value) -> &Value {
		body["input"]
			.as_array()
			.expect("native input")
			.iter()
			.find(|item| item["type"] == "additional_tools")
			.expect("native tool declarations")
	}

	assert!(advertised(&bodies[0]).to_string().contains("decodex_fixture_mutation"));

	let guardian: Vec<_> = bodies
		.iter()
		.filter(|body| body["client_metadata"]["x-openai-subagent"] == "guardian")
		.collect();

	assert_eq!(guardian.len(), 2);

	for body in &guardian {
		let declarations = advertised(body).to_string();

		assert!(declarations.contains("exec_command"));
		assert!(!declarations.contains("decodex_fixture_mutation"));
	}

	let rejected = guardian[1]["input"]
		.as_array()
		.expect("Guardian input")
		.iter()
		.find(|item| item["type"] == "function_call_output" && item["call_id"] == "excluded-tool")
		.expect("excluded tool response");

	assert_eq!(rejected["output"], "unsupported call: decodex_fixture_mutation");
}

fn direction_output(serial: usize, updated: AgentReviewer) -> Value {
	let serial = if updated == AgentReviewer::AutoReview {
		match serial {
			2 => 4,
			3 => 2,
			4 => 3,
			other => other,
		}
	} else {
		serial
	};

	output(serial)
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native reviewer routing"]
async fn installed_native_live_reviewer_changes_only_the_selected_turn() {
	let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");

	assert!(std::path::Path::new(&binary).is_absolute());

	let home = tempfile::tempdir().unwrap();
	let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
	let address = listener.local_addr().unwrap();
	let requests = Arc::new(AtomicUsize::new(0));
	let bodies = Arc::new(Mutex::new(Vec::new()));
	let backend = tokio::spawn(native_tests::serve_fixture(
		listener,
		requests.clone(),
		None,
		Some(bodies.clone()),
		None,
		output,
	));

	fs::write(home.path().join("config.toml"), format!("model = \"gpt-5.6-sol\"\nmodel_provider = \"fixture\"\ncli_auth_credentials_store = \"file\"\n[features]\nguardian_approval = true\nstep_model_switching = false\n[model_providers.fixture]\nname = \"Isolated reviewer fixture\"\nbase_url = \"http://{address}\"\nwire_api = \"responses\"\nrequires_openai_auth = false\nsupports_websockets = false\n")).unwrap();

	let mut session = NativeSession::start(&binary, home.path());

	time::timeout(Duration::from_secs(60), async {
		let started = session.client.thread_start(serde_json::json!({"cwd":home.path(),"historyMode":"paginated","approvalPolicy":"on-request","approvalsReviewer":"auto_review","sandbox":"read-only","dynamicTools":[{"name":"pause_fixture","description":"Wait for fixture input","inputSchema":{"type":"object","properties":{}}}]})).await.unwrap();
		let thread = started["thread"]["id"].as_str().unwrap();
		let turn = session.client.turn_start(serde_json::json!({"threadId":thread,"input":[{"type":"text","text":"Run the isolated reviewer fixture."}]})).await.unwrap();
		let turn = turn["turn"]["id"].as_str().unwrap();
		let (id, method, params) = next_request(&mut session.events).await;

		assert_eq!(method, "item/tool/call");

		let pending = session.client.server_request_guard(&id, &method, &params).unwrap();
		let owned = OwnedReviewer::new(home.path(), &session.client, thread, turn).await;

		owned.publish(turn, AgentReviewer::User).await;

		assert_eq!(requests.load(Ordering::Acquire), 1, "settings update cannot release pending tool");
		assert!(session.client.server_request_guard(&id, &method, &params).is_some());

		session.client.respond_guarded(id, serde_json::json!({"contentItems":[{"type":"inputText","text":"Continue"}],"success":true}), pending).await.unwrap();

		let (id, method, params) = next_request(&mut session.events).await;

		assert_eq!(method, "item/commandExecution/requestApproval");
		assert_eq!(params["turnId"], turn);

		let pending_approval = session.client.server_request_guard(&id, &method, &params).unwrap();
		let guard = session.client.history_guard(session.client.history_revision()).unwrap();

		assert_eq!(session.client.update_live_reviewer(thread, turn, LiveReviewer::AutoReview, guard).await.unwrap(), LiveSettingsOutcome::Applied);
		assert_eq!(requests.load(Ordering::Acquire), 2, "reviewer publication cannot release an existing user approval");
		assert!(session.client.server_request_guard(&id, &method, &params).is_some());

		let guard = session.client.history_guard(session.client.history_revision()).unwrap();

		assert_eq!(session.client.update_live_reviewer(thread, turn, LiveReviewer::User, guard).await.unwrap(), LiveSettingsOutcome::Applied);
		assert_eq!(requests.load(Ordering::Acquire), 2);

		session.client.respond_guarded(id, serde_json::json!({"decision":"decline"}), pending_approval).await.unwrap();

		finish(&mut session.events).await;

		let guard = session.client.history_guard(session.client.history_revision()).unwrap();

		assert_eq!(session.client.update_live_reviewer(thread, turn, LiveReviewer::AutoReview, guard).await.unwrap(), LiveSettingsOutcome::TargetUnavailable);

		owned.completed_target(turn).await;

		assert_eq!(requests.load(Ordering::Acquire), 3);

		session.client.turn_start(serde_json::json!({"threadId":thread,"input":[{"type":"text","text":"Run the next isolated turn."}]})).await.unwrap();

		finish(&mut session.events).await;

		let bodies = bodies.lock().unwrap();

		assert_eq!(bodies.len(), 6);
		assert_eq!(bodies.iter().filter(|body| body["client_metadata"]["x-openai-subagent"] == "guardian").count(), 1);
	}).await.expect("native reviewer routing deadline");

	drop(session);

	backend.abort();
}

async fn next_request(events: &mut Receiver<ServerEvent>) -> (RequestId, String, Value) {
	loop {
		match events.recv().await.expect("native event stream") {
			ServerEvent::Request { id, method, params } => return (id, method, params),
			ServerEvent::Notification { method, params } if method == "turn/completed" =>
				panic!("turn ended before request: {params}"),
			_ => {},
		}
	}
}

async fn finish(events: &mut Receiver<ServerEvent>) {
	loop {
		match events.recv().await.expect("native event stream") {
			ServerEvent::Request { method, .. } =>
				panic!("unexpected request after live override: {method}"),
			ServerEvent::Notification { method, params } if method == "turn/completed" => {
				assert_eq!(params["turn"]["status"], "completed");

				return;
			},
			_ => {},
		}
	}
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated large Guardian action"]
async fn installed_native_guardian_preserves_large_action() {
	let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");

	assert!(std::path::Path::new(&binary).is_absolute());

	let home = tempfile::tempdir().expect("native Guardian fixture");
	let listener = TcpListener::bind("127.0.0.1:0").await.expect("fixture listener");
	let address = listener.local_addr().expect("fixture address");
	let requests = Arc::new(AtomicUsize::new(0));
	let bodies = Arc::new(Mutex::new(Vec::new()));
	let command = format!("true # {} END_OF_COMPLETE_ACTION", "a".repeat(300_000));
	let emitted = command.clone();
	let backend = tokio::spawn(native_tests::serve_fixture(
		listener,
		requests.clone(),
		None,
		Some(bodies.clone()),
		Some(serde_json::json!({"input_tokens":0,"output_tokens":0,"total_tokens":0})),
		move |serial| match serial {
			0 =>
				serde_json::json!({"type":"function_call","name":"exec_command","arguments":serde_json::json!({"cmd":emitted,"sandbox_permissions":"require_escalated","justification":"Isolated large action fixture"}).to_string(),"call_id":"large-command"}),
			1 =>
				serde_json::json!({"type":"function_call","name":"decodex_fixture_mutation","arguments":"{}","call_id":"excluded-tool"}),
			2 =>
				serde_json::json!({"type":"message","role":"assistant","id":"review","content":[{"type":"output_text","text":"{\"outcome\":\"deny\"}"}]}),
			_ =>
				serde_json::json!({"type":"message","role":"assistant","id":"done","content":[{"type":"output_text","text":"Done"}]}),
		},
	));

	fs::write(home.path().join("config.toml"), format!("model = \"gpt-5.6-sol\"\nmodel_provider = \"fixture\"\ncli_auth_credentials_store = \"file\"\n[features]\nguardian_approval = true\nstep_model_switching = false\n[model_providers.fixture]\nname = \"Isolated Guardian fixture\"\nbase_url = \"http://{address}\"\nwire_api = \"responses\"\nrequires_openai_auth = false\nsupports_websockets = false\n")).expect("fixture config");

	let config_path = home.path().join("config.toml");
	let mut config = fs::read_to_string(&config_path).unwrap();

	config.push_str("\n[auto_review]\npolicy = '  Isolated fixture policy.  '\nexperimental_policy_template = '  Fixture Guardian template: {{ tenant_policy_config }}  '\n");

	fs::write(&config_path, config).unwrap();

	let mut session = NativeSession::start(&binary, home.path());

	time::timeout(Duration::from_secs(60), async {
		let started = session.client.thread_start(serde_json::json!({"cwd":home.path(),"historyMode":"paginated","approvalPolicy":"on-request","approvalsReviewer":"auto_review","sandbox":"read-only","dynamicTools":[{"name":"decodex_fixture_mutation","description":"Isolated parent tool","inputSchema":{"type":"object","properties":{}}}]})).await.expect("native thread");
		let thread = started["thread"]["id"].as_str().expect("thread id");

		session.client.turn_start(serde_json::json!({"threadId":thread,"input":[{"type":"text","text":"Review the isolated large action."}]})).await.expect("native turn");

		let mut reviews = Vec::new();

		loop {
			match session.events.recv().await.expect("native event") {
				ServerEvent::Notification { method, params } if method.starts_with("item/autoApprovalReview/") => {
					let review = guardian::decode_review(&method, &params).expect("complete native review decodes");

					assert!(serde_json::to_vec(&review.event).expect("event JSON").len() > 256 * 1_024);
					assert!(review.event["action"].to_string().contains(&command), "native action was truncated");

					reviews.push(review);
				},
				ServerEvent::Notification { method, params } if method == "turn/completed" => {
					assert_eq!(params["turn"]["status"], "completed");

					break;
				},
				ServerEvent::Request { method, .. } => panic!("unexpected user approval: {method}"),
				_ => {},
			}
		}

		assert_eq!(reviews.len(), 2);
		assert_eq!(reviews[1].status, decodex_codex::guardian::ReviewStatus::Denied);

		let bodies = bodies.lock().expect("fixture bodies");
		let guardian = bodies.iter().find(|body| body["client_metadata"]["x-openai-subagent"] == "guardian").expect("native Guardian request");
		let rendered_request = guardian.to_string();

		assert!(rendered_request.contains("Fixture Guardian template: Isolated fixture policy."));
		assert!(!rendered_request.contains("{{ tenant_policy_config }}"));

		let complete_action = guardian["input"].as_array().expect("Guardian messages").iter().any(|message| {
			message["content"].as_array().is_some_and(|parts| {
				parts.iter().filter_map(|part| part["text"].as_str()).collect::<String>().contains(&command)
			})
		});

		assert!(complete_action, "Guardian input was truncated across content parts");

		assert_guardian_tool_isolation(&bodies);

		assert_eq!(requests.load(Ordering::Acquire), 4);
	}).await.expect("native Guardian deadline");

	drop(session);

	backend.abort();
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native reviewer routing"]
async fn installed_native_live_reviewer_preserves_both_default_directions() {
	qualify_direction(AgentReviewer::User).await;
	qualify_direction(AgentReviewer::AutoReview).await;
}

async fn qualify_direction(updated: AgentReviewer) {
	let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");

	assert!(std::path::Path::new(&binary).is_absolute());

	let home = tempfile::tempdir().expect("native reviewer fixture");
	let listener = TcpListener::bind("127.0.0.1:0").await.expect("native reviewer fixture");
	let address = listener.local_addr().expect("native reviewer fixture");
	let requests = Arc::new(AtomicUsize::new(0));
	let bodies = Arc::new(Mutex::new(Vec::new()));
	let backend = tokio::spawn(native_tests::serve_fixture(
		listener,
		requests.clone(),
		None,
		Some(bodies.clone()),
		Some(serde_json::json!({"input_tokens":0,"output_tokens":0,"total_tokens":0})),
		move |serial| direction_output(serial, updated),
	));

	fs::write(home.path().join("config.toml"), format!("model = \"gpt-5.6-sol\"\nmodel_provider = \"fixture\"\ncli_auth_credentials_store = \"file\"\n[features]\nguardian_approval = true\nstep_model_switching = false\n[model_providers.fixture]\nname = \"Isolated reviewer fixture\"\nbase_url = \"http://{address}\"\nwire_api = \"responses\"\nrequires_openai_auth = false\nsupports_websockets = false\n")).expect("native reviewer fixture");

	let mut session = NativeSession::start(&binary, home.path());

	time::timeout(Duration::from_secs(60), async {
		let started = session.client.thread_start(serde_json::json!({"cwd":home.path(),"historyMode":"paginated","approvalPolicy":"on-request","approvalsReviewer":match updated {AgentReviewer::User=>AgentReviewer::AutoReview,AgentReviewer::AutoReview=>AgentReviewer::User},"sandbox":"read-only","dynamicTools":[{"name":"pause_fixture","description":"Wait for fixture input","inputSchema":{"type":"object","properties":{}}}]})).await.expect("native reviewer fixture");
		let thread = started["thread"]["id"].as_str().expect("native reviewer fixture");
		let turn = session.client.turn_start(serde_json::json!({"threadId":thread,"input":[{"type":"text","text":"Run the isolated reviewer fixture."}]})).await.expect("native reviewer fixture");
		let turn = turn["turn"]["id"].as_str().expect("native reviewer fixture");
		let (id, method, params) = next_request(&mut session.events).await;

		assert_eq!(method, "item/tool/call");

		let pending = session.client.server_request_guard(&id, &method, &params).expect("native reviewer fixture");
		let owned=OwnedReviewer::new(home.path(),&session.client,thread,turn).await;

		owned.observe_model().await;
		owned.publish(turn,updated).await;

		assert_eq!(requests.load(Ordering::Acquire), 1, "settings update cannot release pending tool");
		assert!(session.client.server_request_guard(&id, &method, &params).is_some());

		session.client.respond_guarded(id, serde_json::json!({"contentItems":[{"type":"inputText","text":"Continue"}],"success":true}), pending).await.expect("native reviewer fixture");

		if updated==AgentReviewer::User { decline_command(&session.client,&mut session.events,turn).await; }

		finish(&mut session.events).await;

		owned.completed_target(turn).await;
		owned.observe_model().await;

		assert_eq!(requests.load(Ordering::Acquire), if updated==AgentReviewer::User {3} else {4});

		let next=session.client.turn_start(serde_json::json!({"threadId":thread,"input":[{"type":"text","text":"Run the next isolated turn."}]})).await.expect("native reviewer fixture");

		if updated==AgentReviewer::AutoReview {decline_command(&session.client,&mut session.events,next["turn"]["id"].as_str().expect("native reviewer fixture")).await;}

		finish(&mut session.events).await;

		let bodies = bodies.lock().expect("native reviewer fixture");

		assert_eq!(bodies.len(), 6);
		assert_eq!(bodies.iter().filter(|body| body["client_metadata"]["x-openai-subagent"] == "guardian").count(), 1);
	}).await.expect("native reviewer routing deadline");

	drop(session);

	backend.abort();
}

async fn decline_command(client: &AppServerClient, events: &mut Receiver<ServerEvent>, turn: &str) {
	let (id, method, params) = next_request(events).await;

	assert_eq!(method, "item/commandExecution/requestApproval");
	assert_eq!(params["turnId"], turn);

	client
		.respond(id, serde_json::json!({"decision":"decline"}))
		.await
		.expect("explicit fixture decline");
}
