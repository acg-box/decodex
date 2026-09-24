//! Qualify the installed catalog route without assuming upstream-main endpoint support.
//! Reference: openai/codex 595cc91e8cbb1c2ca822d0311dcf12709410c582,
//! model-provider/src/models_endpoint.rs and app-server model_list tests.
use super::*;
use std::sync::Mutex;

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated provider catalog qualification"]
async fn installed_catalog_discovery_reports_endpoint_and_opt_in_behavior() {
	for (explicit, enabled) in [(true, true), (false, true), (true, false)] {
		tokio::time::timeout(Duration::from_secs(45), qualify(explicit, enabled))
			.await
			.expect("bounded catalog fixture");
	}
}

async fn qualify(explicit: bool, enabled: bool) {
	let binary = std::env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit binary");
	let home = tempfile::tempdir_in("/tmp").expect("fixture home");
	let catalog = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("catalog listener");
	let catalog_address = catalog.local_addr().expect("catalog address");
	let catalog_calls = Arc::new(Mutex::new(Vec::new()));
	let catalog_server = tokio::spawn(serve(catalog, catalog_calls.clone(), true));
	let inference = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("inference listener");
	let inference_address = inference.local_addr().expect("inference address");
	let inference_calls = Arc::new(Mutex::new(Vec::new()));
	let inference_server = tokio::spawn(serve(inference, inference_calls.clone(), false));
	let catalog_setting = if explicit {
		format!("model_catalog_url=\"http://{catalog_address}/provider-catalog?fixture=yes\"\n")
	} else {
		String::new()
	};
	std::fs::write(home.path().join("config.toml"),format!("model=\"catalog-model\"\nmodel_provider=\"fixture\"\n[features]\nenable_request_compression=false\napi_key_model_discovery={enabled}\n[model_providers.fixture]\nname=\"OpenAI\"\nbase_url=\"http://{inference_address}\"\n{catalog_setting}wire_api=\"responses\"\nrequires_openai_auth=true\nsupports_websockets=false\n")).expect("config");
	std::fs::write(
		home.path().join("auth.json"),
		json!({"OPENAI_API_KEY":"synthetic-catalog-token"}).to_string(),
	)
	.expect("synthetic API-key auth");
	let mut session = NativeSession::start(&binary, home.path());
	let decodex_protocol::ChiefCapabilitiesResult::Available { models, .. } =
		crate::chief_capabilities::read(&session.client).await
	else {
		panic!("native catalog result")
	};
	let found = models.iter().any(|model| model.model.as_str() == "catalog-model");
	let explicit_used = !catalog_calls.lock().expect("catalog calls").is_empty();
	let legacy_used = !inference_calls.lock().expect("inference calls").is_empty();
	assert!(!(explicit_used && legacy_used), "one native metadata authority");
	assert_eq!(found, explicit_used || legacy_used);
	assert!(!explicit_used || explicit && enabled);
	assert!(!legacy_used || enabled);
	if explicit && enabled {
		assert!(found, "native catalog discovery must have an observable owner");
	}
	eprintln!(
		"catalog explicit={explicit} enabled={enabled}: {}",
		if explicit_used {
			"explicit endpoint"
		} else if legacy_used {
			"legacy inference endpoint; model_catalog_url not honored"
		} else {
			"bundled catalog"
		}
	);
	if found {
		qualify_inference(&mut session, home.path(), &inference_calls, explicit_used).await;
	}
	drop(session);
	assert!(!catalog_server.is_finished(), "catalog route/auth assertions passed");
	assert!(!inference_server.is_finished(), "inference route/auth assertions passed");
	assert!(catalog_calls.lock().expect("calls").iter().all(|v| v["method"] == "GET"));
	catalog_server.abort();
	inference_server.abort();
}

