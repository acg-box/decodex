//! Rendered detail continuation through the public same-UID query contract.
use super::*;
use decodex_protocol::{
	CURRENT_VERSION, ClientMessage, Cursor, QueryPayload, QueryResultEnvelope, QueryResultPayload,
	ReconnectMode, ServerId, ServerMessage, ServerWelcome, SnapshotEnvelope,
};
use futures_util::{SinkExt, StreamExt};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use tokio_tungstenite::tungstenite::Message;
const SERVER: &str = "018f0f9e-7b6e-4a31-8f4c-1d2e3f405162";
fn fixture() -> (tempfile::TempDir, ClientProfile, std::thread::JoinHandle<usize>) {
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
			tokio::time::timeout(std::time::Duration::from_secs(5), serve(listener)).await.unwrap()
		})
	});
	(root, profile, thread)
}

async fn serve(listener: tokio::net::UnixListener) -> usize {
	for index in 0..2 {
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
		let Message::Text(text) = socket.next().await.unwrap().unwrap() else { panic!("query") };
		let ClientMessage::Query(query) = serde_json::from_str(&text).unwrap() else {
			panic!("query")
		};
		let QueryPayload::GetChiefActivityDetail { work_id, turn_id, item_id, cursor } =
			query.payload
		else {
			panic!("detail")
		};
		assert_eq!(
			(work_id.as_str(), turn_id.as_str(), item_id.as_str()),
			("work", "turn", "item")
		);
		let continuation = ChiefActivityDetailCursor {
			offset: 5,
			fingerprint: WireText::new("a".repeat(64)).unwrap(),
		};
		assert_eq!(cursor, (index == 1).then_some(continuation.clone()));
		let result = ChiefActivityDetailResult::Available {
			text: if index == 0 { "first" } else { "last" }.into(),
			offset: if index == 0 { 0 } else { 5 },
			truncated: index == 0,
			next: (index == 0).then_some(continuation),
		};
		socket
			.send(Message::Text(
				serde_json::to_string(&ServerMessage::QueryResult(QueryResultEnvelope {
					version: CURRENT_VERSION,
					server_id: ServerId::new(SERVER).unwrap(),
					query_id: query.query_id,
					payload: QueryResultPayload::ChiefActivityDetail(result),
				}))
				.unwrap()
				.into(),
			))
			.await
			.unwrap();
	}
	2
}

fn work() -> ChiefWorkItemDto {
	ChiefWorkItemDto {
		id: "work".into(),
		parent_goal_id: None,
		kind: decodex_protocol::ChiefWorkKindDto::Manager,
		title: "Manager".into(),
		codex_thread_id: Some("thread".into()),
		active_turn_id: None,
		dispatch_state: ChiefDispatchStateDto::Idle,
		status: ChiefWorkStatusDto::Open,
		next_check_at_micros: None,
		created_at_micros: 1,
		updated_at_micros: 1,
	}
}
struct DetailView {
	surface: Entity<ChiefSurface>,
	kind: &'static str,
}
impl Render for DetailView {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		self.surface.update(cx, |s, cx| {
			s.detail_row(
				&work(),
				&ChiefActivityDto {
					turn_id: "turn".into(),
					item_id: "item".into(),
					kind: self.kind.into(),
					status: "completed".into(),
					label: "Patch".into(),
					detail: String::new(),
					duration_ms: None,
				},
				div().child("Patch"),
				cx,
			)
		})
	}
}
#[gpui::test]
fn rendered_detail_continuation_reads_exact_cursor_without_accumulating_pages(
	cx: &mut gpui::TestAppContext,
) {
	for kind in [
		"fileChange",
		"commandExecution",
		"webSearch",
		"mcpToolCall",
		"functionCallOutput",
		"imageView",
	] {
		let (_directory, profile, server) = fixture();
		let (view, visual) = cx.add_window_view(|_, cx| {
			let surface = cx.new(ChiefSurface::new);
			cx.observe(&surface, |_, _, cx| cx.notify()).detach();
			surface.update(cx, |s, cx| {
				s.apply_result(Ok(ChiefSnapshotResult::Available(ChiefSnapshotDto {
					runtime_source: Some(EntityId::new("source").unwrap()),
					workspaces: vec![],
					dependencies: vec![],
					pending_events: vec![],
					work_items: vec![work()],
				})));
				s.profile = Some(profile);
				s.load_activity_detail(("work".into(), "turn".into(), "item".into()), None, cx);
			});
			DetailView { surface, kind }
		});
		visual.run_until_parked();
		visual.update(|w, cx| {
			w.resize(gpui::size(px(800.), px(600.)));
			w.draw(cx).clear();
		});
		// Disclosure uses wall-clock animation: measure, begin expansion, then settle.
		visual.update(|w, cx| {
			w.draw(cx).clear();
		});
		std::thread::sleep(std::time::Duration::from_millis(250));
		visual.update(|w, cx| {
			w.draw(cx).clear();
		});
		let before = view
			.read_with(visual, |v, cx| v.surface.read_with(cx, |s, _| s.activity_detail.revision));
		let button =
			visual.debug_bounds("detail-next-action").expect("manager can continue its full patch");
		assert!(button.size.height > px(0.), "{button:?}");
		visual.simulate_click(button.center(), Default::default());
		view.read_with(visual, |v, cx| {
			v.surface.read_with(cx, |s, _| {
				assert!(
					s.activity_detail.revision == before + 1,
					"click missed: {button:?}, revision {}",
					s.activity_detail.revision
				)
			})
		});
		visual.run_until_parked();
		assert_eq!(server.join().unwrap(), 2);
		view.read_with(visual,|v,cx| v.surface.read_with(cx,|s,_| {
  assert!(matches!(&s.activity_detail.value,Some((_,Some(ChiefActivityDetailResult::Available {text,offset:5,next:None,..}))) if text=="last"));
 }));
	}
}
