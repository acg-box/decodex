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
	let (_directory, profile, server) = fixture(feature.clone());
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
	assert!(visual.debug_bounds("context-settings-refresh").is_none());
	let settings = view.read_with(visual, |v, _| v.1.clone());
	settings.update(visual, |s, cx| s.refresh(cx));
	surface.update(visual, |s, _| s.generation += 1);
	visual.run_until_parked();
	visual.update(crate::ui_frame::draw_current_frame);

	for _ in 0..3 {
		visual.update(crate::ui_frame::draw_current_frame);
	}

	assert!(visual.debug_bounds("context-override-async").is_none());
	if context_settings::is_catalog(&feature) {
		assert!(visual.debug_bounds(selector).is_none(), "server controls start collapsed");
		let toggle = visual.debug_bounds("context-mcp-catalogs").unwrap();
		visual.simulate_click(toggle.center(), Default::default());
		settle_disclosure(visual);
	}

	let button = visual.debug_bounds(selector).unwrap();

	visual.simulate_click(button.center(), Default::default());
	visual.run_until_parked();

	assert_eq!(server.join().unwrap().len(), 1);
	visual.update(crate::ui_frame::draw_current_frame);
	let override_selector = if context_settings::is_catalog(&feature) {
		"context-override-mcp-docs.v2"
	} else {
		"context-override-async"
	};
	assert!(visual.debug_bounds(override_selector).is_some());

	surface.read_with(visual, |s, _| {
		assert!(s.context_settings.task.is_none());
		assert!(s.context_settings.feedback.contains("Could not confirm"));
		assert!(
			matches!(&s.context_settings.state, Some(AgentContextSettingsResult::Available {preferences, ..})
			if preferences[0].effective && preferences[0].preference == Some(false))
		);
	});
}

fn settle_disclosure(visual: &mut gpui::VisualTestContext) {
	visual.update(crate::ui_frame::draw_current_frame);
	visual.update(crate::ui_frame::draw_current_frame);
	std::thread::sleep(std::time::Duration::from_millis(250));
	visual.update(crate::ui_frame::draw_current_frame);
}

#[gpui::test]
fn native_defaults_disclosure_tracks_servers_without_dispatching(cx: &mut TestAppContext) {
	let (view, visual) = cx.add_window_view(|_, cx| {
		let surface = cx.new(AgentSurface::new);
		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.visual_context_settings(false, cx);
		});
		let settings = cx.new(|cx| {
			let mut settings = crate::settings_surface::SettingsSurface::new(
				crate::desktop_settings::DesktopSettingsController::production(),
				cx,
			);
			settings.category = crate::settings_surface::SettingsCategory::Agents;
			settings.bind_native_agent(surface.clone(), cx);
			settings.bind_agent(&surface, cx);
			settings
		});
		ContextPanel(surface, settings)
	});
	let surface = view.read_with(visual, |v, _| v.0.clone());
	visual.simulate_resize(gpui::size(context_settings::px(760.), context_settings::px(1600.)));
	visual.update(crate::ui_frame::draw_current_frame);
	assert!(visual.debug_bounds("context-choice-mcp-xcode").is_none());
	assert!(visual.debug_bounds("context-override-async").is_none());
	assert!(visual.debug_bounds("context-settings-refresh").is_none());
	assert!(visual.debug_bounds("context-settings-retry").is_none());
	let toggle = visual.debug_bounds("context-mcp-catalogs").unwrap();
	visual.simulate_click(toggle.center(), Default::default());
	settle_disclosure(visual);
	assert!(visual.debug_bounds("context-choice-mcp-xcode").is_some());
	assert!(visual.debug_bounds("context-choice-mcp-brave").is_some());
	assert!(visual.debug_bounds("context-override-mcp-xcode").is_none());
	assert!(visual.debug_bounds("context-override-mcp-brave").is_some());

	surface.update(visual, |s, cx| {
		let Some(AgentContextSettingsResult::Available { preferences, .. }) =
			&mut s.context_settings.state
		else {
			panic!("settings fixture")
		};
		preferences.push(AgentContextPreference {
			feature: AgentContextFeature::McpCatalogStartup {
				server: WireText::new("new-server").unwrap(),
			},
			effective: false,
			preference: None,
		});
		cx.notify();
	});
	settle_disclosure(visual);
	assert!(visual.debug_bounds("context-choice-mcp-new-server").is_some());
	let toggle = visual.debug_bounds("context-mcp-catalogs").unwrap();
	visual.simulate_click(toggle.center(), Default::default());
	settle_disclosure(visual);
	assert!(visual.debug_bounds("context-choice-mcp-xcode").is_none());
	surface.read_with(visual, |s, _| {
		assert!(!s.context_settings.catalog_expanded);
		assert!(s.context_settings.task.is_none(), "disclosure never starts a configuration request");
		assert!(s.context_settings.feedback.is_empty());
		assert!(matches!(&s.context_settings.state,Some(AgentContextSettingsResult::Available {preferences, ..}) if preferences.len() == 8));
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
