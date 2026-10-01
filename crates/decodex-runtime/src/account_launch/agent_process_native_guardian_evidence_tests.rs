//! Installed Guardian must receive verified user restrictions after a native process restart.
use std::{env, fs, sync::Mutex};

use crate::account_launch::agent_process::native_tests::{reviewer::*, serve_fixture_usage};
use tokio::{net::TcpListener, time};

use decodex_codex::{guardian, guardian::ReviewStatus};

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated Guardian evidence restart"]
async fn installed_guardian_retains_answer_after_compaction_and_restart() {
	let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");
	let home = tempfile::tempdir().unwrap();
	let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
	let address = listener.local_addr().unwrap();
	let requests = Arc::new(AtomicUsize::new(0));
	let bodies = Arc::new(Mutex::new(Vec::new()));
	let backend = tokio::spawn(serve_fixture_usage(
		listener,
		requests.clone(),
		None,
		Some(bodies.clone()),
		|serial| {
			let tokens = if serial == 1 { 250_000 } else { 0 };

			json!({"input_tokens":tokens,"output_tokens":0,"total_tokens":tokens})
		},
		|serial| match serial {
			0 =>
				json!({"type":"function_call","name":"request_user_input","call_id":"publish-scope","arguments":json!({"questions":[{"id":"publish","header":"Publish","question":"Where may this fixture publish?","options":[{"label":"Private","description":"Private repositories only."},{"label":"Nowhere","description":"Keep local."}]}]}).to_string()}),
			2 =>
				json!({"type":"compaction","id":"guardian-checkpoint","encrypted_content":"isolated Guardian checkpoint"}),
			4 =>
				json!({"type":"function_call","name":"exec_command","call_id":"review-action","arguments":json!({"cmd":"exit 0","sandbox_permissions":"require_escalated","justification":"Isolated evidence test"}).to_string()}),
			5 =>
				json!({"type":"message","role":"assistant","id":"review","content":[{"type":"output_text","text":"{\"outcome\":\"deny\"}"}]}),
			_ =>
				json!({"type":"message","role":"assistant","id":"done","content":[{"type":"output_text","text":"Done"}]}),
		},
	));

	fs::write(home.path().join("config.toml"), format!("model = \"gpt-5.6-sol\"\nmodel_provider = \"fixture\"\nmodel_auto_compact_token_limit = 200000\ncli_auth_credentials_store = \"file\"\n[features]\nguardian_approval = true\nguardian_thread_context = true\ndefault_mode_request_user_input = true\nstep_model_switching = false\nremote_compaction_v2 = false\nenable_request_compression = false\n[model_providers.fixture]\nname = \"OpenAI\"\nbase_url = \"http://{address}\"\nwire_api = \"responses\"\nrequires_openai_auth = false\nsupports_websockets = false\n")).unwrap();

	time::timeout(Duration::from_secs(60), async {
		let mut session = NativeSession::start(&binary, home.path());
		let started = session.client.thread_start(json!({"cwd":home.path(),"historyMode":"paginated","approvalPolicy":"on-request","approvalsReviewer":"auto_review","sandbox":"read-only"})).await.unwrap();
		let thread = started["thread"]["id"].as_str().unwrap().to_owned();

		session.client.turn_start(json!({"threadId":thread,"input":[{"type":"text","text":"Keep the working tree unchanged. Ask where publishing is allowed."}]})).await.unwrap();

		let mut answered = false;

		loop {
			match session.events.recv().await.expect("native Guardian event stream") {
				ServerEvent::Request { id, method, .. } => {
					assert_eq!(method, "item/tool/requestUserInput");
					assert!(!answered);

					session.client.respond(id, json!({"answers":{"publish":{"answers":["Private. Verified fixture answer 1351."]}}})).await.unwrap();

					answered = true;
				},
				ServerEvent::Notification { method, params } if method == "turn/completed" => {
					assert_eq!(params["turn"]["status"], "completed");

					break;
				},
				_ => {},
			}
		}

		assert!(answered);

		session.client.turn_start(json!({"threadId":thread,"input":[{"type":"text","text":"Continue after automatic compaction."}]})).await.unwrap();

		finish_compaction(&mut session).await;
		drop(session);

		let mut session = NativeSession::start(&binary, home.path());

		session.client.thread_resume(json!({"threadId":thread})).await.unwrap();
		session.client.turn_start(json!({"threadId":thread,"input":[{"type":"text","text":"Review the isolated command."}]})).await.unwrap();

		let mut denied = false;

		loop {
			match session.events.recv().await.expect("native Guardian event stream") {
				ServerEvent::Request { method, .. } => panic!("unexpected client request: {method}"),
				ServerEvent::Notification { method, params } if method.starts_with("item/autoApprovalReview/") => {
					denied |= guardian::decode_review(&method, &params).unwrap().status == ReviewStatus::Denied;
				},
				ServerEvent::Notification { method, params } if method == "turn/completed" => {
					assert_eq!(params["turn"]["status"], "completed");

					break;
				},
				_ => {},
			}
		}

		assert!(denied);

		let bodies = bodies.lock().unwrap();

		assert!(bodies[2]["input"].as_array().unwrap().iter().any(|item| item["type"] == "compaction_trigger"));
		assert!(bodies[4]["input"].as_array().unwrap().iter().any(|item| item["type"] == "compaction" && item["encrypted_content"] == "isolated Guardian checkpoint"));

		let reviews: Vec<_> = bodies.iter().filter(|body| body["client_metadata"]["x-openai-subagent"] == "guardian").collect();

		assert_eq!(reviews.len(), 1);

		let input = reviews[0]["input"].to_string();

		assert!(input.contains("Keep the working tree unchanged."));
		assert!(input.contains("Private. Verified fixture answer 1351."));
		assert_eq!(requests.load(Ordering::Acquire), 7);
	}).await.expect("Guardian evidence deadline");

	backend.abort();
}

async fn finish_compaction(session: &mut NativeSession) {
	let mut compacted = false;

	loop {
		match session.events.recv().await.expect("native Guardian event stream") {
			ServerEvent::Notification { method, params } => {
				assert_ne!(method, "error", "compaction failed: {params}");

				if method == "item/completed" && params["item"]["type"] == "contextCompaction" {
					compacted = true;
				}
				if method == "turn/completed" {
					assert_eq!(params["turn"]["status"], "completed");
					assert!(compacted);

					return;
				}
			},
			ServerEvent::Request { method, .. } => panic!("unexpected compact request: {method}"),
			_ => {},
		}
	}
}
