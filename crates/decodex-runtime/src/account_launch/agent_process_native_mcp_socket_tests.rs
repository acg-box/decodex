//! Qualify sharing through separate native socket clients without installing a daemon.

use std::{
	env, fs,
	path::Path,
	process::Stdio,
	sync::{Arc, atomic::AtomicUsize},
	time::Duration,
};

use futures_util::{SinkExt as _, StreamExt as _};
use serde_json::{Value, json};
use tokio::{
	io::{self, Join},
	net::TcpListener,
	process::{Child, ChildStdin, ChildStdout, Command},
	sync::Barrier,
	time,
};
use tokio_tungstenite::{WebSocketStream, client_async, tungstenite::Message};

use crate::account_launch::agent_process::native_tests;

type ProxySocket = WebSocketStream<Join<ChildStdout, ChildStdin>>;

struct SocketClient {
	process: Child,
	socket: ProxySocket,
	next_id: u64,
}

impl SocketClient {
	async fn connect(binary: &Path, home: &Path, socket: &Path) -> Self {
		let mut process = Command::new(binary)
			.args(["app-server", "proxy", "--sock"])
			.arg(socket)
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
			.expect("start native socket proxy");
		let stdout = process.stdout.take().expect("proxy stdout");
		let stdin = process.stdin.take().expect("proxy stdin");
		let (socket, _) = client_async("ws://localhost/", io::join(stdout, stdin))
			.await
			.expect("upgrade native socket proxy");
		let mut client = Self { process, socket, next_id: 1 };
		client
			.request(
				"initialize",
				json!({"clientInfo":{"name":"decodex-socket-fixture","version":"1"},"capabilities":{"experimentalApi":true}}),
			)
			.await;
		client
			.socket
			.send(Message::Text(json!({"method":"initialized"}).to_string().into()))
			.await
			.expect("complete native client initialization");
		client
	}

	async fn request(&mut self, method: &str, params: Value) -> Value {
		let id = self.next_id;
		self.next_id += 1;
		self.socket
			.send(Message::Text(
				json!({"id":id,"method":method,"params":params}).to_string().into(),
			))
			.await
			.expect("send native socket request");
		loop {
			let message = self
				.socket
				.next()
				.await
				.expect("native socket response")
				.expect("read native socket response");
			let Message::Text(text) = message else { continue };
			let response: Value = serde_json::from_str(&text).expect("native socket JSON");
			if response["id"] != id {
				continue;
			}
			assert!(response.get("error").is_none(), "{method}: {}", response["error"]);
			return response["result"].clone();
		}
	}

	async fn start_thread(&mut self, home: &Path, approval: &str) -> String {
		self.request(
			"thread/start",
			json!({"cwd":home,"approvalPolicy":approval,"sandbox":"read-only"}),
		)
		.await["thread"]["id"]
			.as_str()
			.expect("native socket thread ID")
			.to_owned()
	}

	async fn helper(&mut self, thread: &str, server: &str) -> u64 {
		self.request(
			"mcpServer/tool/call",
			json!({"threadId":thread,"server":server,"tool":"identify","arguments":{},"meta":null}),
		)
		.await["structuredContent"]["pid"]
			.as_u64()
			.expect("MCP helper identity")
	}

	async fn input_helper(&mut self, thread: &str, label: &str, barrier: Arc<Barrier>) -> u64 {
		let id = self.next_id;
		self.next_id += 1;
		self.socket
			.send(Message::Text(
				json!({"id":id,"method":"mcpServer/tool/call","params":{"threadId":thread,"server":"modern","tool":"ask","arguments":{"label":label},"meta":null}}).to_string().into(),
			))
			.await
			.expect("send native input tool call");
		let mut prompts = 0;
		loop {
			let message = self
				.socket
				.next()
				.await
				.expect("native input response")
				.expect("read native input response");
			let Message::Text(text) = message else { continue };
			let response: Value = serde_json::from_str(&text).expect("native input JSON");
			if response["method"] == "mcpServer/elicitation/request" {
				assert_eq!(
					response["params"]["threadId"], thread,
					"input belongs to its calling thread"
				);
				prompts += 1;
				assert_eq!(prompts, 1, "each call receives one input request");
				time::timeout(Duration::from_secs(10), barrier.wait())
					.await
					.expect("both shared calls request input before either reply");
				self.socket
					.send(Message::Text(json!({"id":response["id"],"result":{"action":"accept","content":{"answer":label}}}).to_string().into()))
					.await
					.expect("answer the exact native input request");
				continue;
			}
			if response["id"] != id {
				continue;
			}
			assert!(
				response.get("error").is_none(),
				"native input tool call: {}",
				response["error"]
			);
			assert_eq!(prompts, 1);
			let content = &response["result"]["structuredContent"];
			assert_eq!(content["label"], label);
			assert_eq!(content["answer"], label);
			return content["pid"].as_u64().expect("shared input helper identity");
		}
	}

