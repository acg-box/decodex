//! Rendered model reads cross the public socket without sending mutations.
use std::thread::JoinHandle;

use futures_util::{SinkExt as _, StreamExt as _};
use gpui::TestAppContext;
use tempfile::TempDir;
use tokio::net::UnixListener;
use tokio_tungstenite::tungstenite::Message;

use crate::shell::agent_surface::{
	model_settings::*,
	wire_test_support::{self, SERVER},
};
use decodex_protocol::{
	AgentCapabilitiesResult, AgentModelDto, AgentWorkKindDto, CURRENT_VERSION, ClientMessage,
	QueryPayload, QueryResultEnvelope, QueryResultPayload, ServerId, ServerMessage,
};

struct SettingsView {
	surface: Entity<AgentSurface>,
}
impl Render for SettingsView {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		self.surface.update(cx, |s, cx| s.model_settings_panel(&work(), cx))
	}
}

fn fixture() -> (TempDir, ClientProfile, JoinHandle<()>) {
	wire_test_support::fixture(serve)
}

fn work() -> AgentWorkItemDto {
	AgentWorkItemDto {
		id: "root".into(),
		parent_goal_id: None,
		kind: AgentWorkKindDto::Goal,
		title: "Root".into(),
		codex_thread_id: Some("thread".into()),
		active_turn_id: None,
		dispatch_state: AgentDispatchStateDto::Idle,
		status: AgentWorkStatusDto::Open,
		next_check_at_micros: None,
		created_at_micros: 1,
		updated_at_micros: 1,
	}
}

fn snapshot() -> AgentSnapshotDto {
	AgentSnapshotDto {
		runtime_source: Some(EntityId::new("source").unwrap()),
		workspaces: vec![],
		work_items: vec![work()],
		dependencies: vec![],
		pending_events: vec![],
	}
}

#[gpui::test]
fn model_settings_click_refreshes_idle_task_and_rejects_foreign_reply(cx: &mut TestAppContext) {
	let (_dir, profile, server) = fixture();
	let (view, visual) = cx.add_window_view(|_, cx| {
		let surface = cx.new(AgentSurface::new);

		cx.observe(&surface, |_, _, cx| cx.notify()).detach();

		surface.update(cx, |s, _| {
			s.apply_result(Ok(AgentSnapshotResult::Available(snapshot())));

			s.profile = Some(profile);
		});

		SettingsView { surface }
	});
	let surface = view.read_with(visual, |v, _| v.surface.clone());

	for index in 0..4 {
		visual.update(|w, cx| {
			w.resize(gpui::size(px(900.), px(600.)));
			w.draw(cx).clear();
		});

		let button = visual.debug_bounds("native-model-settings-read").unwrap();

		visual.simulate_click(button.center(), Default::default());
		visual.run_until_parked();

		surface.read_with(visual, |s, _| {
			assert!(s.model_settings.task.is_none());

			let state = s.model_settings.observations.get("root").unwrap();

			match index {
				0 => {
					assert!(settings_text(state).contains("configured-model"));
					assert!(settings_text(state).contains("Model provider: server-provider"));
				},
				1 => assert!(matches!(
					state,
					State::Available {
						model: None,
						reasoning_effort: None,
						model_provider: None,
						..
					}
				)),
				2 => assert_eq!(*state, State::NotReported),
				_ => assert_eq!(*state, State::Unavailable),
			}
		});
	}

	server.join().unwrap();
}

#[gpui::test]
fn model_settings_snapshot_identity_aba_requires_a_fresh_read(cx: &mut TestAppContext) {
	let surface = cx.new(AgentSurface::new);

	for change in ["thread", "turn", "running", "removed", "source"] {
		surface.update(cx, |s, _| {
			s.apply_result(Ok(AgentSnapshotResult::Available(snapshot())));

			s.model_settings.work = Some("root".into());

			s.model_settings.observations.insert("root".into(), State::NotReported);

			let before = s.model_settings.epoch;
			let mut next = snapshot();

			match change {
				"thread" => next.work_items[0].codex_thread_id = Some("other".into()),
				"turn" => next.work_items[0].active_turn_id = Some("other".into()),
				"running" => next.work_items[0].dispatch_state = AgentDispatchStateDto::Running,
				"removed" => next.work_items.clear(),
				_ => next.runtime_source = Some(EntityId::new("other").unwrap()),
			}

			s.apply_result(Ok(AgentSnapshotResult::Available(next)));

			assert_ne!(s.model_settings.epoch, before, "{change}");

			s.apply_result(Ok(AgentSnapshotResult::Available(snapshot())));

			assert!(s.model_settings.work.is_none());
			assert!(s.model_settings.observations.is_empty());
		});
	}
}

