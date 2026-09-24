//! Installed external-auth retries with synthetic unsigned JWTs and loopback HTTP only.
use super::*;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use decodex_codex::app_server_client::{AppServerClient, RequestId, ServerEvent};
use tokio::io::{AsyncBufReadExt as _, AsyncReadExt as _, AsyncWriteExt as _};

fn token(serial: u8) -> String {
	let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none","typ":"JWT"}"#);
	let claims = json!({"email":"fixture@example.invalid","serial":serial,"https://api.openai.com/auth":{"chatgpt_user_id":"fixture-user","chatgpt_account_id":PROVIDER,"chatgpt_plan_type":"pro"}});
	format!(
		"{header}.{}.c2lnbmF0dXJl",
		URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).expect("fixture claims"))
	)
}
async fn start(
	binary: &std::ffi::OsStr,
	home: &Path,
) -> (AppServerClient, tokio::sync::mpsc::Receiver<ServerEvent>, tokio::process::Child) {
	let mut child = tokio::process::Command::new(binary)
		.arg("app-server")
		.env_clear()
		.env("HOME", home)
		.env("CODEX_HOME", home)
		.env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
		.current_dir(home)
		.stdin(std::process::Stdio::piped())
		.stdout(std::process::Stdio::piped())
		.stderr(std::process::Stdio::null())
		.kill_on_drop(true)
		.spawn()
		.expect("native fixture");
	let (client, events) = AppServerClient::from_io(
		child.stdout.take().expect("stdout"),
		child.stdin.take().expect("stdin"),
	);
	client.initialize(json!({"clientInfo":{"name":"decodex_refresh_fixture","version":"0.1"},"capabilities":{"experimentalApi":true}})).await.expect("initialize");
	(client, events, child)
}
async fn login(client: &AppServerClient, access: &str) {
	client.request("account/login/start",json!({"type":"chatgptAuthTokens","accessToken":access,"chatgptAccountId":PROVIDER,"chatgptPlanType":"pro"})).await.unwrap_or_else(|error| {if let decodex_codex::app_server_client::ClientError::Remote(remote)=error {panic!("external fixture login: {}",remote.message.replace(access,"[synthetic-token]"));}panic!("external fixture transport");});
}
async fn turn(
	client: &AppServerClient,
	events: &mut tokio::sync::mpsc::Receiver<ServerEvent>,
	thread: &str,
	binding: &AccountBinding,
	completed: bool,
) {
	let copy = client.clone();
	let target = thread.to_owned();
	let dispatch = tokio::spawn(async move {
		copy.turn_start(
			json!({"threadId":target,"input":[{"type":"text","text":"Return fixture result"}]}),
		)
		.await
	});
	loop {
		match events.recv().await.expect("native event") {
			ServerEvent::Request { id, method, params } => {
				assert_eq!(method, "account/chatgptAuthTokens/refresh");
				let RequestId::Number(number) = id else { panic!("native callback numeric id") };
				let request = json!({"id":number,"method":method,"params":params});
				let response =
					handle(binding, u64::try_from(number).expect("callback id"), &method, &request)
						.expect("production callback");
				let response: Value = serde_json::from_slice(&response).expect("callback frame");
				if let Some(result) = response.get("result") {
					client
						.respond(RequestId::Number(number), result.clone())
						.await
						.expect("callback response");
				} else {
					client
						.respond_error(
							RequestId::Number(number),
							serde_json::from_value(response["error"].clone())
								.expect("callback error"),
						)
						.await
						.expect("callback refusal");
				}
			},
			ServerEvent::Notification { method, params }
				if method == "turn/completed" && params["threadId"] == thread =>
			{
				assert_eq!(
					params["turn"]["status"],
					if completed { "completed" } else { "failed" }
				);
				break;
			},
			_ => {},
		}
	}
	dispatch.await.expect("dispatch task").expect("turn accepted");
}
#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; synthetic native401 refresh qualification"]
async fn installed_native_401_uses_production_refresh_reply_and_reauthenticates_after_restart() {
	tokio::time::timeout(Duration::from_secs(45), qualify(false))
		.await
		.expect("bounded native refresh");
}
#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; synthetic native refresh refusal"]
async fn installed_native_401_refresh_refusal_fails_without_replaying_inference() {
	tokio::time::timeout(Duration::from_secs(45), qualify(true))
		.await
		.expect("bounded refused fixture");
}

