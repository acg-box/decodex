//! Exact-artifact qualification of the two selected native lookup boundaries.
use std::{
	env, fs,
	sync::{
		Arc,
		atomic::{AtomicUsize, Ordering},
	},
	time::Duration,
};

use tokio::{net::TcpListener, time};

use super::{NativeSession, ServerEvent, serve};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated synthetic native fixture"]
async fn qualified_artifact_reads_item_anchors_and_registered_resource_owners() {
	let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").expect("qualified artifact");
	let home = tempfile::tempdir().unwrap();
	let root = home.path().canonicalize().unwrap();
	let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
	let address = listener.local_addr().unwrap();
	let calls = Arc::new(AtomicUsize::new(0));
	let backend = tokio::spawn(serve(listener, calls.clone()));
	fs::write(root.join("config.toml"), format!("model = \"gpt-5.6-sol\"\nmodel_provider = \"fixture\"\ncli_auth_credentials_store = \"file\"\n[model_providers.fixture]\nname = \"Native lookup fixture\"\nbase_url = \"http://{address}\"\nwire_api = \"responses\"\nrequires_openai_auth = false\nsupports_websockets = false\n")).unwrap();
	let mut session = NativeSession::with_capabilities(
		&binary,
		&root,
		decodex_codex::app_server_client::InitializeCapabilities::for_agent(),
	);
	time::timeout(Duration::from_secs(60), async {
		let first = session.client.thread_start(serde_json::json!({"cwd":root,"historyMode":"paginated","approvalPolicy":"never","sandbox":"read-only"})).await.unwrap();
		let thread = first["thread"]["id"].as_str().unwrap();
		let started = session.client.turn_start(serde_json::json!({"threadId":thread,"input":[{"type":"text","text":"Native lookup input"}]})).await.unwrap();
		let turn = started["turn"]["id"].as_str().unwrap();
		loop {
			if let ServerEvent::Notification { method, params } = session.events.recv().await.unwrap()
				&& method == "turn/completed" && params["turn"]["id"] == turn {
				assert_eq!(params["turn"]["status"], "completed"); break;
			}
		}
		let history = session.client.thread_read_turn(thread, turn).await.unwrap();
		let items = history["thread"]["turns"][0]["items"].as_array().unwrap();
		let user = items.iter().find(|item| item["type"] == "userMessage").unwrap();
		let answer = items.iter().find(|item| item["type"] == "agentMessage").unwrap();
		let window = session.client.thread_item_context(thread, turn, user["id"].as_str().unwrap(), None, false).await.unwrap();
		let visible = window["data"].as_array().unwrap();
		assert_eq!(visible[0]["item"]["id"], user["id"]);
		assert!(visible.iter().any(|entry| entry["item"]["id"] == answer["id"]));
		assert!(visible.iter().all(|entry| entry["turnId"] == turn));
		let second = session.client.thread_start(serde_json::json!({"cwd":root,"historyMode":"paginated","approvalPolicy":"never","sandbox":"read-only"})).await.unwrap();
		let other = second["thread"]["id"].as_str().unwrap();
		let started = session.client.turn_start(serde_json::json!({"threadId":other,"input":[{"type":"text","text":"Persist the second lookup fixture"}]})).await.unwrap();
		let second_turn = started["turn"]["id"].as_str().unwrap();
		loop {
			if let ServerEvent::Notification { method, params } = session.events.recv().await.unwrap()
				&& method == "turn/completed" && params["turn"]["id"] == second_turn {
				assert_eq!(params["turn"]["status"], "completed"); break;
			}
		}

		for owner in [thread, other] {
			session.client.add_thread_attachment(owner, "decodex.link", "exact-resource", serde_json::json!({"url":"https://example.invalid/issue/1"})).await.unwrap();
		}
		session.client.request("thread/archive", serde_json::json!({"threadId":other})).await.unwrap();
		let owners = session.client.thread_attachment_owners("decodex.link", "exact-resource", None, None).await.unwrap();
		assert_eq!(owners.data.len(), 2);
		assert!(owners.data.iter().any(|owner| owner.thread_id == other && owner.archived));
		let archived = session.client.thread_attachment_owners("decodex.link", "exact-resource", Some(true), None).await.unwrap();
		assert_eq!(archived.data.len(), 1); assert_eq!(archived.data[0].thread_id, other);
		assert!(session.client.thread_attachment_owners("decodex.link", "unregistered-resource", None, None).await.unwrap().data.is_empty());
		assert_eq!(calls.load(Ordering::Acquire), 2, "lookup must not start another model turn");
	}).await.expect("bounded native lookup qualification");
	drop(session);
	backend.abort();
}
