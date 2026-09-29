//! Rendered detail continuation through the public same-UID query contract.
use super::{super::wire_test_support::SERVER, *};
use decodex_protocol::{
	CURRENT_VERSION, ClientMessage, QueryPayload, QueryResultEnvelope, QueryResultPayload,
	ServerId, ServerMessage,
};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;
fn fixture() -> (tempfile::TempDir, ClientProfile, std::thread::JoinHandle<usize>) {
	super::super::wire_test_support::fixture(move |listener| serve(listener))
}

async fn serve(listener: tokio::net::UnixListener) -> usize {
	for index in 0..2 {
		let mut socket = super::super::wire_test_support::accept(&listener).await;
		let Message::Text(text) = socket.next().await.unwrap().unwrap() else { panic!("query") };
		let ClientMessage::Query(query) = serde_json::from_str(&text).unwrap() else {
			panic!("query")
		};
		let QueryPayload::GetAgentActivityDetail { work_id, turn_id, item_id, cursor } =
			query.payload
		else {
			panic!("detail")
		};
		assert_eq!(
			(work_id.as_str(), turn_id.as_str(), item_id.as_str()),
			("work", "turn", "item")
		);
		let continuation = AgentActivityDetailCursor {
			offset: 5,
			fingerprint: WireText::new("a".repeat(64)).unwrap(),
		};
		assert_eq!(cursor, (index == 1).then_some(continuation.clone()));
		let result = AgentActivityDetailResult::Available {
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
					payload: QueryResultPayload::AgentActivityDetail(result),
				}))
				.unwrap()
				.into(),
			))
			.await
			.unwrap();
	}
	2
}

fn work() -> AgentWorkItemDto {
	AgentWorkItemDto {
		id: "work".into(),
		parent_goal_id: None,
		kind: decodex_protocol::AgentWorkKindDto::Manager,
		title: "Manager".into(),
		codex_thread_id: Some("thread".into()),
		active_turn_id: None,
		dispatch_state: AgentDispatchStateDto::Idle,
		status: AgentWorkStatusDto::Open,
		next_check_at_micros: None,
		created_at_micros: 1,
		updated_at_micros: 1,
	}
}
struct DetailView {
	surface: Entity<AgentSurface>,
	kind: &'static str,
}
impl Render for DetailView {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		self.surface.update(cx, |s, cx| {
			s.detail_row(
				&work(),
				&AgentActivityDto {
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
			let surface = cx.new(AgentSurface::new);
			cx.observe(&surface, |_, _, cx| cx.notify()).detach();
			surface.update(cx, |s, cx| {
				s.apply_result(Ok(AgentSnapshotResult::Available(AgentSnapshotDto {
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
  assert!(matches!(&s.activity_detail.value,Some((_,Some(AgentActivityDetailResult::Available {text,offset:5,next:None,..}))) if text=="last"));
 }));
	}
}
