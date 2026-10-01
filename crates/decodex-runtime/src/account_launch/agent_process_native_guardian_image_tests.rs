//! Cold image history survives while the synchronous Guardian keeps its native text-only profile.
use std::{env, fs, sync::Mutex};

use tokio::{net::TcpListener, time};

use crate::account_launch::agent_process::native_tests::reviewer::*;
use decodex_codex::guardian::{self, ReviewStatus};

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated Guardian image evidence"]
async fn installed_guardian_preserves_native_image_profile_after_restart() {
	let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").expect("native binary");
	let home = tempfile::tempdir().unwrap();
	let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
	let address = listener.local_addr().unwrap();
	let requests = Arc::new(AtomicUsize::new(0));
	let bodies = Arc::new(Mutex::new(Vec::new()));
	let backend = tokio::spawn(serve_fixture(
		listener,
		requests.clone(),
		None,
		Some(bodies.clone()),
		Some(serde_json::json!({"input_tokens":0,"output_tokens":0,"total_tokens":0})),
		|serial| match serial {
			1 =>
				serde_json::json!({"type":"function_call","name":"exec_command","call_id":"image-review","arguments":serde_json::json!({"cmd":"exit 0","sandbox_permissions":"require_escalated","justification":"Isolated image evidence test"}).to_string()}),
			2 =>
				serde_json::json!({"type":"message","role":"assistant","id":"review","content":[{"type":"output_text","text":"{\"outcome\":\"deny\"}"}]}),
			_ =>
				serde_json::json!({"type":"message","role":"assistant","id":"done","content":[{"type":"output_text","text":"Done"}]}),
		},
	));

	fs::write(home.path().join("config.toml"), format!("model = \"gpt-5.6-sol\"\nmodel_provider = \"fixture\"\ncli_auth_credentials_store = \"file\"\n[features]\nguardian_approval = true\nguardian_thread_context = true\nstep_model_switching = false\nenable_request_compression = false\n[model_providers.fixture]\nname = \"OpenAI\"\nbase_url = \"http://{address}\"\nwire_api = \"responses\"\nrequires_openai_auth = false\nsupports_websockets = false\n")).unwrap();

	time::timeout(Duration::from_secs(60), async {
		let mut session = NativeSession::start(&binary, home.path());
		let started = session.client.thread_start(serde_json::json!({"cwd":home.path(),"historyMode":"paginated","approvalPolicy":"on-request","approvalsReviewer":"auto_review","sandbox":"read-only"})).await.unwrap();
		let thread = started["thread"]["id"].as_str().unwrap().to_owned();

		session.client.turn_start(serde_json::json!({"threadId":thread,"input":[
			{"type":"text","text":"Keep the working tree unchanged. These images are evidence."},
			{"type":"image","fileId":"file_guardian_first","detail":"original"},
			{"type":"image","fileId":"file_guardian_second","detail":"high"}
		]})).await.unwrap();

		assert!(!finish(&mut session).await);

		drop(session);

		let mut session = NativeSession::start(&binary, home.path());

		session.client.thread_resume(serde_json::json!({"threadId":thread,"excludeTurns":true})).await.unwrap();

		assert_eq!(requests.load(Ordering::Acquire), 1, "resume must not infer");

		session.client.turn_start(serde_json::json!({"threadId":thread,"input":[{"type":"text","text":"Review the isolated command using the saved evidence."}]})).await.unwrap();

		assert!(finish(&mut session).await);

		let bodies = bodies.lock().unwrap();
		let reviews: Vec<_> = bodies.iter().filter(|body| body["client_metadata"]["x-openai-subagent"] == "guardian").collect();

		assert_eq!(reviews.len(), 1);

		let input = reviews[0]["input"].as_array().unwrap();
		let images: Vec<_> = input.iter().filter_map(|item| item["content"].as_array()).flatten()
			.filter(|part| part["type"] == "input_image").map(|part| part["file_id"].as_str().unwrap()).collect();

		assert!(images.is_empty(), "synchronous Guardian does not admit transcript images");

		let restored_images: Vec<_> = bodies[1]["input"].as_array().unwrap().iter()
			.filter_map(|item| item["content"].as_array()).flatten()
			.filter(|part| part["type"] == "input_image").map(|part| part["file_id"].as_str().unwrap()).collect();

		assert_eq!(restored_images, ["file_guardian_first", "file_guardian_second"]);
		assert!(reviews[0]["input"].to_string().contains("Keep the working tree unchanged."));
		assert_eq!(requests.load(Ordering::Acquire), 4);
	}).await.expect("Guardian image deadline");

	backend.abort();
}

async fn finish(session: &mut NativeSession) -> bool {
	let mut denied = false;

	loop {
		match session.events.recv().await.expect("native Guardian image event") {
			ServerEvent::Request { method, .. } => panic!("unexpected client request: {method}"),
			ServerEvent::Notification { method, params } => {
				assert_ne!(method, "error", "native error: {params}");

				if method.starts_with("item/autoApprovalReview/") {
					denied |= guardian::decode_review(&method, &params)
						.expect("native review event")
						.status
						== ReviewStatus::Denied;
				}
				if method == "turn/completed" {
					assert_eq!(params["turn"]["status"], "completed");

					return denied;
				}
			},
			_ => {},
		}
	}
}
