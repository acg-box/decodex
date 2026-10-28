//! Rendered model reads cross the public socket without sending mutations.
use std::{thread::JoinHandle, time::Instant};

use futures_util::{SinkExt as _, StreamExt as _};
#[cfg(test)] use gpui::AppContext as _;
use gpui::TestAppContext;
use tempfile::TempDir;
use tokio::net::UnixListener;
use tokio_tungstenite::tungstenite::Message;

#[cfg(test)]
use crate::shell::agent_surface::model_settings::{
	AgentDispatchStateDto, AgentSnapshotResult, AgentWorkStatusDto, ClientProfile,
	ConversationModel, ConversationReasoningEffort, Entity, Panel, Render, Window, WireText,
};
use crate::shell::agent_surface::{
	model_settings::{
		self, AgentModelSettingsResult, AgentSnapshotDto, AgentSurface, AgentWorkItemDto, Context,
		EntityId, IntoElement,
	},
	wire_test_support::{self, SERVER},
};
use decodex_protocol::{
	AgentActionDto, AgentCapabilitiesResult, AgentModelDto, AgentWorkKindDto, CURRENT_VERSION,
	ClientMessage, QueryPayload, QueryResultEnvelope, QueryResultPayload, ServerId, ServerMessage,
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
		agents: Default::default(),
		guardian: Default::default(),
		portfolio: Default::default(),
		archived_work_ids: Default::default(),
		agent_slots: Default::default(),
		context_references: vec![],
		connection_initializing: false,
		runtime_source: Some(EntityId::new("source").unwrap()),
		projects: vec![],
		work_items: vec![work()],
		dependencies: vec![],
		pending_events: vec![],
	}
}

#[test]
fn repeated_model_observations_only_invalidate_changed_values() {
	let state = AgentModelSettingsResult::Available {
		service_tier: None,
		work_id: EntityId::new("root").unwrap(),
		thread_id: EntityId::new("thread").unwrap(),
		account_id: EntityId::new("account").unwrap(),
		model_provider: None,
		model: Some(WireText::new("model").unwrap()),
		reasoning_effort: None,
	};
	let mut panel = Panel::default();

	assert!(panel.observe("root".into(), state.clone()));
	assert!(!panel.observe("root".into(), state.clone()));
	assert!(!panel.observe("root".into(), AgentModelSettingsResult::Unavailable));
	assert_eq!(panel.observations.get("root"), Some(&state));
	assert!(panel.observe("root".into(), AgentModelSettingsResult::NotReported));
	assert!(!panel.observe("root".into(), AgentModelSettingsResult::NotReported));
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
			w.resize(gpui::size(model_settings::px(900.), model_settings::px(600.)));
			w.draw(cx).clear(cx);
		});

		let button = visual.debug_bounds("native-model-settings-read").unwrap();

		visual.simulate_click(button.center(), Default::default());
		visual.run_until_parked();

		surface.read_with(visual, |s, _| {
			assert!(s.model_settings.task.is_none());

			let state = s.model_settings.observations.get("root").unwrap();

			match index {
				0 => {
					assert!(model_settings::settings_text(state).contains("configured-model"));
					assert!(
						model_settings::settings_text(state)
							.contains("Model provider: server-provider")
					);
				},
				1 => assert!(matches!(
					state,
					AgentModelSettingsResult::Available {
						model: None,
						reasoning_effort: None,
						model_provider: None,
						..
					}
				)),
				2 => assert_eq!(*state, AgentModelSettingsResult::NotReported),
				_ => assert_eq!(*state, AgentModelSettingsResult::Unavailable),
			}
		});
	}

	server.join().unwrap();
}

