//! Exercise real Preview clicks through the same-UID local WebSocket protocol.
use super::*;
use decodex_protocol::{
	CURRENT_VERSION, ChiefTimelineAttachment, ChiefTimelineAttachmentSource, ChiefTimelinePage,
	ClientMessage, Cursor, QueryPayload, QueryResultEnvelope, QueryResultPayload, ReconnectMode,
	ServerId, ServerMessage, ServerWelcome, SnapshotEnvelope,
};
use futures_util::{SinkExt, StreamExt};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use tokio_tungstenite::tungstenite::Message;

const SERVER: &str = "018f0f9e-7b6e-4a31-8f4c-1d2e3f405162";
const PNG: &[u8] = include_bytes!("../../../assets/workspace-symbols/plus.png");

fn fixture(
	mode: &'static str,
) -> (tempfile::TempDir, ClientProfile, std::thread::JoinHandle<Vec<ChiefMediaRequest>>) {
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

async fn serve(listener: tokio::net::UnixListener, mode: &str) -> Vec<ChiefMediaRequest> {
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
		let QueryPayload::GetChiefMedia { request } = query.payload else {
			panic!("media query: {:?}", query.payload)
		};
		assert_eq!(request.thread_id.as_str(), "native-thread");
		assert_eq!(request.turn_id.as_str(), "turn");
		assert_eq!(request.item_id.as_str(), "image");
		assert_eq!(request.index, 0);
		let split = PNG.len() / 2;
		assert_eq!(request.offset as usize, if index == 0 { 0 } else { split });
		assert_eq!(
			request.fingerprint,
			if index == 0 { None } else { Some(EntityId::new("a".repeat(64)).unwrap()) }
		);
		requests.push(request.clone());
		let result = if mode == "unavailable" {
			ChiefMediaResult::Unavailable
		} else {
			ChiefMediaResult::Available {
				request: Box::new(request),
				account_id: EntityId::new(if mode == "account" {
					"other-account"
				} else {
					"account"
				})
				.unwrap(),
				fingerprint: EntityId::new("a".repeat(64)).unwrap(),
				mime_type: "image/png".into(),
				total_bytes: PNG.len() as u32,
				bytes: if index == 0 { PNG[..split].to_vec() } else { PNG[split..].to_vec() },
			}
		};
		let result = ServerMessage::QueryResult(QueryResultEnvelope {
			version: CURRENT_VERSION,
			server_id: ServerId::new(SERVER).unwrap(),
			query_id: query.query_id,
			payload: QueryResultPayload::ChiefMedia(result),
		});
		socket.send(Message::Text(serde_json::to_string(&result).unwrap().into())).await.unwrap();
	}
	requests
}

fn prepare(
	surface: &mut ChiefSurface,
	profile: ClientProfile,
	cx: &mut Context<ChiefSurface>,
) -> String {
	surface.visual_workspace_fixture(cx);
	surface.graph_visible = false;
	surface.profile = Some(profile);
	let work = surface
		.snapshot
		.as_mut()
		.unwrap()
		.work_items
		.iter_mut()
		.find(|work| Some(&work.id) == surface.selected.as_ref())
		.unwrap();
	work.codex_thread_id = Some("native-thread".into());
	let work_id = work.id.clone();
	assert!(surface.native_history.replace(
		Binding {
			work: work_id.clone(),
			thread: "native-thread".into(),
			account: "account".into()
		},
		ChiefTimelinePage {
			thread_id: "native-thread".into(),
			entries: vec![ChiefTimelineEntry {
				position: 1,
				content: Content::Item {
					turn_id: "turn".into(),
					item_id: "image".into(),
					kind: "userMessage".into(),
					text: "Image question".into(),
					truncated: false,
					activity: None,
					attachments: vec![ChiefTimelineAttachment {
						index: 0,
						kind: "localImage".into(),
						label: "photo.png".into(),
						source: ChiefTimelineAttachmentSource::Local
					}],
				}
			}],
			next_cursor: None,
			active_realtime_session_at_page_start: None,
		}
	));
	cx.notify();
	work_id
}

#[gpui::test]
fn preview_click_reads_real_local_chunks_and_rejects_changed_account(
	cx: &mut gpui::TestAppContext,
) {
	for mode in ["complete", "account", "unavailable"] {
		let (_root, profile, server) = fixture(mode);
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		visual.update(|window, _| window.resize(gpui::size(gpui::px(1000.), gpui::px(700.))));
		let work = surface.update(visual, |surface, cx| prepare(surface, profile, cx));
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		let action = visual.debug_bounds("native-media-action").unwrap();
		visual.simulate_click(action.center(), Default::default());
		visual.run_until_parked();
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		let requests = server.join().unwrap();
		assert!(requests.iter().all(|request| request.work_id.as_str() == work));
		surface.read_with(visual, |surface, _| {
			assert!(surface.native_history.preview.task.is_none());
			assert_eq!(surface.native_history.preview.image.is_some(), mode == "complete");
			assert_eq!(surface.native_history.preview.notice.is_some(), mode != "complete");
		});
		assert_eq!(visual.debug_bounds("native-media-preview").is_some(), mode == "complete");
	}
}
