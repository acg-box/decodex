//! Exercise voice selection across the same-UID service socket with a lost save reply.
use std::{future, thread::JoinHandle};

use futures_util::{SinkExt as _, StreamExt as _};
use gpui::{AppContext as _, TestAppContext};
use tempfile::TempDir;
use tokio::net::UnixListener;
use tokio_tungstenite::tungstenite::Message;

#[cfg(test)] use crate::shell::agent_surface::voice_settings::{ClientProfile, Render, Window};
use crate::shell::agent_surface::{
	voice_settings::{
		AgentActionDto, AgentSurface, AgentVoiceSettingsResult, ComposerInput, Context, Entity,
		EntityId, IntoElement, NextCall, WireText,
	},
	wire_test_support::{self, SERVER},
};
use decodex_protocol::{
	CURRENT_VERSION, ClientMessage, CommandPayload, QueryPayload, QueryResultEnvelope,
	QueryResultPayload, ServerId, ServerMessage,
};

struct VoicePanel(Entity<AgentSurface>);
impl Render for VoicePanel {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		self.0.update(cx, |s, cx| s.voice_settings_panel("root", cx))
	}
}

fn fixture() -> (TempDir, ClientProfile, JoinHandle<Vec<AgentActionDto>>) {
	wire_test_support::fixture(serve)
}

#[gpui::test]
fn voice_picker_sends_once_then_reads_effective_override_after_lost_reply(cx: &mut TestAppContext) {
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
		w.resize(gpui::size(gpui::px(700.), gpui::px(850.)));
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
        assert!(matches!(&s.voice_settings.state,Some(AgentVoiceSettingsResult::Available {effective:Some(e),preference:Some(p),..}) if e.as_str()=="maple"&&p.as_str()=="juniper"));
        assert!(s.voice.is_none());
    });
}

#[gpui::test]
fn changed_native_source_retires_voice_panel_epoch(cx: &mut TestAppContext) {
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
	cx: &mut TestAppContext,
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

#[gpui::test]
fn changing_pages_retires_pending_voice_settings(cx: &mut TestAppContext) {
	let surface = cx.new(AgentSurface::new);

	surface.update(cx, |s, cx| {
		s.visual_workspace_fixture(cx);

		let old = s.selected.clone().unwrap();
		let next = s
			.snapshot
			.as_ref()
			.unwrap()
			.work_items
			.iter()
			.find(|work| work.id != old)
			.unwrap()
			.id
			.clone();

		s.voice_settings.work = Some(old);
		s.voice_settings.task = Some(cx.spawn(async |_, _| future::pending().await));

		let epoch = s.voice_settings.epoch;

		s.open_page(&next, cx);

		assert_eq!(s.selected.as_deref(), Some(next.as_str()));
		assert!(
			s.voice_settings.task.is_none(),
			"The old read must not block settings on the new page"
		);
		assert!(s.voice_settings.work.is_none());
		assert_ne!(s.voice_settings.epoch, epoch);
	});
}

async fn serve(listener: UnixListener) -> Vec<AgentActionDto> {
	let mut actions = Vec::new();

	for index in 0..3 {
		let mut socket = wire_test_support::accept(&listener).await;
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

		let state = AgentVoiceSettingsResult::Available {
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

#[gpui::test]
fn managed_voice_preflight_rejects_unavailable_or_changed_source(cx: &mut TestAppContext) {
	for changed_source in [false, true] {
		let (_directory, profile, server) =
			wire_test_support::fixture(move |listener| async move {
				let mut socket = wire_test_support::accept(&listener).await;
				let Message::Text(text) = socket.next().await.unwrap().unwrap() else {
					panic!("request")
				};
				let ClientMessage::Query(query) = serde_json::from_str(&text).unwrap() else {
					panic!("query")
				};
				let QueryPayload::GetAgentVoiceSettings { work_id } = query.payload else {
					panic!("voice settings")
				};
				let state = if changed_source {
					AgentVoiceSettingsResult::Available {
						work_id,
						review_token: WireText::new("a".repeat(64)).unwrap(),
						voices: vec![WireText::new("juniper").unwrap()],
						effective: None,
						preference: None,
					}
				} else {
					AgentVoiceSettingsResult::Unavailable
				};
				let response = ServerMessage::QueryResult(QueryResultEnvelope {
					version: CURRENT_VERSION,
					server_id: ServerId::new(SERVER).unwrap(),
					query_id: query.query_id,
					payload: QueryResultPayload::AgentVoiceSettings(state),
				});
				socket
					.send(Message::Text(serde_json::to_string(&response).unwrap().into()))
					.await
					.unwrap();
			});
		let (view, visual) = cx.add_window_view(|window, cx| {
			let surface = cx.new(AgentSurface::new);
			surface.update(cx, |s, cx| {
				s.visual_workspace_fixture(cx);
				s.profile = Some(profile);
				s.snapshot.as_mut().unwrap().work_items[0].codex_thread_id =
					Some("voice-thread".into());
				s.start_voice(window, cx);
				assert!(s.voice.is_none());
				assert!(s.voice_task.is_some());
				if changed_source {
					s.snapshot.as_mut().unwrap().runtime_source =
						Some(EntityId::new("replacement-runtime").unwrap());
				}
			});
			VoicePanel(surface)
		});
		visual.run_until_parked();
		server.join().unwrap();
		view.read_with(visual, |view, cx| {
			let s = view.0.read(cx);
			assert!(s.voice.is_none());
			assert!(s.voice_task.is_none());
			assert_eq!(
				s.feedback,
				if changed_source {
					"Voice start canceled because the conversation changed."
				} else {
					"Voice is unavailable or disabled by managed policy."
				}
			);
		});
	}
}
