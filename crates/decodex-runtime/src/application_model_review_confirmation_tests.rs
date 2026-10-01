//! Real native inference after explicit model-source confirmation.
use serde_json::Value;
use tokio::{
	io::{AsyncBufReadExt as _, AsyncReadExt as _, AsyncWriteExt as _, BufReader},
	net::TcpListener,
	time,
};

use crate::application::{
	ServiceApplication,
	account_nudge::{native_tests::model_review::*, tests},
};
use decodex_core::ConversationId;
use decodex_protocol::{
	CommandEnvelope, ConversationExecutionSettings, ConversationModel, ConversationReasoningEffort,
	ConversationResult, ConversationSummary, InitialModelSource,
};

pub(super) async fn assert_cold_readback(root: &DecodexRoot, id: &ConversationId) {
	let store = SqliteStore::open(&root.paths()).expect("reopen product database");
	let app = tests::application(store);
	let id = EntityId::new(id.as_str()).expect("conversation");
	let ConversationResult::Available(current) = app.conversation_get(&id).await else {
		panic!("cold conversation readback");
	};

	assert_eq!(current.state, decodex_protocol::ConversationState::Ready);
	assert_eq!(current.conversation_revision, EntityRevision(2));
	assert!(current.codex_thread_id.is_some());
	assert!(current.active_turn_id.is_none());

	assert_native_observation(&current);
}

pub(super) async fn qualify(
	app: &ServiceApplication,
	listener: &TcpListener,
	id: &ConversationId,
	account_id: EntityId,
	account_revision: i64,
) {
	let command = CommandEnvelope {
		version: CURRENT_VERSION,
		client_command_id: ClientCommandId::new("native-review-confirm").expect("command"),
		idempotency_key: IdempotencyKey::new("native-review-confirm").expect("key"),
		expected_revision: Some(EntityRevision(1)),
		correlation_id: CorrelationId::new("native-review-confirm").expect("correlation"),
		causation_id: None,
		payload: CommandPayload::ReviewConversationModelSettings {
			conversation_id: EntityId::new(id.as_str()).expect("conversation"),
			execution: ConversationExecutionSettings::new(
				ConversationModel::new("gpt-5.6-sol").expect("model"),
				ConversationReasoningEffort::High,
				false,
			),
			source: InitialModelSource { account_id, account_revision },
		},
	};
	let requests = std::sync::Mutex::new(Vec::new());
	let release = tokio::sync::Notify::new();

	tokio::select! {
		_ = serve(listener, &requests, &release) => panic!("fixture server stopped"),
		() = async {
			app.execute(&command).await.expect("confirm saved request");

			loop {
				if !requests.lock().expect("requests").is_empty() { break; }

				time::sleep(Duration::from_millis(25)).await;
			}

			app.execute(&command).await.expect("repeat while inference is running");
			release.notify_one();

			loop {
				let publication = app.next_publication().await.expect("native lifecycle publication");

				if let decodex_protocol::EventPayload::ConversationTurnFinished { conversation, outcome, .. } = publication.event {
					assert_eq!(outcome, decodex_protocol::ConversationTurnOutcome::Succeeded);
					assert_eq!(conversation.state, decodex_protocol::ConversationState::Ready);

					assert_native_observation(&conversation);

					break;
				}
			}
			// Discard the first command publication; native lifecycle events still run.
			app.execute(&command).await.expect("repeat confirmation");

			time::sleep(Duration::from_millis(500)).await;
		} => {}
	}

	let requests = requests.lock().expect("requests");

	assert_eq!(requests.len(), 1, "confirmation retry must not infer twice");

	let request = &requests[0];

	assert_eq!(request["model"], "gpt-5.6-sol");
	assert_eq!(request["reasoning"]["effort"], "high");
	assert!(request["input"].to_string().contains("Preserve the original request"));
}

fn assert_native_observation(summary: &ConversationSummary) {
	let native = summary.native_settings.as_deref().expect("native response observation");

	assert_eq!(native.model.as_str(), "gpt-5.6-sol");
	assert_eq!(native.model_provider, "fixture");
	assert!(native.cwd.ends_with("/saved-request"));
	assert_eq!(
		summary.original_working_directory.as_ref().expect("saved directory").as_str(),
		native.cwd
	);
	assert!(native.observed_at_micros > 0);
	assert!(native.source_account_revision.0 > 0);
}

fn response(
	first: &str,
	body: &[u8],
	listener: &TcpListener,
	requests: &std::sync::Mutex<Vec<Value>>,
) -> (&'static str, &'static str, String) {
	if first.starts_with("GET /api/codex/accounts/check ") {
		let address = listener.local_addr().expect("address");

		return ("200 OK", "application/json", serde_json::json!({"accounts":[{"id":"workspace-fixture","workspace_backend_origin":format!("https://{address}"),"account_routing_override":"NO_CONSTRAINT"}]}).to_string());
	}
	if first.starts_with("GET ") {
		return ("404 Not Found", "application/json", "{}".into());
	}

	assert!(first.starts_with("POST /responses "), "unexpected fixture route: {first}");

	requests.lock().expect("requests").push(serde_json::from_slice(body).expect("inference JSON"));

	let frames = [
		serde_json::json!({"type":"response.created","response":{"id":"review-response"}}),
		serde_json::json!({"type":"response.output_item.done","item":{"type":"message","role":"assistant","id":"review-answer","content":[{"type":"output_text","text":"Fixture complete."}]}}),
		serde_json::json!({"type":"response.completed","response":{"id":"review-response","usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}}}),
	];
	let data = frames
		.iter()
		.map(|value| {
			format!("event: {}\ndata: {value}\n\n", value["type"].as_str().expect("event"))
		})
		.collect();

	("200 OK", "text/event-stream", data)
}

async fn serve(
	listener: &TcpListener,
	requests: &std::sync::Mutex<Vec<Value>>,
	release: &tokio::sync::Notify,
) {
	loop {
		let (socket, _) = listener.accept().await.expect("fixture connection");
		let mut socket = BufReader::new(socket);
		let mut first = String::new();

		socket.read_line(&mut first).await.expect("request line");

		let mut length = 0;

		loop {
			let mut line = String::new();

			assert!(socket.read_line(&mut line).await.expect("header") > 0);

			if line == "\r\n" {
				break;
			}

			if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
				length = value.trim().parse::<usize>().expect("body length");
			}
		}

		assert!(length <= 2 * 1_024 * 1_024);

		let mut body = vec![0; length];

		socket.read_exact(&mut body).await.expect("body");

		let (status, content_type, data) = response(&first, &body, listener, requests);

		if first.starts_with("POST /responses ") {
			release.notified().await;
		}

		socket.get_mut().write_all(format!("HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{data}", data.len()).as_bytes()).await.expect("response");
	}
}
