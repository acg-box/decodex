//! Real account settings clicks across the same-UID socket; no native provider is mocked as
//! delivered.
use super::{super::wire_test_support::SERVER, *};
use decodex_protocol::{
	CURRENT_VERSION, ClientMessage, CommandPayload, QueryPayload, QueryResultEnvelope,
	QueryResultPayload, ServerId, ServerMessage,
};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;
fn fixture() -> (tempfile::TempDir, ClientProfile, std::thread::JoinHandle<Vec<AgentActionDto>>) {
	super::super::wire_test_support::fixture(serve)
}

async fn serve(listener: tokio::net::UnixListener) -> Vec<AgentActionDto> {
	let mut actions = Vec::new();
	for index in 0..3 {
		let mut socket = super::super::wire_test_support::accept(&listener).await;
		let Message::Text(text) = socket.next().await.unwrap().unwrap() else {
			panic!("text request")
		};
		let request: ClientMessage = serde_json::from_str(&text).unwrap();
		if index == 1 {
			let ClientMessage::Command(command) = request else { panic!("setting command") };
			let CommandPayload::Agent { action } = command.payload else { panic!("Agent command") };
			let AgentActionDto::SetAppToolExposure { work_id, connector_id, review_token, omit } =
				&*action
			else {
				panic!("account setting")
			};
			assert_eq!(work_id.as_str(), "root");
			assert_eq!(connector_id.as_str(), "calendar");
			assert_eq!(review_token.as_str(), "a".repeat(64));
			assert_eq!(*omit, Some(vec![Surface::Direct]));
			actions.push(*action);
			// Lose the response after dispatch. The client must read, never resend.
			socket.close(None).await.unwrap();
			continue;
		}
		let ClientMessage::Query(query) = request else { panic!("settings read") };
		let QueryPayload::GetAgentAppExposure { work_id, connector_id } = query.payload else {
			panic!("account query")
		};
		assert_eq!(work_id.as_str(), "root");
		assert_eq!(connector_id.as_str(), "calendar");
		let state = State::Available {
			work_id,
			connector_id,
			review_token: WireText::new(if index == 0 { "a" } else { "b" }.repeat(64)).unwrap(),
			effective: Some(vec![]),
			preference: if index == 0 { None } else { Some(vec!["direct".into()]) },
			can_update: true,
			last_outcome: if index == 0 { None } else { Some("unknown".into()) },
		};
		let result = ServerMessage::QueryResult(QueryResultEnvelope {
			version: CURRENT_VERSION,
			server_id: ServerId::new(SERVER).unwrap(),
			query_id: query.query_id,
			payload: QueryResultPayload::AgentAppExposure(state),
		});
		socket.send(Message::Text(serde_json::to_string(&result).unwrap().into())).await.unwrap();
	}
	actions
}

#[gpui::test]
fn app_exposure_click_sends_once_and_reads_after_lost_reply(cx: &mut gpui::TestAppContext) {
	use decodex_protocol::{AgentPendingEventDto, AgentWorkKindDto};
	let (_directory, profile, server) = fixture();
	let (view, visual) = cx.add_window_view(|_, cx| {
		let surface = cx.new(AgentSurface::new);
		cx.observe(&surface, |_, _, cx| cx.notify()).detach();
		ExposureView { surface }
	});
	let surface = view.read_with(visual, |v, _| v.surface.clone());
	surface.update(visual, |s, cx| {
		s.apply_result(Ok(AgentSnapshotResult::Available(AgentSnapshotDto {
			runtime_source: Some(EntityId::new("native-source").unwrap()),
			workspaces: vec![],
			dependencies: vec![],
			work_items: vec![AgentWorkItemDto {
				id: "root".into(),
				parent_goal_id: None,
				kind: AgentWorkKindDto::Goal,
				title: "Agent".into(),
				codex_thread_id: Some("thread".into()),
				active_turn_id: None,
				dispatch_state: AgentDispatchStateDto::Idle,
				status: AgentWorkStatusDto::Open,
				next_check_at_micros: None,
				created_at_micros: 1,
				updated_at_micros: 1,
			}],
			pending_events: vec![AgentPendingEventDto {
				id: 7,
				source_event_id: "approval".into(),
				work_item_id: "root".into(),
				event_kind: "server_request_pending".into(),
				created_at_micros: 1,
				delivery_claimed: false,
			}],
		})));
		s.integrations = Some(("root".into(), None));
		s.profile = Some(profile);
		s.update_app_exposure("root", "calendar", false, cx);
	});
	visual.run_until_parked();
	visual.update(|w, cx| {
		w.resize(gpui::size(px(1180.), px(2600.)));
		w.draw(cx).clear();
	});
	let toggle = visual.debug_bounds("app-exposure-surface-0").unwrap();
	visual.simulate_click(toggle.center(), Default::default());
	visual.update(|w, cx| {
		w.draw(cx).clear();
	});
	let save = visual.debug_bounds("app-exposure-save").unwrap();
	visual.simulate_click(save.center(), Default::default());
	visual.run_until_parked();
	assert_eq!(server.join().unwrap().len(), 1);
	surface.read_with(visual,|s,_|{
 assert!(s.app_exposure.feedback.contains("not retried"));
 assert!(s.app_exposure.task.is_none());
 assert!(matches!(&s.app_exposure.state,Some(State::Available{preference:Some(values),..}) if values==&["direct"]));
 });
}

struct ExposureView {
	surface: Entity<AgentSurface>,
}
impl Render for ExposureView {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		self.surface.update(cx, |s, cx| s.integrations_panel("root", cx))
	}
}
