//! Native catalog identity follows account changes; token rotation alone keeps its owner.
//! Upstream: 2b842962883f2e01526b9c64e383cb2375123a5d and models_identity.rs at 595cc91.
use super::*;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use std::sync::Mutex;
use tokio::io::{AsyncBufReadExt as _, AsyncReadExt as _, AsyncWriteExt as _};

const FIRST: &str = "123e4567-e89b-42d3-a456-426614174011";
const SECOND: &str = "123e4567-e89b-42d3-a456-426614174012";

struct AuthSession {
	client: AppServerClient,
	events: mpsc::Receiver<ServerEvent>,
	child: tokio::process::Child,
}
impl AuthSession {
	async fn start(binary: &std::ffi::OsStr, home: &std::path::Path) -> Self {
		let mut child = tokio::process::Command::new(binary)
			.arg("app-server")
			.env_clear()
			.env("HOME", home)
			.env("CODEX_HOME", home)
			.env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
			.current_dir(home)
			.stdin(Stdio::piped())
			.stdout(Stdio::piped())
			.stderr(Stdio::null())
			.kill_on_drop(true)
			.spawn()
			.expect("native fixture");
		let (client, events) = AppServerClient::from_io(
			child.stdout.take().expect("stdout"),
			child.stdin.take().expect("stdin"),
		);
		client.initialize(json!({"clientInfo":{"name":"decodex_catalog_auth_fixture","version":"0.1"},"capabilities":{"experimentalApi":true}})).await.expect("initialize");
		Self { client, events, child }
	}
}

fn token(account: &str, serial: u8) -> String {
	let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none","typ":"JWT"}"#);
	let claims = json!({"email":"fixture@example.invalid","serial":serial,"https://api.openai.com/auth":{"chatgpt_user_id":"fixture-user","chatgpt_account_id":account,"chatgpt_plan_type":"pro"}});
	format!(
		"{header}.{}.c2lnbmF0dXJl",
		URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).expect("claims"))
	)
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native account catalog identity"]
async fn installed_catalog_auth_change_reports_refresh_behavior() {
	tokio::time::timeout(Duration::from_secs(45), qualify())
		.await
		.expect("bounded catalog auth fixture");
}

async fn qualify() {
	let binary = std::env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit binary");
	let home = tempfile::tempdir_in("/tmp").expect("fixture home");
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("listener");
	let address = listener.local_addr().expect("address");
	let calls = Arc::new(Mutex::new(Vec::new()));
	let server = tokio::spawn(serve(listener, calls.clone()));
	std::fs::write(home.path().join("config.toml"),format!("model=\"catalog-auth-model\"\nmodel_provider=\"fixture\"\nchatgpt_base_url=\"http://{address}/backend-api\"\n[features]\nenable_request_compression=false\n[model_providers.fixture]\nname=\"OpenAI\"\nbase_url=\"http://{address}\"\nrequires_openai_auth=true\nsupports_websockets=false\nrequest_max_retries=0\nstream_max_retries=0\n")).expect("config");
	let mut session = AuthSession::start(&binary, home.path()).await;
	login(&session, FIRST, 1).await;
	let started = session.client.thread_start(json!({"cwd":home.path(),"model":"catalog-auth-model","approvalPolicy":"never","sandbox":"read-only"})).await.expect("thread");
	let thread = started["thread"]["id"].as_str().expect("thread id");
	assert_eq!(run_turn(&mut session, thread).await, context_window(FIRST));
	let mut expected_accounts = vec![FIRST];
	// Do not fetch a catalog before the next turn: qualify native refresh first.
	for (account, serial) in [(SECOND, 1), (FIRST, 2)] {
		login(&session, account, serial).await;
		let window = run_turn(&mut session, thread).await;
		expected_accounts.push(account);
		if window == context_window(account) {
			eprintln!("catalog account change: native before-turn refresh");
		} else {
			// The installed alpha uses the bundled unknown-model context when identity changes.
			// An arbitrary stale or absent value is not an accepted version disposition.
			assert_eq!(window, 258_400, "known bundled unknown-model fallback");
			eprintln!("catalog account change: bundled fallback until explicit model/list");
			let decodex_protocol::ChiefCapabilitiesResult::Available { models, .. } =
				crate::chief_capabilities::read(&session.client).await
			else {
				panic!("fresh native catalog")
			};
			assert!(models.iter().any(|m| m.model.as_str() == "catalog-auth-model"));
			assert_eq!(run_turn(&mut session, thread).await, context_window(account));
			expected_accounts.push(account);
		}
	}
	let calls = calls.lock().expect("calls").clone();
	let inference: Vec<_> = calls.iter().filter(|v| v["kind"] == "inference").collect();
	assert_eq!(inference.len(), expected_accounts.len());
	for (call, account) in inference.iter().zip(expected_accounts) {
		assert_eq!(call["account"], account);
		assert_eq!(
			call["instructions"],
			instructions(FIRST),
			"session base instructions remain stable across catalog refresh"
		);
	}
	assert!(calls.iter().any(|v| v["kind"] == "catalog" && v["account"] == FIRST));
	assert!(calls.iter().any(|v| v["kind"] == "catalog" && v["account"] == SECOND));
	assert!(!home.path().join("auth.json").exists(), "external auth remains in memory");
	session.child.kill().await.expect("stop fixture");
	session.child.wait().await.expect("reap fixture");
	drop(session);
	assert!(!server.is_finished(), "native route assertions passed");
	server.abort();
}

