//! Exercise voice selection across the same-UID service socket with a lost save reply.
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
			let AgentActionDto::SetVoicePreference { work_id, review_token, voice } = &*action
			else {
				panic!("account setting")
			};
			assert_eq!(work_id.as_str(), "root");
			assert_eq!(review_token.as_str(), "a".repeat(64));
			assert_eq!(voice.as_str(), "juniper");
			actions.push(*action);
			// Lose the response after dispatch. The client must read, never resend.
			socket.close(None).await.unwrap();
			continue;
		}
		let ClientMessage::Query(query) = request else { panic!("settings read") };
		let QueryPayload::GetAgentVoiceSettings { work_id } = query.payload else {
			panic!("account query")
		};
		assert_eq!(work_id.as_str(), "root");
		let state = State::Available {
			work_id: EntityId::new("root").unwrap(),
			review_token: WireText::new(if index == 0 { "a" } else { "b" }.repeat(64)).unwrap(),
			voices: vec![WireText::new("juniper").unwrap(), WireText::new("maple").unwrap()],
			effective: Some(WireText::new("maple").unwrap()),
			preference: Some(WireText::new(if index == 0 { "maple" } else { "juniper" }).unwrap()),
		};
		let result = ServerMessage::QueryResult(QueryResultEnvelope {
			version: CURRENT_VERSION,
			server_id: ServerId::new(SERVER).unwrap(),
			query_id: query.query_id,
			payload: QueryResultPayload::AgentVoiceSettings(state),
		});
		socket.send(Message::Text(serde_json::to_string(&result).unwrap().into())).await.unwrap();
	}
	actions
}

struct VoicePanel(Entity<AgentSurface>);
impl Render for VoicePanel {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		self.0.update(cx, |s, cx| s.voice_settings_panel("root", cx))
	}
}
#[gpui::test]
fn voice_picker_sends_once_then_reads_effective_override_after_lost_reply(
	cx: &mut gpui::TestAppContext,
) {
	let (_directory, profile, server) = fixture();
	let (view, visual) = cx.add_window_view(|_, cx| {
		let surface = cx.new(AgentSurface::new);
		surface.update(cx, |s, _| {
			s.selected = Some("root".into());
			s.profile = Some(profile);
		});
		VoicePanel(surface)
	});
	let surface = view.read_with(visual, |v, _| v.0.clone());
	visual.update(|w, cx| {
		w.resize(gpui::size(px(700.), px(850.)));
		w.draw(cx).clear();
	});
	let button = visual.debug_bounds("voice-settings-toggle").unwrap();
	visual.simulate_click(button.center(), Default::default());
	visual.run_until_parked();
	visual.update(|w, cx| {
		w.draw(cx).clear();
	});
	let button = visual.debug_bounds("voice-choice-0").unwrap();
	visual.simulate_click(button.center(), Default::default());
	visual.simulate_keystrokes("space");
	visual.run_until_parked();
	assert_eq!(server.join().unwrap().len(), 1);
	surface.read_with(visual,|s,_| {
        assert!(s.voice_settings.task.is_none());
        assert!(s.voice_settings.feedback.contains("could not be confirmed"));
        assert!(matches!(&s.voice_settings.state,Some(State::Available {effective:Some(e),preference:Some(p),..}) if e.as_str()=="maple"&&p.as_str()=="juniper"));
        assert!(s.voice.is_none());
    });
}

#[gpui::test]
fn changed_native_source_retires_voice_panel_epoch(cx: &mut gpui::TestAppContext) {
	let surface = cx.new(AgentSurface::new);
	surface.update(cx, |s, cx| {
		s.visual_workspace_fixture(cx);
		let mut snapshot = s.snapshot.clone().unwrap();
		let work = snapshot.work_items[0].id.clone();
		s.voice_settings.work = Some(work);
		let epoch = s.voice_settings.epoch;
		snapshot.runtime_source = Some(EntityId::new("changed-source").unwrap());
		s.invalidate_voice_settings(&snapshot);
		assert!(s.voice_settings.work.is_none());
		assert_ne!(s.voice_settings.epoch, epoch);
	});
}

#[gpui::test]
fn voice_call_options_are_bound_to_work_and_runtime_and_keep_blank_defaults(
	cx: &mut gpui::TestAppContext,
) {
	let surface = cx.new(AgentSurface::new);
	surface.update(cx, |s, cx| {
		s.visual_workspace_fixture(cx);
		let work = s.snapshot.as_ref().unwrap().work_items[0].id.clone();
		s.snapshot.as_mut().unwrap().work_items[0].codex_thread_id = Some("voice-thread".into());
		let model = cx.new(|cx| ComposerInput::new(0, cx));
		let start = cx.new(|cx| ComposerInput::new(0, cx));
		let end = cx.new(|cx| ComposerInput::new(0, cx));
		model.update(cx, |input, cx| input.set_content("realtime-fixture", cx));
		start.update(cx, |input, cx| input.set_content("Start fixture", cx));
		s.voice_settings.next =
			Some(NextCall { target: s.voice_option_target(&work).unwrap(), model, start, end });
		let options = s.voice_call_options(&work, cx).unwrap();
		assert_eq!(options.model.unwrap().as_str(), "realtime-fixture");
		assert_eq!(options.start_instructions.unwrap().as_str(), "Start fixture");
		assert!(options.end_instructions.is_none());
		assert_eq!(s.voice_call_options("another-work", cx).unwrap(), Default::default());
		s.snapshot.as_mut().unwrap().runtime_source = Some(EntityId::new("new-runtime").unwrap());
		assert_eq!(s.voice_call_options(&work, cx).unwrap(), Default::default());
	});
}