async fn qualify(fail: bool) {
	let binary = std::env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit binary");
	assert!(Path::new(&binary).is_absolute());
	let home = tempfile::tempdir().expect("fixture home");
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("listener");
	let address = listener.local_addr().expect("address");
	let initial = token(1);
	let refreshed = token(2);
	let seen = Arc::new(Mutex::new(Vec::new()));
	let server = tokio::spawn(serve(listener, initial.clone(), refreshed.clone(), seen.clone()));
	std::fs::write(home.path().join("config.toml"),format!("model=\"gpt-5.6-sol\"\nmodel_provider=\"routing\"\ncli_auth_credentials_store=\"file\"\nchatgpt_base_url=\"http://{address}/backend-api\"\n[model_providers.routing]\nname=\"OpenAI\"\nbase_url=\"http://{address}/v1\"\nrequires_openai_auth=true\nsupports_websockets=false\nrequest_max_retries=0\nstream_max_retries=0\n")).expect("fixture config");
	let (binding, calls) = binding(&refreshed, PROVIDER, fail);
	let (client, mut events, mut child) = start(&binary, home.path()).await;
	login(&client, &initial).await;
	let started = client
		.thread_start(json!({"cwd":home.path(),"approvalPolicy":"never","sandbox":"read-only"}))
		.await
		.expect("start thread");
	let thread = started["thread"]["id"].as_str().expect("thread").to_owned();
	turn(&client, &mut events, &thread, &binding, !fail).await;
	assert_eq!(calls.load(Ordering::SeqCst), 1);
	if fail {
		assert_eq!(*seen.lock().expect("observations"), vec![false]);
		child.kill().await.expect("stop refused fixture");
		child.wait().await.expect("reap refused fixture");
		server.abort();
		return;
	}
	assert_eq!(*seen.lock().expect("observations"), vec![false, true]);
	assert!(!home.path().join("auth.json").exists(), "external credentials must stay in memory");
	child.kill().await.expect("stop native");
	child.wait().await.expect("reap native");
	let (client, mut events, mut child) = start(&binary, home.path()).await;
	login(&client, &refreshed).await;
	client.thread_resume(json!({"threadId":thread})).await.expect("resume exact thread");
	turn(&client, &mut events, &thread, &binding, !fail).await;
	assert_eq!(calls.load(Ordering::SeqCst), 1, "valid successor needs no further callback");
	assert_eq!(*seen.lock().expect("observations"), vec![false, true, true]);
	child.kill().await.expect("stop native");
	child.wait().await.expect("reap native");
	server.abort();
}
async fn serve(
	listener: tokio::net::TcpListener,
	initial: String,
	refreshed: String,
	seen: Arc<Mutex<Vec<bool>>>,
) {
	'connections: while let Ok((socket, _)) = listener.accept().await {
		let mut socket = tokio::io::BufReader::new(socket);
		let mut first = String::new();
		if socket.read_line(&mut first).await.expect("request line") == 0 {
			continue;
		}
		let mut length = 0;
		let mut authorization = String::new();
		loop {
			let mut line = String::new();
			if socket.read_line(&mut line).await.expect("header") == 0 {
				continue 'connections;
			}
			if line == "\r\n" {
				break;
			}
			if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
				length = v.trim().parse::<usize>().expect("length");
			}
			if line.to_ascii_lowercase().starts_with("authorization:") {
				authorization = line.split_once(':').expect("header separator").1.trim().to_owned();
			}
		}
		assert!(length <= 2 * 1024 * 1024);
		let mut body = vec![0; length];
		socket.read_exact(&mut body).await.expect("body");
		let (status, mime, body) = if first.starts_with("POST /v1/responses ") {
			let fresh = authorization == format!("Bearer {refreshed}");
			assert!(fresh || authorization == format!("Bearer {initial}"));
			seen.lock().expect("observations").push(fresh);
			if !fresh {
				(401, "application/json", json!({"error":{"message":"unauthorized"}}).to_string())
			} else {
				let frames = [
					json!({"type":"response.created","response":{"id":"refresh-fixture"}}),
					json!({"type":"response.output_item.done","item":{"type":"message","role":"assistant","id":"fixture-message","content":[{"type":"output_text","text":"Fixture success"}]}}),
					json!({"type":"response.completed","response":{"id":"refresh-fixture","usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}}}),
				];
				(
					200,
					"text/event-stream",
					frames
						.iter()
						.map(|v| {
							format!(
								"event: {}\ndata: {v}\n\n",
								v["type"].as_str().expect("event type")
							)
						})
						.collect(),
				)
			}
		} else if first.contains("/accounts/check") {
			(200,"application/json",json!({"accounts":[{"id":PROVIDER,"workspace_backend_origin":"https://chatgpt.com","account_routing_override":"NO_CONSTRAINT"}]}).to_string())
		} else if first.contains("/settings/user") {
			(200, "application/json", json!({"commit_attribution_enabled":false}).to_string())
		} else {
			(404, "application/json", "{}".into())
		};
		socket.write_all(format!("HTTP/1.1 {status} Fixture\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.expect("HTTP response");
	}
}