async fn login(session: &AuthSession, account: &str, serial: u8) {
	session.client.request("account/login/start",json!({"type":"chatgptAuthTokens","accessToken":token(account,serial),"chatgptAccountId":account,"chatgptPlanType":"pro"})).await.expect("synthetic login");
}
async fn run_turn(session: &mut AuthSession, thread: &str) -> u64 {
	let mut window = None;
	session
		.client
		.turn_start(
			json!({"threadId":thread,"input":[{"type":"text","text":"Return fixture result"}]}),
		)
		.await
		.expect("turn");
	loop {
		if let ServerEvent::Notification { method, params } =
			session.events.recv().await.expect("event")
			&& params["threadId"] == thread
		{
			if method == "thread/tokenUsage/updated" {
				window = params["tokenUsage"]["modelContextWindow"].as_u64();
			}
			if method == "turn/completed" {
				assert_eq!(params["turn"]["status"], "completed");
				return window.expect("native context window observation");
			}
		}
	}
}
fn context_window(account: &str) -> u64 {
	if account == FIRST { 120_000 } else { 240_000 }
}
fn instructions(account: &str) -> String {
	format!("Catalog instructions for {account}.")
}

async fn serve(listener: tokio::net::TcpListener, calls: Arc<Mutex<Vec<Value>>>) {
	while let Ok((socket, _)) = listener.accept().await {
		let mut socket = tokio::io::BufReader::new(socket);
		let Some((target, account, body)) = request(&mut socket).await else { continue };
		let auxiliary = target.starts_with("/backend-api/")
			&& !target.contains("/accounts/check")
			&& !target.contains("/settings/user");
		let (mime, body) = if auxiliary {
			("application/json", "{}".into())
		} else {
			response(&target, &account, body, &calls)
		};
		let status = if auxiliary { "404 Not Found" } else { "200 OK" };
		let frame = format!(
			"HTTP/1.1 {status}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
			body.len()
		);
		let _ = socket.get_mut().write_all(frame.as_bytes()).await;
	}
}
async fn request(
	socket: &mut tokio::io::BufReader<tokio::net::TcpStream>,
) -> Option<(String, String, Value)> {
	let mut line = String::new();
	if socket.read_line(&mut line).await.expect("request") == 0 {
		return None;
	}
	let target = line.split_whitespace().nth(1).expect("target").to_owned();
	let mut length = 0;
	let mut account = String::new();
	loop {
		line.clear();
		if socket.read_line(&mut line).await.expect("header") == 0 {
			return None;
		}
		if line == "\r\n" {
			break;
		}
		if let Some((name, value)) = line.split_once(':') {
			if name.eq_ignore_ascii_case("content-length") {
				length = value.trim().parse::<usize>().expect("length");
			}
			if name.eq_ignore_ascii_case("authorization") {
				let bearer = value.trim().strip_prefix("Bearer ").expect("bearer");
				let claims = URL_SAFE_NO_PAD
					.decode(bearer.split('.').nth(1).expect("claims"))
					.expect("base64");
				let claims: Value = serde_json::from_slice(&claims).expect("claims JSON");
				account = claims["https://api.openai.com/auth"]["chatgpt_account_id"]
					.as_str()
					.expect("account")
					.into();
			}
		}
	}
	assert!(length <= 2 * 1024 * 1024);
	let mut bytes = vec![0; length];
	socket.read_exact(&mut bytes).await.expect("body");
	let body =
		if bytes.is_empty() { Value::Null } else { serde_json::from_slice(&bytes).expect("JSON") };
	Some((target, account, body))
}
fn response(
	target: &str,
	account: &str,
	body: Value,
	calls: &Mutex<Vec<Value>>,
) -> (&'static str, String) {
	if target.starts_with("/models?") {
		assert!(matches!(account, FIRST | SECOND));
		calls.lock().expect("calls").push(json!({"kind":"catalog","account":account}));
		let mut model = effort::fixture_model("catalog-auth-model", "high");
		model["model_messages"]["instructions_template"] = json!(instructions(account));
		model["context_window"] = json!(context_window(account));
		model["max_context_window"] = json!(context_window(account));
		model["effective_context_window_percent"] = json!(100);
		("application/json", json!({"models":[model]}).to_string())
	} else if target == "/responses" {
		calls.lock().expect("calls").push(
			json!({"kind":"inference","account":account,"instructions":body["instructions"]}),
		);
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
	} else if target.contains("/accounts/check") {
		("application/json",json!({"accounts":[{"id":account,"workspace_backend_origin":"https://chatgpt.com","account_routing_override":"NO_CONSTRAINT"}]}).to_string())
	} else if target.contains("/settings/user") {
		("application/json", json!({"commit_attribution_enabled":false}).to_string())
	} else {
		panic!("unexpected native fixture route: {target}")
	}
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated effective native login policy"]
async fn installed_login_policy_reports_and_enforces_running_restrictions() {
	tokio::time::timeout(Duration::from_secs(45), async {
        let binary = std::env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit binary");
        for method in ["api", "chatgpt"] {
            let home = tempfile::tempdir_in("/tmp").expect("fixture home");
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("listener");
            let address = listener.local_addr().expect("address");
            let calls = Arc::new(Mutex::new(Vec::new()));
            let server = tokio::spawn(serve(listener, calls.clone()));
            let config = home.path().join("config.toml");
            std::fs::write(&config,format!("forced_login_method=\"{method}\"\nchatgpt_base_url=\"http://{address}/backend-api\"\nmodel_provider=\"fixture\"\n[model_providers.fixture]\nname=\"OpenAI\"\nbase_url=\"http://{address}\"\nrequires_openai_auth=true\nsupports_websockets=false\n")).expect("config");
            let mut session = AuthSession::start(&binary,home.path()).await;
            let before = session.client.request("configRequirements/read",json!({})).await.expect("requirements");
            assert_eq!(before["requirements"]["allowedLoginMethods"],json!([method]));
            // Changing disk config cannot rewrite the running authentication manager's policy.
            let changed = std::fs::read_to_string(&config).expect("config").replace(&format!("forced_login_method=\"{method}\""),"");
            std::fs::write(&config,changed).expect("changed config");
            let after = session.client.request("configRequirements/read",json!({})).await.expect("running requirements");
            assert_eq!(after["requirements"]["allowedLoginMethods"],json!([method]));
            let prohibited = if method=="api" {
                json!({"type":"chatgptAuthTokens","accessToken":token(FIRST,1),"chatgptAccountId":FIRST,"chatgptPlanType":"pro"})
            } else { json!({"type":"apiKey","apiKey":"synthetic-prohibited-key"}) };
            assert!(matches!(session.client.request("account/login/start",prohibited).await,Err(decodex_codex::app_server_client::ClientError::Remote(_))));
            assert!(calls.lock().expect("calls").is_empty(),"prohibited login must not fetch models or start inference");
            session.child.kill().await.expect("stop fixture");
            session.child.wait().await.expect("reap fixture");
            assert!(!server.is_finished());
            server.abort();
        }
    }).await.expect("bounded login policy fixture");
}
