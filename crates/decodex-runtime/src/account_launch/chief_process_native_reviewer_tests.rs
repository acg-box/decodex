//! Native active-reviewer changes preserve pending requests and future defaults.
use super::*;
#[path = "chief_process_native_reviewer_store.rs"] mod store;
use decodex_protocol::ChiefAppReviewer as Reviewer;
use std::sync::atomic::AtomicUsize;

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native reviewer routing"]
async fn installed_native_live_reviewer_changes_only_the_selected_turn() {
	qualify(Reviewer::User).await;
	qualify(Reviewer::AutoReview).await;
}

async fn qualify(updated: Reviewer) {
	let binary = std::env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");
	assert!(std::path::Path::new(&binary).is_absolute());
	let home = tempfile::tempdir().expect("native reviewer fixture");
	let listener =
		tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("native reviewer fixture");
	let address = listener.local_addr().expect("native reviewer fixture");
	let requests = Arc::new(AtomicUsize::new(0));
	let bodies = Arc::new(std::sync::Mutex::new(Vec::new()));
	let backend = tokio::spawn(serve_with_output(
		listener,
		requests.clone(),
		Some(bodies.clone()),
		|_| json!({"input_tokens":0,"output_tokens":0,"total_tokens":0}),
		move |serial| output(serial, updated),
	));
	std::fs::write(home.path().join("config.toml"), format!("model = \"gpt-5.6-sol\"\nmodel_provider = \"fixture\"\ncli_auth_credentials_store = \"file\"\n[features]\nguardian_approval = true\nstep_model_switching = false\n[model_providers.fixture]\nname = \"Isolated reviewer fixture\"\nbase_url = \"http://{address}\"\nwire_api = \"responses\"\nrequires_openai_auth = false\nsupports_websockets = false\n")).expect("native reviewer fixture");
	let mut session = NativeSession::start(&binary, home.path());
	tokio::time::timeout(Duration::from_secs(60), async {
		let started = session.client.thread_start(json!({"cwd":home.path(),"historyMode":"paginated","approvalPolicy":"on-request","approvalsReviewer":match updated {Reviewer::User=>Reviewer::AutoReview,Reviewer::AutoReview=>Reviewer::User},"sandbox":"read-only","dynamicTools":[{"name":"pause_fixture","description":"Wait for fixture input","inputSchema":{"type":"object","properties":{}}}]})).await.expect("native reviewer fixture");
		let thread = started["thread"]["id"].as_str().expect("native reviewer fixture");
		let turn = session.client.turn_start(json!({"threadId":thread,"input":[{"type":"text","text":"Run the isolated reviewer fixture."}]})).await.expect("native reviewer fixture");
		let turn = turn["turn"]["id"].as_str().expect("native reviewer fixture");
		let (id, method, params) = next_request(&mut session.events).await;
		assert_eq!(method, "item/tool/call");
		let pending = session.client.server_request_guard(&id, &method, &params).expect("native reviewer fixture");
		let owned=store::OwnedReviewer::new(home.path(),&session.client,thread,turn).await;
		owned.publish(turn,updated).await;
		assert_eq!(requests.load(Ordering::Acquire), 1, "settings update cannot release pending tool");
		assert!(session.client.server_request_guard(&id, &method, &params).is_some());
		session.client.respond_guarded(id, json!({"contentItems":[{"type":"inputText","text":"Continue"}],"success":true}), pending).await.expect("native reviewer fixture");
		if updated==Reviewer::User { decline_command(&session.client,&mut session.events,turn).await; }
		finish(&mut session.events).await;
		owned.completed_target(turn).await;
		assert_eq!(requests.load(Ordering::Acquire), if updated==Reviewer::User {3} else {4});
		let next=session.client.turn_start(json!({"threadId":thread,"input":[{"type":"text","text":"Run the next isolated turn."}]})).await.expect("native reviewer fixture");
		if updated==Reviewer::AutoReview {decline_command(&session.client,&mut session.events,next["turn"]["id"].as_str().expect("native reviewer fixture")).await;}
		finish(&mut session.events).await;
		let bodies = bodies.lock().expect("native reviewer fixture");
		assert_eq!(bodies.len(), 6);
		assert_eq!(bodies.iter().filter(|body| body["client_metadata"]["x-openai-subagent"] == "guardian").count(), 1);
	}).await.expect("native reviewer routing deadline");
	drop(session);
	backend.abort();
}

fn output(serial: usize, updated: Reviewer) -> Value {
	let serial = if updated == Reviewer::AutoReview {
		match serial {
			2 => 4,
			3 => 2,
			4 => 3,
			other => other,
		}
	} else {
		serial
	};
	match serial {
		0 =>
			json!({"type":"function_call","name":"pause_fixture","arguments":"{}","call_id":"pause"}),
		1 | 3 =>
			json!({"type":"function_call","name":"exec_command","arguments":json!({"cmd":"echo reviewer-test","sandbox_permissions":"require_escalated","justification":"isolated routing fixture"}).to_string(),"call_id":format!("command-{serial}")}),
		4 =>
			json!({"type":"message","role":"assistant","id":"review","content":[{"type":"output_text","text":"{\"outcome\":\"deny\"}"}]}),
		_ =>
			json!({"type":"message","role":"assistant","id":format!("done-{serial}"),"content":[{"type":"output_text","text":"Done"}]}),
	}
}

async fn next_request(events: &mut mpsc::Receiver<ServerEvent>) -> (RequestId, String, Value) {
	loop {
		match events.recv().await.expect("native event stream") {
			ServerEvent::Request { id, method, params } => return (id, method, params),
			ServerEvent::Notification { method, params } if method == "turn/completed" =>
				panic!("turn ended before request: {params}"),
			_ => {},
		}
	}
}

async fn finish(events: &mut mpsc::Receiver<ServerEvent>) {
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

async fn decline_command(
	client: &AppServerClient,
	events: &mut mpsc::Receiver<ServerEvent>,
	turn: &str,
) {
	let (id, method, params) = next_request(events).await;
	assert_eq!(method, "item/commandExecution/requestApproval");
	assert_eq!(params["turnId"], turn);
	client.respond(id, json!({"decision":"decline"})).await.expect("explicit fixture decline");
}
