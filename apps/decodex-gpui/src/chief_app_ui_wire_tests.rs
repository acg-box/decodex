//! Collect complete App UI documents through the real local wire boundary.
use super::*;
use decodex_protocol::{
	CURRENT_VERSION, ClientMessage, Cursor, QueryPayload, QueryResultEnvelope, QueryResultPayload,
	ReconnectMode, ServerId, ServerMessage, ServerWelcome, SnapshotEnvelope,
};
use futures_util::{SinkExt, StreamExt};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use tokio_tungstenite::tungstenite::Message;

const SERVER: &str = "018f0f9e-7b6e-4a31-8f4c-1d2e3f405162";
const DOCUMENT: &[u8] =
	br#"{"item":{"id":"image"},"resources":[{"text":"<button>Fixture</button>"}]}"#;

fn fixture(
	mode: &'static str,
) -> (tempfile::TempDir, ClientProfile, std::thread::JoinHandle<Vec<ChiefAppUiRequest>>) {
	let root = tempfile::tempdir_in("/tmp").unwrap();
	let path = root.path().canonicalize().unwrap();
	let server = path.join("server");
	std::fs::create_dir(&server).unwrap();
	std::fs::set_permissions(&server, std::fs::Permissions::from_mode(0o700)).unwrap();
	let uid = std::fs::metadata(&path).unwrap().uid();
	let config = path.join("config.toml");
	std::fs::write(&config,format!("version = 1\nactive_profile = \"local\"\ncache = {{}}\n[profiles.local]\nkind = \"local\"\npolicy = \"same_uid\"\nservice_owner_uid = {uid}\nexpected_server_identity = \"{SERVER}\"\n")).unwrap();
	std::fs::set_permissions(config, std::fs::Permissions::from_mode(0o600)).unwrap();
	let socket_path = server.join("decodex.sock");
	let listener = std::os::unix::net::UnixListener::bind(&socket_path).unwrap();
	std::fs::set_permissions(socket_path, std::fs::Permissions::from_mode(0o600)).unwrap();
	listener.set_nonblocking(true).unwrap();
	let profile = ClientProfile::load(&path, None).unwrap();
	let thread = std::thread::spawn(move || {
		let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
		runtime.block_on(async {
			let listener = tokio::net::UnixListener::from_std(listener).unwrap();
			tokio::time::timeout(std::time::Duration::from_secs(5), serve(listener, mode))
				.await
				.unwrap()
		})
	});
	(root, profile, thread)
}

async fn serve(listener: tokio::net::UnixListener, mode: &str) -> Vec<ChiefAppUiRequest> {
	let mut requests = Vec::new();
	while requests.len() < if mode == "complete" { 2 } else { 1 } {
		let index = requests.len();
		let mut socket =
			tokio_tungstenite::accept_async(listener.accept().await.unwrap().0).await.unwrap();
		let _hello = socket.next().await.unwrap().unwrap();
		for message in [
			ServerMessage::Welcome(ServerWelcome {
				version: CURRENT_VERSION,
				server_id: ServerId::new(SERVER).unwrap(),
				instance_id: None,
				cursor: Cursor(0),
				reconnect: ReconnectMode::Snapshot,
			}),
			ServerMessage::Snapshot(SnapshotEnvelope {
				version: CURRENT_VERSION,
				server_id: ServerId::new(SERVER).unwrap(),
				cursor: Cursor(0),
				items: vec![],
			}),
		] {
			socket
				.send(Message::Text(serde_json::to_string(&message).unwrap().into()))
				.await
				.unwrap();
		}
		let Message::Text(text) = socket.next().await.unwrap().unwrap() else {
			panic!("text query")
		};
		let ClientMessage::Query(query) = serde_json::from_str::<ClientMessage>(&text).unwrap()
		else {
			panic!("query")
		};
		if matches!(
			query.payload,
			QueryPayload::WaitForChiefOutput { .. } | QueryPayload::GetNativeAgents { .. }
		) {
			// The workspace also opens an independent live-output and agent observations.
			socket.close(None).await.unwrap();
			continue;
		}
		let QueryPayload::GetChiefAppUi { request } = query.payload else {
			panic!("media query: {:?}", query.payload)
		};
		assert_eq!(request.thread_id.as_str(), "native-thread");
		assert_eq!(request.turn_id.as_str(), "turn");
		assert_eq!(request.item_id.as_str(), "image");
		let split = DOCUMENT.len() / 2;
		assert_eq!(request.offset as usize, if index == 0 { 0 } else { split });
		assert_eq!(
			request.fingerprint,
			if index == 0 { None } else { Some(EntityId::new("a".repeat(64)).unwrap()) }
		);
		requests.push(request.clone());
		let result = if mode == "unavailable" {
			ChiefAppUiResult::Unavailable
		} else {
			ChiefAppUiResult::Available {
				request: Box::new(request),
				account_id: EntityId::new(if mode == "account" {
					"other-account"
				} else {
					"account"
				})
				.unwrap(),
				fingerprint: EntityId::new("a".repeat(64)).unwrap(),
				total_bytes: DOCUMENT.len() as u32,
				bytes: if index == 0 {
					DOCUMENT[..split].to_vec()
				} else {
					DOCUMENT[split..].to_vec()
				},
			}
		};
		let result = ServerMessage::QueryResult(QueryResultEnvelope {
			version: CURRENT_VERSION,
			server_id: ServerId::new(SERVER).unwrap(),
			query_id: query.query_id,
			payload: QueryResultPayload::ChiefAppUi(result),
		});
		socket.send(Message::Text(serde_json::to_string(&result).unwrap().into())).await.unwrap();
	}
	requests
}

#[tokio::test]
async fn app_document_collection_requires_one_account_and_complete_chunks() {
	for mode in ["complete", "account", "unavailable"] {
		let (_root, profile, server) = fixture(mode);
		let request = ChiefAppUiRequest {
			work_id: EntityId::new("work").unwrap(),
			thread_id: EntityId::new("native-thread").unwrap(),
			turn_id: EntityId::new("turn").unwrap(),
			item_id: EntityId::new("image").unwrap(),
			offset: 0,
			fingerprint: None,
		};
		let result = load(&ChiefClient::new(profile), request, "account").await;
		let requests = server.join().unwrap();
		if mode == "complete" {
			assert_eq!(result.unwrap()["resources"][0]["text"], "<button>Fixture</button>");
			assert_eq!(requests.len(), 2);
		} else {
			assert!(result.is_err());
			assert_eq!(requests.len(), 1);
		}
	}
}
