//! Exercise context selection across the same-UID service socket with a lost save reply.
use std::thread::JoinHandle;

use futures_util::{SinkExt as _, StreamExt as _};
#[cfg(test)] use gpui::AppContext as _;
use gpui::TestAppContext;
use tempfile::TempDir;
use tokio::net::UnixListener;
use tokio_tungstenite::tungstenite::Message;

#[cfg(test)]
use crate::shell::agent_surface::context_settings::{self, ClientProfile, Entity, Render, Window};
use crate::shell::agent_surface::{
	context_settings::{
		AgentActionDto, AgentContextFeature, AgentContextPreference, AgentContextSettingsResult,
		AgentSurface, Context, EntityId, IntoElement,
	},
	wire_test_support::{self, SERVER},
};
use decodex_protocol::{
	CURRENT_VERSION, ClientMessage, CommandPayload, QueryPayload, QueryResultEnvelope,
	QueryResultPayload, ServerId, ServerMessage, WireText,
};

struct ContextPanel(Entity<AgentSurface>, Entity<crate::settings_surface::SettingsSurface>);
impl Render for ContextPanel {
	fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
		self.1.clone()
	}
}

fn fixture(
	feature: AgentContextFeature,
) -> (TempDir, ClientProfile, JoinHandle<Vec<AgentActionDto>>) {
	wire_test_support::fixture(move |listener| serve(listener, feature))
}

#[gpui::test]
fn context_picker_sends_once_then_reads_effective_override_after_lost_reply(
	cx: &mut TestAppContext,
) {
	run_setting(cx, AgentContextFeature::AsyncCompaction, "context-choice-async");
}

#[gpui::test]
fn global_settings_mcp_switch_sends_only_its_reviewed_server(cx: &mut TestAppContext) {
	run_setting(
		cx,
		AgentContextFeature::McpCatalogStartup { server: WireText::new("docs.v2").unwrap() },
		"context-choice-mcp-docs.v2",
	);
}

fn run_setting(cx: &mut TestAppContext, feature: AgentContextFeature, selector: &'static str) {
	let (_directory, profile, server) = fixture(feature);
	let (view, visual) = cx.add_window_view(|_, cx| {
		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, _| {
			s.selected = Some("root".into());
			s.profile = Some(profile);
		});

		let settings = cx.new(|cx| {
			let mut s = crate::settings_surface::SettingsSurface::new(
				crate::desktop_settings::DesktopSettingsController::production(),
				cx,
			);
			s.category = crate::settings_surface::SettingsCategory::Agents;
			s.bind_native_agent(surface.clone(), cx);
			s
		});

		ContextPanel(surface, settings)
	});
	let surface = view.read_with(visual, |v, _| v.0.clone());

	visual.update(|w, cx| {
		w.resize(gpui::size(context_settings::px(1000.), context_settings::px(1400.)));
		w.draw(cx).clear(cx);
	});

	assert!(visual.debug_bounds("automatic-prompt-suggestions").is_some());
	let button = visual.debug_bounds("context-settings-refresh").unwrap();

	visual.simulate_click(button.center(), Default::default());
	visual.run_until_parked();
	visual.update(crate::ui_frame::draw_current_frame);

	let button = visual.debug_bounds(selector).unwrap();

	visual.simulate_click(button.center(), Default::default());
	visual.simulate_keystrokes("space");
	visual.run_until_parked();

	assert_eq!(server.join().unwrap().len(), 1);

	surface.read_with(visual,|s,_| {
        assert!(s.context_settings.task.is_none());
        assert!(s.context_settings.feedback.contains("unconfirmed"));
        assert!(matches!(&s.context_settings.state,Some(AgentContextSettingsResult::Available {preferences,..}) if preferences[0].effective && preferences[0].preference == Some(false)));

    });
}

async fn serve(listener: UnixListener, expected: AgentContextFeature) -> Vec<AgentActionDto> {
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
			let AgentActionDto::SetContextPreference { work_id, review_token, feature, enabled } =
				&*action
			else {
				panic!("account setting")
			};

			assert_eq!(work_id.as_str(), "root");
			assert_eq!(review_token.as_str(), "a".repeat(64));
			assert_eq!(feature, &expected);
			assert!(!enabled);

			actions.push(*action);
			// Lose the response after dispatch. The client must read, never resend.
			socket.close(None).await.unwrap();

			continue;
		}

		let ClientMessage::Query(query) = request else { panic!("settings read") };
		let QueryPayload::GetAgentContextSettings { work_id } = query.payload else {
			panic!("account query")
		};

		assert_eq!(work_id.as_str(), "root");

		let state = AgentContextSettingsResult::Available {
			work_id: EntityId::new("root").unwrap(),
			review_token: WireText::new(if index == 0 { "a" } else { "b" }.repeat(64)).unwrap(),
			preferences: vec![AgentContextPreference {
				feature: expected.clone(),
				effective: true,
				preference: if index == 0
					&& matches!(expected, AgentContextFeature::McpCatalogStartup { .. })
				{
					None
				} else {
					Some(index == 0)
				},
			}],
		};
		let result = ServerMessage::QueryResult(QueryResultEnvelope {
			version: CURRENT_VERSION,
			server_id: ServerId::new(SERVER).unwrap(),
			query_id: query.query_id,
			payload: QueryResultPayload::AgentContextSettings(state),
		});

		socket.send(Message::Text(serde_json::to_string(&result).unwrap().into())).await.unwrap();
	}

	actions
}
