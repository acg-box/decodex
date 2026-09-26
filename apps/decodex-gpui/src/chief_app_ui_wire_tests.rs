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

pub(super) fn fixture(
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
	let mut receipt_index = 0;
	while requests.len() < if mode == "complete" { 2 } else { 1 } {
		let index = if mode.starts_with("receipt-") { receipt_index } else { requests.len() };
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
		if let QueryPayload::GetChiefAppUiSource { work_id, thread_id, fingerprint } =
			&query.payload
		{
			assert_eq!(work_id.as_str(), "work");
			assert_eq!(thread_id.as_str(), "native-thread");
			assert_eq!(fingerprint.as_str(), "c".repeat(64));
			let result = ServerMessage::QueryResult(QueryResultEnvelope {
				version: CURRENT_VERSION,
				server_id: ServerId::new(SERVER).unwrap(),
				query_id: query.query_id,
				payload: QueryResultPayload::ChiefAppUiSource(mode == "current"),
			});
			socket
				.send(Message::Text(serde_json::to_string(&result).unwrap().into()))
				.await
				.unwrap();
			return requests;
		}
		if let QueryPayload::GetChiefAppUiReceipt { request } = &query.payload {
			assert_eq!(request.work_id.as_str(), "work");
			assert_eq!(request.operation_id.as_str(), "saved-operation");
			let document = serde_json::to_vec(&serde_json::json!({"workId": if mode == "receipt-foreign" { "foreign" } else { "work" },
                "operationId":"saved-operation", "reservationId":42, "server":"fixture", "tool":"calculate", "arguments":{"value":7},
                "state":"unknown", "uncertaintyAcknowledged":false})).unwrap();
			let split = document.len() / 2;
			assert_eq!(request.offset as usize, if index == 0 { 0 } else { split });
			let response = decodex_protocol::ChiefAppUiReceiptResult::Available {
				request: Box::new(request.clone()),
				fingerprint: EntityId::new("a".repeat(64)).unwrap(),
				total_bytes: document.len() as u32,
				bytes: if index == 0 {
					document[..split].to_vec()
				} else {
					document[split..].to_vec()
				},
			};
			let result = ServerMessage::QueryResult(QueryResultEnvelope {
				version: CURRENT_VERSION,
				server_id: ServerId::new(SERVER).unwrap(),
				query_id: query.query_id,
				payload: QueryResultPayload::ChiefAppUiReceipt(response),
			});
			socket
				.send(Message::Text(serde_json::to_string(&result).unwrap().into()))
				.await
				.unwrap();
			receipt_index += 1;
			if receipt_index == 2 {
				return requests;
			}
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
				source_fingerprint: EntityId::new("c".repeat(64)).unwrap(),
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
			assert_eq!(result.unwrap().0["resources"][0]["text"], "<button>Fixture</button>");
			assert_eq!(requests.len(), 2);
		} else {
			assert!(result.is_err());
			assert_eq!(requests.len(), 1);
		}
	}
}

#[tokio::test]
async fn displayed_source_check_uses_only_the_lightweight_query() {
	for mode in ["current", "changed"] {
		let (_root, profile, server) = fixture(mode);
		let current = ChiefClient::new(profile)
			.app_ui_source(
				EntityId::new("work").unwrap(),
				EntityId::new("native-thread").unwrap(),
				EntityId::new("c".repeat(64)).unwrap(),
			)
			.await
			.unwrap();
		assert_eq!(current, mode == "current");
		assert!(server.join().unwrap().is_empty(), "source check must not reload resource chunks");
	}
}

#[gpui::test]
fn stale_display_source_closes_the_desktop_host(cx: &mut gpui::TestAppContext) {
	let (_root, profile, server) = fixture("changed");
	let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
	surface.update(visual, |s, cx| {
		s.native_history.app_ui.host = Some(native::AppHost);
		let request = ChiefAppUiRequest {
			work_id: EntityId::new("work").unwrap(),
			thread_id: EntityId::new("native-thread").unwrap(),
			turn_id: EntityId::new("turn").unwrap(),
			item_id: EntityId::new("image").unwrap(),
			offset: 0,
			fingerprint: None,
		};
		s.monitor_app_ui(
			profile,
			request,
			EntityId::new("c".repeat(64)).unwrap(),
			s.native_history.app_ui.serial,
			cx,
		);
	});
	visual.run_until_parked();
	assert!(server.join().unwrap().is_empty());
	surface.read_with(visual, |s, _| {
		assert!(s.native_history.app_ui.host.is_none());
		assert_eq!(
			s.native_history.app_ui.notice,
			Some("App source changed or disconnected. Open it again to refresh.")
		);
	});
}