async fn qualify_inference(
	session: &mut NativeSession,
	home: &std::path::Path,
	calls: &Mutex<Vec<Value>>,
	explicit: bool,
) {
	assert!(
		calls.lock().expect("calls").iter().all(|v| v["method"] == "GET"),
		"discovery does not start inference"
	);
	let response = session
		.client
		.thread_start(
			json!({"cwd":home,"model":"catalog-model","approvalPolicy":"never","sandbox":"read-only"}),
		)
		.await
		.expect("thread");
	let thread = response["thread"]["id"].as_str().expect("thread id");
	session
		.client
		.turn_start(
			json!({"threadId":thread,"input":[{"type":"text","text":"Return fixture output"}]}),
		)
		.await
		.expect("turn");
	loop {
		if let ServerEvent::Notification { method, params } =
			session.events.recv().await.expect("event")
			&& method == "turn/completed"
			&& params["threadId"] == thread
		{
			assert_eq!(params["turn"]["status"], "completed");
			break;
		}
	}
	let calls = calls.lock().expect("calls");
	let bodies: Vec<_> = calls.iter().filter(|v| v["method"] == "POST").collect();
	assert_eq!(bodies.len(), 1);
	assert_eq!(bodies[0]["body"]["model"], "catalog-model");
	assert_eq!(bodies[0]["body"]["instructions"], instructions(explicit));
}

fn instructions(explicit: bool) -> &'static str {
	if explicit { "Explicit catalog instructions." } else { "Legacy catalog instructions." }
}

async fn serve(listener: tokio::net::TcpListener, calls: Arc<Mutex<Vec<Value>>>, explicit: bool) {
	use tokio::io::{AsyncBufReadExt as _, AsyncReadExt as _, AsyncWriteExt as _};
	while let Ok((socket, _)) = listener.accept().await {
		let mut socket = tokio::io::BufReader::new(socket);
		let mut line = String::new();
		socket.read_line(&mut line).await.expect("request");
		let mut parts = line.split_whitespace();
		let method = parts.next().expect("method").to_owned();
		let target = parts.next().expect("target").to_owned();
		let mut length = 0;
		let mut authenticated = false;
		loop {
			line.clear();
			assert!(socket.read_line(&mut line).await.expect("header") > 0);
			if line == "\r\n" {
				break;
			}
			if let Some((name, value)) = line.split_once(':') {
				if name.eq_ignore_ascii_case("content-length") {
					length = value.trim().parse::<usize>().expect("length");
				}
				if name.eq_ignore_ascii_case("authorization") {
					assert_eq!(value.trim(), "Bearer synthetic-catalog-token");
					authenticated = true;
				}
			}
		}
		assert!(authenticated);
		assert!(length <= 2 * 1024 * 1024);
		let mut bytes = vec![0; length];
		socket.read_exact(&mut bytes).await.expect("body");
		let body = if bytes.is_empty() {
			Value::Null
		} else {
			serde_json::from_slice(&bytes).expect("JSON")
		};
		calls.lock().expect("calls").push(json!({"method":method,"target":target,"body":body}));
		let (content_type, response) = if method == "GET" {
			assert!(target.starts_with(if explicit { "/provider-catalog?" } else { "/models?" }));
			assert!(target.contains("client_version="));
			if explicit {
				assert!(target.contains("fixture=yes"));
			}
			let mut model = effort::fixture_model("catalog-model", "high");
			model["model_messages"]["instructions_template"] = json!(instructions(explicit));
			("application/json", json!({"models":[model]}).to_string())
		} else {
			assert!(!explicit);
			assert_eq!(method, "POST");
			assert_eq!(target, "/responses");
			let frames = [
				json!({"type":"response.created","response":{"id":"fixture"}}),
				json!({"type":"response.completed","response":{"id":"fixture","usage":{"input_tokens":1,"output_tokens":0,"total_tokens":1}}}),
			];
			(
				"text/event-stream",
				frames
					.iter()
					.map(|v| format!("event: {}\ndata: {v}\n\n", v["type"].as_str().expect("type")))
					.collect(),
			)
		};
		let response = format!(
			"HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",
			response.len()
		);
		socket.get_mut().write_all(response.as_bytes()).await.expect("response");
	}
}
