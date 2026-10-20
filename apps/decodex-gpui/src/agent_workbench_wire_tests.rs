//! Desk request navigation reads the exact event and shows the existing native form.
use super::*;
use crate::shell::agent_surface::{self, wire_test_support};
use decodex_protocol::{
	AgentRequestResult, AgentRequestText, AgentWorkKindDto, CURRENT_VERSION, ClientMessage,
	QueryPayload, QueryResultEnvelope, QueryResultPayload, ServerId, ServerMessage,
};
use futures_util::{SinkExt as _, StreamExt as _};
use gpui::{AppContext as _, Entity, Render, Window};
use tokio_tungstenite::tungstenite::Message;

struct RequestView(Entity<AgentSurface>);

impl Render for RequestView {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		self.0.update(cx, |s, cx| {
			if s.workspace.graph_expanded {
				s.render_agent_workbench(cx)
			} else {
				s.render_transcript_view(window, cx)
			}
		})
	}
}

fn request(event: i64) -> AgentRequestResult {
	AgentRequestResult::Available {
		event_id: event,
		work_id: "release".into(),
		method: "item/commandExecution/requestApproval".into(),
		request_json: AgentRequestText::new(
			serde_json::json!({"command":format!("printf trial-{event}"),"availableDecisions":["accept","decline"]}).to_string(),
		)
		.unwrap(),
	}
}

#[gpui::test]
fn task_owner_desk_reads_the_second_request_and_shows_its_approval_form(
	cx: &mut gpui::TestAppContext,
) {
	let (_root, profile, server) = wire_test_support::fixture(serve);
	let (view, visual) = cx.add_window_view(|_, cx| {
		let surface = cx.new(agent_surface::work_board::tests::fixture);
		cx.observe(&surface, |_, _, cx| cx.notify()).detach();
		surface.update(cx, |s, _| {
			let snapshot = s.snapshot.as_mut().unwrap();
			snapshot.work_items.iter_mut().find(|work| work.id == "release").unwrap().kind =
				AgentWorkKindDto::Task;
			snapshot.pending_events = [901, 902]
				.into_iter()
				.map(|id| decodex_protocol::AgentPendingEventDto {
					id,
					source_event_id: format!("pending-{id}"),
					work_item_id: "release".into(),
					event_kind: "permission_pending".into(),
					created_at_micros: 1,
					delivery_claimed: false,
				})
				.collect();
			s.selected = Some("release".into());
			s.native_agents.selected = Some(("release".into(), "thread-release".into()));
			s.workspace.graph_expanded = true;
			s.request = Some(request(901));
			s.profile = Some(profile);
		});
		RequestView(surface)
	});
	visual.simulate_resize(gpui::size(gpui::px(1100.), gpui::px(1000.)));
	visual.update(crate::ui_frame::draw_frame);
	let second = visual.debug_bounds("workbench-item-request-902").unwrap();
	visual.simulate_click(second.center(), Default::default());
	visual.run_until_parked();
	assert_eq!(server.join().unwrap(), 902);
	visual.run_until_parked();
	visual.update(crate::ui_frame::draw_frame);
	view.read_with(visual, |view, cx| {
		let surface = view.0.read(cx);
		assert_eq!(surface.native_agents.selected.as_ref().unwrap().1, "thread-release");
		assert_eq!(surface.request, Some(request(902)));
		assert!(surface.submission.command.is_none());
	});
	assert!(visual.debug_bounds("request-accept").is_some());
	assert!(visual.debug_bounds("request-decline").is_some());
	let surface = view.read_with(visual, |view, _| view.0.clone());
	surface.update(visual, |s, cx| {
		s.native_agents.selected = Some(("release".into(), "native-child".into()));
		cx.notify();
	});
	visual.update(crate::ui_frame::draw_frame);
	assert!(visual.debug_bounds("request-accept").is_none());
	assert!(visual.debug_bounds("request-decline").is_none());
}

async fn serve(listener: tokio::net::UnixListener) -> i64 {
	let mut socket = wire_test_support::accept(&listener).await;
	let Message::Text(text) = socket.next().await.unwrap().unwrap() else { panic!("query frame") };
	let ClientMessage::Query(query) = serde_json::from_str(&text).unwrap() else {
		panic!("request navigation must be read only")
	};
	let QueryPayload::GetAgentRequest { event_id } = query.payload else {
		panic!("exact request read")
	};
	let result = ServerMessage::QueryResult(QueryResultEnvelope {
		version: CURRENT_VERSION,
		server_id: ServerId::new(wire_test_support::SERVER).unwrap(),
		query_id: query.query_id,
		payload: QueryResultPayload::AgentRequest(request(event_id)),
	});
	socket.send(Message::Text(serde_json::to_string(&result).unwrap().into())).await.unwrap();
	event_id
}