	async fn close(mut self) {
		self.socket.close(None).await.expect("close native socket");
		let (mut stdout, stdin) = self.socket.into_inner().into_inner();
		drop(stdin);
		time::timeout(Duration::from_secs(10), io::copy(&mut stdout, &mut io::sink()))
			.await
			.expect("native proxy output deadline")
			.expect("drain native proxy output");
		let status = time::timeout(Duration::from_secs(10), self.process.wait())
			.await
			.expect("native proxy exit deadline")
			.expect("native proxy exit");
		assert!(status.success(), "native proxy did not exit cleanly");
	}
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native socket clients"]
async fn separate_clients_share_a_compatible_native_mcp_helper() {
	qualify_socket_clients(false, false).await;
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; run before enabling native MCP sharing"]
async fn separate_clients_keep_unmarked_legacy_helpers_private() {
	qualify_socket_clients(true, false).await;
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated shared MCP input"]
async fn shared_modern_input_returns_to_each_calling_client() {
	qualify_socket_clients(false, true).await;
}

async fn qualify_socket_clients(mixed: bool, input: bool) {
	let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");
	let binary = Path::new(&binary);
	assert!(binary.is_absolute());
	let root = tempfile::tempdir_in("/tmp").expect("temporary native socket home");
	let home = root.path();
	let fixture = home.join("mcp.py");
	let fixture_source = if input {
		include_str!("native_mcp_shared_input.py")
	} else {
		include_str!("native_mcp_sharing.py")
	};
	fs::write(&fixture, fixture_source).expect("write MCP fixture");
	let fixture = serde_json::to_string(&fixture).expect("serialize MCP fixture path");
	let listener = TcpListener::bind("127.0.0.1:0").await.expect("fixture listener");
	let address = listener.local_addr().expect("fixture address");
	let backend = tokio::spawn(native_tests::serve_fixture(
		listener,
		Arc::new(AtomicUsize::new(0)),
		None,
		None,
		None,
		|serial| json!({"type":"message","role":"assistant","id":format!("answer-{serial}"),"content":[{"type":"output_text","text":"Done"}]}),
	));
	let mut config = format!(
		"thread_unload_delay_secs=0\nmodel=\"gpt-5.6-sol\"\nmodel_provider=\"fixture\"\ncli_auth_credentials_store=\"file\"\n\
		[model_providers.fixture]\nname=\"fixture\"\nbase_url=\"http://{address}\"\nwire_api=\"responses\"\nrequires_openai_auth=false\nsupports_websockets=false\n\
		[features]\ncode_mode=false\nmcp_connection_sharing=true\n\
		[mcp_servers.modern]\ncommand=\"/usr/bin/python3\"\nargs=[{fixture},\"modern\"]\nrequired=true\n"
	);
	if mixed {
		config.push_str(&format!(
			"[mcp_servers.legacy]\ncommand=\"/usr/bin/python3\"\nargs=[{fixture},\"legacy-empty\"]\nrequired=true\n"
		));
	}
	fs::write(home.join("config.toml"), config).expect("write native socket config");
	let socket = home.join("native.sock");
	let mut server = Command::new(binary)
		.arg("app-server")
		.arg("--listen")
		.arg(format!("unix://{}", socket.display()))
		.env_clear()
		.env("HOME", home)
		.env("CODEX_HOME", home)
		.env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
		.current_dir(home)
		.stdin(Stdio::null())
		.stdout(Stdio::null())
		.stderr(Stdio::null())
		.kill_on_drop(true)
		.spawn()
		.expect("start native socket server");

	time::timeout(Duration::from_secs(90), async {
		while !socket.exists() {
			assert!(
				server.try_wait().expect("native socket server status").is_none(),
				"native socket server exited"
			);
			time::sleep(Duration::from_millis(10)).await;
		}
		exercise_socket_clients(binary, home, &socket, mixed, input).await;
	})
	.await
	.expect("bounded native socket qualification");
	server.kill().await.expect("stop native socket server");
	backend.abort();
}

async fn exercise_socket_clients(
	binary: &Path,
	home: &Path,
	socket: &Path,
	mixed: bool,
	input: bool,
) {
	let mut first = SocketClient::connect(binary, home, socket).await;
	let mut second = SocketClient::connect(binary, home, socket).await;
	assert_ne!(first.process.id(), second.process.id(), "clients use distinct processes");
	let approval = if input { "on-request" } else { "never" };
	let first_thread = first.start_thread(home, approval).await;
	let second_thread = second.start_thread(home, approval).await;
	let modern = if input {
		let barrier = Arc::new(Barrier::new(2));
		let (first_helper, second_helper) = tokio::join!(
			first.input_helper(&first_thread, "first", Arc::clone(&barrier)),
			second.input_helper(&second_thread, "second", barrier),
		);
		assert_eq!(first_helper, second_helper, "input calls use one shared helper");
		first_helper
	} else {
		let first_helper = first.helper(&first_thread, "modern").await;
		assert_eq!(second.helper(&second_thread, "modern").await, first_helper);
		first_helper
	};
	if mixed {
		assert_ne!(
			first.helper(&first_thread, "legacy").await,
			second.helper(&second_thread, "legacy").await
		);
	}
	first.request("thread/unsubscribe", json!({"threadId":first_thread})).await;
	time::timeout(Duration::from_secs(20), first.close())
		.await
		.expect("first native client exit deadline");
	assert_eq!(second.helper(&second_thread, "modern").await, modern);
	second.request("thread/unsubscribe", json!({"threadId":second_thread})).await;
	time::timeout(Duration::from_secs(20), second.close())
		.await
		.expect("second native client exit deadline");
}