#[gpui::test]
fn model_settings_snapshot_identity_aba_requires_a_fresh_read(cx: &mut TestAppContext) {
	let surface = cx.new(AgentSurface::new);

	for change in ["thread", "removed", "source"] {
		surface.update(cx, |s, _| {
			s.apply_result(Ok(AgentSnapshotResult::Available(snapshot())));

			s.model_settings.work = Some("root".into());

			s.model_settings
				.observations
				.insert("root".into(), AgentModelSettingsResult::NotReported);

			let before = s.model_settings.epoch;
			let mut next = snapshot();

			match change {
				"thread" => next.work_items[0].codex_thread_id = Some("other".into()),
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
fn main_and_other_chats_keep_their_models_across_navigation_and_turn_changes(
	cx: &mut TestAppContext,
) {
	let surface = cx.new(AgentSurface::new);

	surface.update(cx, |s, cx| {
		let mut state = snapshot();
		let mut other = work();

		other.id = "other".into();
		other.kind = AgentWorkKindDto::Manager;
		other.parent_goal_id = Some("root".into());
		other.codex_thread_id = Some("other-thread".into());

		state.work_items.push(other);
		s.apply_result(Ok(AgentSnapshotResult::Available(state.clone())));

		for (owner, thread, model, effort) in [
			("root", "thread", "main-model", "high"),
			("other", "other-thread", "other-model", "low"),
		] {
			s.model_settings.observations.insert(
				owner.into(),
				AgentModelSettingsResult::Available {
					service_tier: None,
					work_id: EntityId::new(owner).unwrap(),
					thread_id: EntityId::new(thread).unwrap(),
					account_id: EntityId::new("account").unwrap(),
					model_provider: None,
					model: Some(WireText::new(model).unwrap()),
					reasoning_effort: Some(WireText::new(effort).unwrap()),
				},
			);
			s.model_settings.read_at.insert(owner.into(), Instant::now());
		}
		for owner in ["other", "root", "other", "root"] {
			s.open_page(owner, cx);

			assert_eq!(
				s.composer_model_label(cx),
				if owner == "root" { "main-model" } else { "other-model" }
			);
			assert_eq!(
				s.composer_effort_value(cx).as_deref(),
				Some(if owner == "root" { "high" } else { "low" })
			);
		}

		s.new_work_conversation(cx);

		assert_eq!(s.composer_model_label(cx), "main-model");
		assert_eq!(s.composer_effort_value(cx).as_deref(), Some("high"));

		let action = s.build_submission("Start this conversation", cx).unwrap();
		let AgentActionDto::NewConversation { execution, .. } = &action else {
			panic!("new conversation")
		};

		assert_eq!(execution.model.as_ref().unwrap().as_str(), "main-model");
		assert_eq!(execution.reasoning_effort, Some(ConversationReasoningEffort::High));

		let captured = s.draft_profiles.execution.capture(&action).unwrap();

		s.open_page("other", cx);
		s.new_work_conversation(cx);

		assert_eq!(
			s.composer_model_label(cx),
			"main-model",
			"reopening the draft must retain its chosen model"
		);

		s.draft_profiles.execution.accepted(Some(&captured));

		assert!(s.draft_profiles.execution.choice(&captured.0).is_empty());

		s.open_page("root", cx);

		state.work_items[0].active_turn_id = Some("new-turn".into());
		state.work_items[0].dispatch_state = AgentDispatchStateDto::Running;

		let epoch = s.model_settings.epoch;

		s.apply_result(Ok(AgentSnapshotResult::Available(state)));

		assert_ne!(s.model_settings.epoch, epoch);
		assert!(
			s.model_settings.read_at.is_empty(),
			"read the native settings again for the new turn"
		);
		assert_eq!(
			s.composer_model_label(cx),
			"main-model",
			"keep the same thread's last observation until that read completes"
		);
		assert_eq!(s.model_settings.observations.len(), 2);
		assert!(s.draft_profiles.execution.choice("root").is_empty());
		assert!(s.draft_profiles.execution.choice("other").is_empty());
	});
}

#[gpui::test]
fn failed_refresh_keeps_the_same_threads_last_native_model(cx: &mut TestAppContext) {
	let surface = cx.new(AgentSurface::new);

	surface.update(cx, |s, cx| {
		s.apply_result(Ok(AgentSnapshotResult::Available(snapshot())));

		let observation = AgentModelSettingsResult::Available {
			service_tier: None,
			work_id: EntityId::new("root").unwrap(),
			thread_id: EntityId::new("thread").unwrap(),
			account_id: EntityId::new("account").unwrap(),
			model_provider: None,
			model: Some(WireText::new("native-model").unwrap()),
			reasoning_effort: Some(WireText::new("high").unwrap()),
		};

		s.model_settings.observe("root".into(), observation.clone());

		let mut running = snapshot();

		running.work_items[0].active_turn_id = Some("next-turn".into());
		running.work_items[0].dispatch_state = AgentDispatchStateDto::Running;

		s.apply_result(Ok(AgentSnapshotResult::Available(running)));

		for _ in 0..2 {
			s.model_settings.observe("root".into(), AgentModelSettingsResult::Unavailable);

			assert_eq!(s.composer_model_label(cx), "native-model");
			assert_eq!(s.composer_effort_value(cx).as_deref(), Some("high"));
		}

		let mut changed = observation;

		if let AgentModelSettingsResult::Available { model, .. } = &mut changed {
			*model = Some(WireText::new("updated-model").unwrap());
		}

		s.model_settings.observe("root".into(), changed);

		assert_eq!(s.composer_model_label(cx), "updated-model");

		let mut rebound = snapshot();

		rebound.work_items[0].codex_thread_id = Some("other-thread".into());

		s.apply_result(Ok(AgentSnapshotResult::Available(rebound)));

		assert_eq!(s.composer_model_value(cx), None, "never retain another thread's model");

		s.model_settings.observe("root".into(), AgentModelSettingsResult::Unavailable);

		assert_eq!(s.composer_model_label(cx), "Model unavailable");
	});
}

#[gpui::test]
fn model_default_reasoning_has_the_same_value_in_new_and_existing_chats(cx: &mut TestAppContext) {
	let surface = cx.new(AgentSurface::new);

	surface.update(cx, |s, cx| {
		s.model.update(cx, |input, cx| input.set_content("native-model", cx));

		s.creation_inherit_effort = true;
		s.capabilities = Some(AgentCapabilitiesResult::Available {
			memory_enabled: None,
			models: vec![AgentModelDto {
				model: ConversationModel::new("native-model").unwrap(),
				name: "Native model".into(),
				efforts: vec![ConversationReasoningEffort::Medium],
				default_effort: Some(ConversationReasoningEffort::Medium),
				supports_fast: false,
				service_tiers: vec![],
				default_service_tier: None,
				available_cyber_programs: None,
				specialty: None,
				supports_images: false,
				availability: None,
				upgrade: None,
			}],
		});

		assert_eq!(s.composer_model_label(cx), "Native model");
		assert_eq!(s.composer_effort_value(cx).as_deref(), Some("medium"));

		s.apply_result(Ok(AgentSnapshotResult::Available(snapshot())));
		s.model_settings.observations.insert(
			"root".into(),
			AgentModelSettingsResult::Available {
				service_tier: None,
				work_id: EntityId::new("root").unwrap(),
				thread_id: EntityId::new("thread").unwrap(),
				account_id: EntityId::new("account").unwrap(),
				model_provider: None,
				model: Some(WireText::new("native-model").unwrap()),
				reasoning_effort: None,
			},
		);

		assert_eq!(s.composer_model_label(cx), "Native model");
		assert_eq!(s.composer_effort_value(cx).as_deref(), Some("medium"));

		s.effort = ConversationReasoningEffort::High;

		s.select_composer_option("model", "native-model", cx);

		assert_eq!(
			s.draft_profiles.execution.choice("root").reasoning_effort,
			Some(ConversationReasoningEffort::Medium),
			"selecting a model must preserve the visible effort, not another editor's value"
		);

		s.draft_profiles.execution = Default::default();

		s.model_settings.observations.insert("root".into(), AgentModelSettingsResult::Unavailable);

		assert_eq!(s.composer_model_value(cx), None);
		assert_eq!(s.composer_model_label(cx), "Model unavailable");
		assert_eq!(s.composer_effort_value(cx), None, "never reuse an unrelated editor value");
	});
}

#[gpui::test]
fn observed_settings_do_not_replace_newer_manual_intent(cx: &mut TestAppContext) {
	let surface = cx.new(AgentSurface::new);

	surface.update(cx, |s, cx| {
		s.visual_workspace_fixture(cx);

		let version = s.draft_profiles.execution.revision();
		let input_at_read = s.model.read(cx).content().to_owned();
		let observation = AgentModelSettingsResult::Available {
			service_tier: None,
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
			AgentModelSettingsResult::Available {
				service_tier: None,
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
		assert_eq!(s.composer_effort_value(cx).as_deref(), Some("low"));
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
			AgentModelSettingsResult::Available {
				service_tier: None,
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

		assert_eq!(
			s.model_settings.observations.get("worker"),
			Some(&AgentModelSettingsResult::Unavailable)
		);
		assert_eq!(s.composer_model_label(cx), "composer-model");
		assert_eq!(s.composer_effort_value(cx).as_deref(), Some("medium"));
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
		s.model_settings.observations.insert("agent".into(), AgentModelSettingsResult::NotReported);
		s.clear_activity_detail();

		assert_eq!(
			s.model_settings.observations.get("agent"),
			Some(&AgentModelSettingsResult::NotReported)
		);

		let before = s.model_settings.epoch;

		s.mark_stale(cx);

		assert!(
			s.model_settings.observations.is_empty(),
			"disconnect invalidates native observations"
		);
		assert_ne!(s.model_settings.epoch, before, "late reads must be discarded");

		s.model_settings.observations.insert("agent".into(), AgentModelSettingsResult::NotReported);

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
				Some(&AgentModelSettingsResult::Unavailable),
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
			AgentModelSettingsResult::NotReported
		} else {
			AgentModelSettingsResult::Available {
				service_tier: None,
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
