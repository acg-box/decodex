//! Installed-native client-message identity, durable receipt and cold recovery.
use super::*;
use crate::agent::{AgentConfig, AgentCoordinator};
use decodex_database::SqliteStore;
use std::sync::atomic::AtomicUsize;
use tokio::io::{AsyncBufReadExt as _, AsyncReadExt as _, AsyncWriteExt as _};

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native steering receipt qualification"]
async fn installed_native_steer_receipt_confirms_live_and_cold_without_replay() {
	for cold in [false, true] {
		tokio::time::timeout(Duration::from_secs(50), qualify(cold))
			.await
			.expect("bounded native steering qualification");
	}
}

async fn qualify(cold: bool) {
	let binary = std::env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");
	assert!(std::path::Path::new(&binary).is_absolute());
	let home = tempfile::tempdir_in("/tmp").expect("native steering fixture operation");
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
		.await
		.expect("native steering fixture operation");
	let address = listener.local_addr().expect("native steering fixture operation");
	let seen = Arc::new(tokio::sync::Notify::new());
	let release = Arc::new(tokio::sync::Notify::new());
	let count = Arc::new(AtomicUsize::new(0));
	let backend = tokio::spawn(serve(listener, seen.clone(), release.clone(), count.clone()));
	std::fs::write(home.path().join("config.toml"), format!("model = \"gpt-5.6-sol\"\nmodel_provider = \"fixture\"\n[model_providers.fixture]\nname = \"Steering receipt fixture\"\nbase_url = \"http://{address}\"\nwire_api = \"responses\"\nrequires_openai_auth = false\nsupports_websockets = false\n")).expect("native steering fixture operation");
	let root = decodex_core::DecodexRoot::new(
		home.path().canonicalize().expect("native steering fixture operation").join("product"),
	)
	.expect("native steering fixture operation");
	root.paths().ensure_layout().expect("native steering fixture operation");
	let store = SqliteStore::open(&root.paths()).expect("native steering fixture operation");
	let config =
		AgentConfig::new("gpt-5.6-sol".into(), "medium".into(), home.path().display().to_string());
	let mut session = NativeSession::start(&binary, home.path());
	let mut agent = AgentCoordinator::new(store.clone(), session.client.clone(), config.clone())
		.expect("native steering fixture operation");
	let work = agent
		.start_agent("agent", "Identical fixture input")
		.await
		.expect("native steering fixture operation");
	let thread_id = work.codex_thread_id.expect("native steering fixture operation");
	let turn = work.active_turn_id.expect("native steering fixture operation");
	seen.notified().await;
	let key = "native-steer-fixture";
	let pending = store
		.begin_agent_steer(
			"agent".into(),
			turn.clone(),
			key.into(),
			json!({"text":"Identical fixture input","source":"user"}).to_string(),
		)
		.await
		.expect("native steering fixture operation");
	// Deliberately do not settle the durable attempt from the RPC reply. Only a
	// real native receipt may resolve this saved uncertain state.
	let reply = session.client.turn_steer(json!({"threadId":thread_id,"expectedTurnId":turn,"clientUserMessageId":key,"input":[{"type":"text","text":"Identical fixture input","text_elements":[]}]})).await.expect("native steering fixture operation");
	assert_eq!(reply["turnId"], turn);
	assert!(
		!store
			.agent_steer_confirmed("agent".into(), thread_id.clone(), turn.clone(), key.into())
			.await
			.expect("native steering fixture operation")
	);
	release.notify_one();
	consume_receipt(&mut agent, &mut session.events, &thread_id, &turn, key, cold).await;
	if cold {
		store
			.complete_agent_turn("agent".into(), turn.clone())
			.await
			.expect("terminal state can precede receipt recovery");
	}
	let calls = count.load(Ordering::Acquire);
	assert_eq!(calls, 2, "one original sample and one steer sample");
	drop(agent);
	drop(session);
	drop(store);
	let store = SqliteStore::open(&root.paths()).expect("native steering fixture operation");
	let session = NativeSession::start(&binary, home.path());
	let mut agent = AgentCoordinator::new(store.clone(), session.client.clone(), config)
		.expect("native steering fixture operation");
	agent.recover_persisted().await.expect("native steering fixture operation");
	assert!(
		store
			.agent_steer_confirmed("agent".into(), thread_id.clone(), turn.clone(), key.into())
			.await
			.expect("native steering fixture operation")
	);
	assert!(
		!store
			.agent_steer_confirmed(
				"agent".into(),
				thread_id.clone(),
				turn.clone(),
				"another-submission".into()
			)
			.await
			.expect("native steering fixture operation")
	);
	assert!(
		store
			.get_agent_inbox_event(pending)
			.await
			.expect("native steering fixture operation")
			.disposition
			.is_some()
	);
	let history = session
		.client
		.thread_read_turn(&thread_id, &turn)
		.await
		.expect("native steering fixture operation");
	let items = history["thread"]["turns"]
		.as_array()
		.expect("native steering fixture operation")
		.iter()
		.find(|item| item["id"] == turn)
		.expect("native steering fixture operation")["items"]
		.as_array()
		.expect("native steering fixture operation");
	assert_eq!(items.iter().filter(|item| item["type"] == "userMessage").count(), 2);
	assert_eq!(items.iter().filter(|item| item["clientId"] == key).count(), 1);
	qualify_query(&root, store.clone(), &thread_id, &turn, key).await;
	assert_eq!(count.load(Ordering::Acquire), calls, "receipt recovery cannot replay input");
	drop(agent);
	drop(session);
	backend.abort();
}

