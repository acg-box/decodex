//! Exercise search selection across the same-UID service socket with a lost save reply.
use std::thread::JoinHandle;

use futures_util::{SinkExt as _, StreamExt as _};
#[cfg(test)] use gpui::AppContext as _;
use gpui::TestAppContext;
use tempfile::TempDir;
use tokio::net::UnixListener;
use tokio_tungstenite::tungstenite::Message;

#[cfg(test)]
use crate::shell::agent_surface::search_settings::{self, ClientProfile, Entity, Render, Window};
use crate::shell::agent_surface::{
	search_settings::{
		AgentActionDto, AgentSearchSettingsResult, AgentSurface, Context, EntityId, IntoElement,
		WireText,
	},
	wire_test_support::{self, SERVER},
};
use decodex_protocol::{
	CURRENT_VERSION, ClientMessage, CommandPayload, QueryPayload, QueryResultEnvelope,
	QueryResultPayload, ServerId, ServerMessage,
};

struct SearchPanel(Entity<AgentSurface>);
impl Render for SearchPanel {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		self.0.update(cx, |s, cx| s.search_settings_panel("root", cx))
	}
}

fn fixture() -> (TempDir, ClientProfile, JoinHandle<Vec<AgentActionDto>>) {
	wire_test_support::fixture(serve)
}

#[gpui::test]
fn search_picker_sends_once_then_reads_effective_override_after_lost_reply(
	cx: &mut TestAppContext,
) {
	let (_directory, profile, server) = fixture();
	let (view, visual) = cx.add_window_view(|_, cx| {
		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, _| {
			s.selected = Some("root".into());
			s.profile = Some(profile);
		});

		SearchPanel(surface)
	});
	let surface = view.read_with(visual, |v, _| v.0.clone());

	visual.update(|w, cx| {
		w.resize(gpui::size(search_settings::px(700.), search_settings::px(850.)));
		w.draw(cx).clear(cx);
	});

	let button = visual.debug_bounds("search-settings-toggle").unwrap();

	visual.simulate_click(button.center(), Default::default());
	visual.run_until_parked();
	visual.update(|w, cx| {
		w.draw(cx).clear(cx);
	});

	let button = visual.debug_bounds("search-choice-0").unwrap();

	visual.simulate_click(button.center(), Default::default());
	visual.simulate_keystrokes("space");
	visual.run_until_parked();

	assert_eq!(server.join().unwrap().len(), 1);

	surface.read_with(visual,|s,_| {
        assert!(s.search_settings.task.is_none());
        assert!(s.search_settings.feedback.contains("could not be confirmed"));
        assert!(matches!(&s.search_settings.state,Some(AgentSearchSettingsResult::Available {effective:Some(e),preference:Some(p),..}) if e.as_str()=="live"&&p.as_str()=="indexed"));

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
			let AgentActionDto::SetSearchPreference { work_id, review_token, mode } = &*action
			else {
				panic!("account setting")
			};

			assert_eq!(work_id.as_str(), "root");
			assert_eq!(review_token.as_str(), "a".repeat(64));
			assert_eq!(mode.as_str(), "indexed");

			actions.push(*action);
			// Lose the response after dispatch. The client must read, never resend.
			socket.close(None).await.unwrap();

			continue;
		}

		let ClientMessage::Query(query) = request else { panic!("settings read") };
		let QueryPayload::GetAgentSearchSettings { work_id } = query.payload else {
			panic!("account query")
		};

		assert_eq!(work_id.as_str(), "root");

		let state = AgentSearchSettingsResult::Available {
			work_id: EntityId::new("root").unwrap(),
			review_token: WireText::new(if index == 0 { "a" } else { "b" }.repeat(64)).unwrap(),
			modes: vec![WireText::new("indexed").unwrap(), WireText::new("live").unwrap()],
			effective: Some(WireText::new("live").unwrap()),
			preference: Some(WireText::new(if index == 0 { "live" } else { "indexed" }).unwrap()),
		};
		let result = ServerMessage::QueryResult(QueryResultEnvelope {
			version: CURRENT_VERSION,
			server_id: ServerId::new(SERVER).unwrap(),
			query_id: query.query_id,
			payload: QueryResultPayload::AgentSearchSettings(state),
		});

		socket.send(Message::Text(serde_json::to_string(&result).unwrap().into())).await.unwrap();
	}

	actions
}