#[gpui::test]
fn observed_settings_do_not_replace_newer_manual_intent(cx: &mut TestAppContext) {
	let surface = cx.new(AgentSurface::new);

	surface.update(cx, |s, cx| {
		s.visual_workspace_fixture(cx);

		let version = s.draft_profiles.execution.revision();
		let input_at_read = s.model.read(cx).content().to_owned();
		let observation = State::Available {
			work_id: EntityId::new("agent").unwrap(),
			thread_id: EntityId::new("native-thread").unwrap(),
			account_id: EntityId::new("account").unwrap(),
			model_provider: Some(WireText::new("server-provider").unwrap()),
			model: Some(WireText::new("native-model").unwrap()),
			reasoning_effort: Some(WireText::new("medium").unwrap()),
		};

		s.model_settings.work = Some("agent".into());

		s.model_settings.observations.insert("agent".into(), observation.clone());
		s.adopt_composer_observation("agent", &observation, version, &input_at_read, cx);

		assert_eq!(s.model.read(cx).content(), input_at_read);
		assert_eq!(s.composer_model_label(cx), "native-model");
		assert_eq!(s.effort, ConversationReasoningEffort::Medium);
		assert!(s.draft_profiles.execution.choice("agent").is_empty());

		let observed_input = s.model.read(cx).content().to_owned();

		s.model.update(cx, |input, cx| input.set_content("typed-model-draft", cx));
		s.adopt_composer_observation("agent", &observation, version, &observed_input, cx);

		assert_eq!(s.model.read(cx).content(), "typed-model-draft");

		s.select_composer_option("model", "user-choice", cx);
		s.adopt_composer_observation("agent", &observation, version, &input_at_read, cx);

		assert_eq!(s.model.read(cx).content(), "user-choice");
		assert_eq!(
			s.draft_profiles.execution.choice("agent").model.unwrap().as_str(),
			"user-choice"
		);
	});
}

#[gpui::test]
fn a_new_explicit_model_uses_its_own_capabilities_not_the_observed_model(cx: &mut TestAppContext) {
	let surface = cx.new(AgentSurface::new);

	surface.update(cx, |s, cx| {
		s.visual_workspace_fixture(cx);

		s.model_settings.work = Some("agent".into());

		s.model_settings.observations.insert(
			"agent".into(),
			State::Available {
				work_id: EntityId::new("agent").unwrap(),
				thread_id: EntityId::new("native-thread").unwrap(),
				account_id: EntityId::new("account").unwrap(),
				model_provider: None,
				model: Some(WireText::new("old-native-model").unwrap()),
				reasoning_effort: Some(WireText::new("high").unwrap()),
			},
		);

		s.capabilities = Some(AgentCapabilitiesResult::Available {
			memory_enabled: None,
			models: [
				("old-native-model", ConversationReasoningEffort::High),
				("new-choice", ConversationReasoningEffort::Low),
			]
			.into_iter()
			.map(|(model, effort)| AgentModelDto {
				model: ConversationModel::new(model).unwrap(),
				name: model.into(),
				efforts: vec![effort.clone()],
				default_effort: Some(effort),
				supports_fast: false,
				available_cyber_programs: None,
				specialty: None,
				supports_images: true,
				availability: None,
				upgrade: None,
				service_tiers: vec![],
				default_service_tier: None,
			})
			.collect(),
		});

		s.select_composer_option("model", "new-choice", cx);

		assert_eq!(s.composer_model_value(cx).as_deref(), Some("new-choice"));
		assert_eq!(
			s.draft_profiles.execution.choice("agent").reasoning_effort,
			Some(ConversationReasoningEffort::Low)
		);
		assert_eq!(s.composer_effort_value(), "low");
	});
}

#[gpui::test]
fn inspecting_a_worker_does_not_replace_the_composers_model_observation(cx: &mut TestAppContext) {
	let surface = cx.new(AgentSurface::new);

	surface.update(cx, |s, cx| {
		s.visual_workspace_fixture(cx);

		s.composer_manager = Some("agent".into());
		s.selected = Some("worker".into());

		s.model_settings.observations.insert(
			"agent".into(),
			State::Available {
				work_id: EntityId::new("agent").unwrap(),
				thread_id: EntityId::new("native-thread").unwrap(),
				account_id: EntityId::new("account").unwrap(),
				model_provider: None,
				model: Some(WireText::new("composer-model").unwrap()),
				reasoning_effort: Some(WireText::new("medium").unwrap()),
			},
		);

		s.profile = None;

		s.read_model_settings("worker", cx);

		assert_eq!(s.model_settings.observations.get("worker"), Some(&State::Unavailable));
		assert_eq!(s.composer_model_label(cx), "composer-model");
		assert_eq!(s.composer_effort_value(), "medium");
		assert_eq!(s.model_settings.observations.len(), 2);
	});
}