async fn consume_receipt(
	agent: &mut AgentCoordinator,
	events: &mut mpsc::Receiver<ServerEvent>,
	thread_id: &str,
	turn: &str,
	key: &str,
	cold: bool,
) {
	let mut matching = 0;
	loop {
		let event = events.recv().await.expect("native event");
		let terminal = matches!(&event, ServerEvent::Notification { method, params } if method == "turn/completed" && params["turn"]["id"] == turn);
		if let ServerEvent::Notification { method, params } = &event
			&& method == "item/completed"
			&& params["item"]["type"] == "userMessage"
			&& params["item"]["clientId"] == key
		{
			assert_eq!(params["threadId"], thread_id);
			assert_eq!(params["turnId"], turn);
			matching += 1;
		}
		if !cold {
			agent.handle_event(event).await.expect("native steering fixture operation");
		}
		if terminal {
			break;
		}
	}
	assert_eq!(matching, 1);
}

async fn serve(
	listener: tokio::net::TcpListener,
	seen: Arc<tokio::sync::Notify>,
	release: Arc<tokio::sync::Notify>,
	count: Arc<AtomicUsize>,
) {
	while let Ok((socket, _)) = listener.accept().await {
		let mut socket = tokio::io::BufReader::new(socket);
		let mut length = 0;
		loop {
			let mut line = String::new();
			assert!(
				socket.read_line(&mut line).await.expect("native steering fixture operation") > 0
			);
			if line == "\r\n" {
				break;
			}
			if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
				length = value.trim().parse::<usize>().expect("native steering fixture operation");
			}
		}
		assert!((1..=2 * 1024 * 1024).contains(&length));
		let mut body = vec![0; length];
		socket.read_exact(&mut body).await.expect("native steering fixture operation");
		let serial = count.fetch_add(1, Ordering::AcqRel);
		if serial == 0 {
			seen.notify_one();
			release.notified().await;
		}
		let frames = [
			json!({"type":"response.created","response":{"id":format!("response-{serial}")}}),
			json!({"type":"response.output_item.done","item":{"type":"message","role":"assistant","id":format!("answer-{serial}"),"content":[{"type":"output_text","text":"Fixture complete."}]}}),
			json!({"type":"response.completed","response":{"id":format!("response-{serial}"),"usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}}}),
		];
		let data = frames
			.iter()
			.map(|value| {
				format!(
					"event: {}\ndata: {value}\n\n",
					value["type"].as_str().expect("native steering fixture operation")
				)
			})
			.collect::<String>();
		socket.get_mut().write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{data}",data.len()).as_bytes()).await.expect("native steering fixture operation");
	}
}

async fn qualify_query(
	root: &decodex_core::DecodexRoot,
	store: SqliteStore,
	thread: &str,
	turn: &str,
	key: &str,
) {
	use crate::{
		ProtocolServer, ServerConfig,
		application::{ProductStore, ServiceApplication},
	};
	use decodex_protocol::{
		AgentClient, AgentSteerIdentity, AgentSteerReceiptResult, CURRENT_VERSION, ClientProfile,
		DoctorCheck, DoctorComponent, DoctorIssue, DoctorReport, DoctorStatus, EntityId,
		IdempotencyKey, LocalTransportAuthority, ServerId, WireText,
	};
	use std::os::unix::fs::PermissionsExt as _;
	let server_id =
		ServerId::new("20000000-0000-4000-8000-000000000001").expect("fixture server identity");
	let doctor = DoctorReport::new(
		server_id.clone(),
		CURRENT_VERSION,
		DoctorComponent::ALL
			.into_iter()
			.map(|component| {
				DoctorCheck::new(component, DoctorStatus::Unavailable(DoctorIssue::NotProbed))
			})
			.collect(),
	)
	.expect("fixture doctor");
	let app = ServiceApplication::new(
		ProductStore::Available(store),
		None,
		None,
		None,
		crate::conversation::ConversationCapability::Unavailable(
			decodex_protocol::ConversationUnavailableReason::AppServerProfile,
		),
		doctor,
	);
	let uid = unsafe { libc::geteuid() };
	let authority = LocalTransportAuthority::new(
		root.paths(),
		decodex_core::LocalTrustPolicy::SameUid,
		Some(uid),
	)
	.expect("fixture transport authority");
	let mut server = ProtocolServer::new(server_id, app, ServerConfig::default())
		.bind(authority)
		.await
		.expect("real service query transport");
	let config = root.as_path().join("config.toml");
	std::fs::write(&config,format!("version = 1\nactive_profile = \"local\"\ncache = {{}}\n[profiles.local]\nkind = \"local\"\npolicy = \"same_uid\"\nservice_owner_uid = {uid}\nexpected_server_identity = \"20000000-0000-4000-8000-000000000001\"\n")).expect("fixture client configuration");
	std::fs::set_permissions(config, std::fs::Permissions::from_mode(0o600))
		.expect("private fixture configuration");
	let profile = ClientProfile::load(root.as_path(), None).expect("fixture client profile");
	let client = AgentClient::new(profile);
	let identity = AgentSteerIdentity {
		work_id: EntityId::new("agent").expect("work"),
		thread_id: WireText::new(thread).expect("thread"),
		turn_id: WireText::new(turn).expect("turn"),
		submission_id: IdempotencyKey::new(key).expect("submission"),
	};
	assert_eq!(
		client.steer_receipt(identity.clone()).await.expect("positive query"),
		AgentSteerReceiptResult::Confirmed { identity: identity.clone() }
	);
	let mut unrelated = identity.clone();
	unrelated.submission_id = IdempotencyKey::new("unrelated").expect("other submission");
	assert_eq!(
		client.steer_receipt(unrelated).await.expect("unconfirmed query"),
		AgentSteerReceiptResult::Unconfirmed
	);
	if let Some(binary) = std::env::var_os("DECODEX_TEST_STEER_VISUAL_BINARY") {
		let output = std::env::var_os("DECODEX_TEST_STEER_VISUAL_OUTPUT")
			.expect("explicit visual output directory");
		let output = std::path::PathBuf::from(output).join(format!("{thread}.png"));
		let result = tokio::process::Command::new(binary)
			.kill_on_drop(true)
			.env("DECODEX_VISUAL_AGENT_ROOT", root.as_path())
			.env("DECODEX_VISUAL_AGENT_WORK", "agent")
			.env(
				"DECODEX_VISUAL_AGENT_STEER_RECEIPT",
				serde_json::to_string(&identity).expect("receipt identity"),
			)
			.env("DECODEX_VISUAL_OUTPUT", &output)
			.output()
			.await
			.expect("rendered receipt proof");
		assert!(
			result.status.success(),
			"visual proof failed: {}",
			String::from_utf8_lossy(&result.stderr)
		);
		let evidence: Value = serde_json::from_slice(
			&std::fs::read(output.with_extension("steer.json")).expect("visual receipt evidence"),
		)
		.expect("visual evidence JSON");
		assert_eq!(evidence["before"]["uncertain"], true);
		assert_eq!(evidence["after"]["uncertain"], false);
		assert_eq!(evidence["after"]["draft"], "Later draft retained.");
	}
	server.shutdown().await.expect("query server shutdown");
}
