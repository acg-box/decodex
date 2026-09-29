//! Exercise real Preview clicks through the same-UID local WebSocket protocol.
use super::*;
use decodex_protocol::{
	AgentTimelineAttachment, AgentTimelineAttachmentSource, AgentTimelinePage, CURRENT_VERSION,
	ClientMessage, QueryPayload, QueryResultEnvelope, QueryResultPayload, ServerId, ServerMessage,
};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

use super::super::wire_test_support::SERVER;
const PNG: &[u8] = include_bytes!("../../../assets/workspace-symbols/plus.png");

fn fixture(
	mode: &'static str,
) -> (tempfile::TempDir, ClientProfile, std::thread::JoinHandle<Vec<AgentMediaRequest>>) {
	super::super::wire_test_support::fixture(move |listener| serve(listener, mode))
}

async fn serve(listener: tokio::net::UnixListener, mode: &str) -> Vec<AgentMediaRequest> {
	let mut requests = Vec::new();
	while requests.len() < if mode == "complete" { 2 } else { 1 } {
		let index = requests.len();
		let mut socket = super::super::wire_test_support::accept(&listener).await;
		let Message::Text(text) = socket.next().await.unwrap().unwrap() else {
			panic!("text query")
		};
		let ClientMessage::Query(query) = serde_json::from_str::<ClientMessage>(&text).unwrap()
		else {
			panic!("query")
		};
		if matches!(
			query.payload,
			QueryPayload::WaitForAgentOutput { .. } | QueryPayload::GetNativeAgents { .. }
		) {
			// The workspace also opens an independent live-output and agent observations.
			socket.close(None).await.unwrap();
			continue;
		}
		let QueryPayload::GetAgentMedia { request } = query.payload else {
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
			AgentMediaResult::Unavailable
		} else {
			AgentMediaResult::Available {
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
			payload: QueryResultPayload::AgentMedia(result),
		});
		socket.send(Message::Text(serde_json::to_string(&result).unwrap().into())).await.unwrap();
	}
	requests
}

fn prepare(
	surface: &mut AgentSurface,
	profile: ClientProfile,
	cx: &mut Context<AgentSurface>,
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
		AgentTimelinePage {
			thread_id: "native-thread".into(),
			entries: vec![AgentTimelineEntry {
				position: 1,
				content: Content::Item {
					phase: None,
					app_ui: false,
					turn_id: "turn".into(),
					item_id: "image".into(),
					kind: "userMessage".into(),
					text: "Image question".into(),
					truncated: false,
					activity: None,
					attachments: vec![AgentTimelineAttachment {
						index: 0,
						kind: "localImage".into(),
						label: "photo.png".into(),
						source: AgentTimelineAttachmentSource::Local
					}],
				}
			}],
			next_cursor: None,
			weather: Default::default(),
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
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
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