#[gpui::test]
fn unavailable_service_clears_model_observations_but_detail_close_keeps_them(
	cx: &mut TestAppContext,
) {
	let surface = cx.new(AgentSurface::new);

	surface.update(cx, |s, cx| {
		s.visual_workspace_fixture(cx);
		s.model_settings.observations.insert("agent".into(), State::NotReported);
		s.clear_activity_detail();

		assert_eq!(s.model_settings.observations.get("agent"), Some(&State::NotReported));

		let before = s.model_settings.epoch;

		s.mark_stale(cx);

		assert!(
			s.model_settings.observations.is_empty(),
			"disconnect invalidates native observations"
		);
		assert_ne!(s.model_settings.epoch, before, "late reads must be discarded");

		s.model_settings.observations.insert("agent".into(), State::NotReported);

		let before = s.model_settings.epoch;

		s.apply_result(Err(()));

		assert!(
			s.model_settings.observations.is_empty(),
			"failed snapshot invalidates observations"
		);
		assert_ne!(s.model_settings.epoch, before);
	});
}

#[gpui::test]
fn ordinary_snapshot_refresh_keeps_model_settings_read(cx: &mut TestAppContext) {
	let (_dir, profile, server) = fixture();
	let surface = cx.new(AgentSurface::new);

	surface.update(cx, |s, _| {
		s.apply_result(Ok(AgentSnapshotResult::Available(snapshot())));

		s.profile = Some(profile);
	});

	for _ in 0..4 {
		surface.update(cx, |s, cx| {
			s.read_model_settings("root", cx);

			assert!(s.model_settings.task.is_some());
			// A normal refresh advances its request generation without changing the binding.
			s.generation += 1;

			s.apply_result(Ok(AgentSnapshotResult::Available(snapshot())));
		});

		cx.run_until_parked();
		surface.read_with(cx, |s, _| {
			assert!(s.model_settings.task.is_none());
			assert!(
				s.model_settings.observations.contains_key("root"),
				"an unchanged binding must retain the completed observation"
			);
		});
	}

	server.join().unwrap();
}

#[gpui::test]
fn missing_native_binding_reports_unavailable_without_starting_a_read(cx: &mut TestAppContext) {
	let (_dir, profile, server) = wire_test_support::fixture(|_| async {});

	server.join().unwrap();

	let surface = cx.new(AgentSurface::new);

	for missing in ["snapshot", "work", "thread"] {
		surface.update(cx, |s, cx| {
			s.reset_model_settings();

			let mut value = snapshot();

			match missing {
				"work" => value.work_items.clear(),
				"thread" => value.work_items[0].codex_thread_id = None,
				_ => {},
			}

			s.snapshot = (missing != "snapshot").then_some(value);
			s.selected = Some("root".into());
			s.profile = Some(profile.clone());

			s.read_model_settings("root", cx);

			assert!(s.model_settings.task.is_none(), "{missing}");
			assert_eq!(
				s.model_settings.observations.get("root"),
				Some(&State::Unavailable),
				"missing {missing} must not leave the panel reading indefinitely"
			);
		});
	}
}

async fn serve(listener: UnixListener) {
	for index in 0..4 {
		let mut socket = wire_test_support::accept(&listener).await;
		let Message::Text(text) = socket.next().await.unwrap().unwrap() else {
			panic!("text query")
		};
		let ClientMessage::Query(query) = serde_json::from_str(&text).unwrap() else {
			panic!("read only")
		};
		let QueryPayload::GetAgentModelSettings { work_id } = query.payload else {
			panic!("model read")
		};

		assert_eq!(work_id.as_str(), "root");

		let state = if index == 2 {
			State::NotReported
		} else {
			State::Available {
				work_id: EntityId::new(if index == 3 { "foreign" } else { "root" }).unwrap(),
				thread_id: EntityId::new("thread").unwrap(),
				account_id: EntityId::new("account").unwrap(),
				model_provider: (index == 0).then(|| WireText::new("server-provider").unwrap()),
				model: (index == 0).then(|| WireText::new("configured-model").unwrap()),
				reasoning_effort: (index == 0).then(|| WireText::new("future-effort").unwrap()),
			}
		};
		let result = ServerMessage::QueryResult(QueryResultEnvelope {
			version: CURRENT_VERSION,
			server_id: ServerId::new(SERVER).unwrap(),
			query_id: query.query_id,
			payload: QueryResultPayload::AgentModelSettings(state),
		});

		socket.send(Message::Text(serde_json::to_string(&result).unwrap().into())).await.unwrap();
	}
}
