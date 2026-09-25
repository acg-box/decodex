//! Rendered model reads cross the public socket without sending mutations.
use super::*;
use decodex_protocol::{
	CURRENT_VERSION, ClientMessage, Cursor, QueryPayload, QueryResultEnvelope, QueryResultPayload,
	ReconnectMode, ServerId, ServerMessage, ServerWelcome, SnapshotEnvelope,
};
use futures_util::{SinkExt, StreamExt};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use tokio_tungstenite::tungstenite::Message;
const SERVER: &str = "018f0f9e-7b6e-4a31-8f4c-1d2e3f405162";
fn fixture() -> (tempfile::TempDir, ClientProfile, std::thread::JoinHandle<()>) {
	let root = tempfile::tempdir_in("/tmp").unwrap();
	let path = root.path().canonicalize().unwrap();
	let server = path.join("server");
	std::fs::create_dir(&server).unwrap();
	std::fs::set_permissions(&server, std::fs::Permissions::from_mode(0o700)).unwrap();
	let uid = std::fs::metadata(&path).unwrap().uid();
	let config = path.join("config.toml");
	std::fs::write(&config,format!("version = 1\nactive_profile = \"local\"\ncache = {{}}\n[profiles.local]\nkind = \"local\"\npolicy = \"same_uid\"\nservice_owner_uid = {uid}\nexpected_server_identity = \"{SERVER}\"\n")).unwrap();
	std::fs::set_permissions(config, std::fs::Permissions::from_mode(0o600)).unwrap();
	let socket_path = server.join("decodex.sock");
	let listener = std::os::unix::net::UnixListener::bind(&socket_path).unwrap();
	std::fs::set_permissions(socket_path, std::fs::Permissions::from_mode(0o600)).unwrap();
	listener.set_nonblocking(true).unwrap();
	let profile = ClientProfile::load(&path, None).unwrap();
	let thread = std::thread::spawn(move || {
		let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
		runtime.block_on(async {
			let listener = tokio::net::UnixListener::from_std(listener).unwrap();
			tokio::time::timeout(std::time::Duration::from_secs(5), serve(listener)).await.unwrap()
		})
	});
	(root, profile, thread)
}

async fn serve(listener: tokio::net::UnixListener) {
	for index in 0..4 {
		let mut socket =
			tokio_tungstenite::accept_async(listener.accept().await.unwrap().0).await.unwrap();
		let _hello = socket.next().await.unwrap().unwrap();
		for message in [
			ServerMessage::Welcome(ServerWelcome {
				version: CURRENT_VERSION,
				server_id: ServerId::new(SERVER).unwrap(),
				instance_id: None,
				cursor: Cursor(0),
				reconnect: ReconnectMode::Snapshot,
			}),
			ServerMessage::Snapshot(SnapshotEnvelope {
				version: CURRENT_VERSION,
				server_id: ServerId::new(SERVER).unwrap(),
				cursor: Cursor(0),
				items: vec![],
			}),
		] {
			socket
				.send(Message::Text(serde_json::to_string(&message).unwrap().into()))
				.await
				.unwrap();
		}
		let Message::Text(text) = socket.next().await.unwrap().unwrap() else {
			panic!("text query")
		};
		let ClientMessage::Query(query) = serde_json::from_str(&text).unwrap() else {
			panic!("read only")
		};
		let QueryPayload::GetChiefModelSettings { work_id } = query.payload else {
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
			payload: QueryResultPayload::ChiefModelSettings(state),
		});
		socket.send(Message::Text(serde_json::to_string(&result).unwrap().into())).await.unwrap();
	}
}
fn work() -> ChiefWorkItemDto {
	ChiefWorkItemDto {
		id: "root".into(),
		parent_goal_id: None,
		kind: decodex_protocol::ChiefWorkKindDto::Goal,
		title: "Root".into(),
		codex_thread_id: Some("thread".into()),
		active_turn_id: None,
		dispatch_state: ChiefDispatchStateDto::Idle,
		status: ChiefWorkStatusDto::Open,
		next_check_at_micros: None,
		created_at_micros: 1,
		updated_at_micros: 1,
	}
}

fn snapshot() -> ChiefSnapshotDto {
	ChiefSnapshotDto {
		runtime_source: Some(EntityId::new("source").unwrap()),
		workspaces: vec![],
		work_items: vec![work()],
		dependencies: vec![],
		pending_events: vec![],
	}
}
struct SettingsView {
	surface: Entity<ChiefSurface>,
}
impl Render for SettingsView {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		self.surface.update(cx, |s, cx| s.model_settings_panel(&work(), cx))
	}
}
#[gpui::test]
fn model_settings_click_refreshes_idle_task_and_rejects_foreign_reply(
	cx: &mut gpui::TestAppContext,
) {
	let (_dir, profile, server) = fixture();
	let (view, visual) = cx.add_window_view(|_, cx| {
		let surface = cx.new(ChiefSurface::new);
		cx.observe(&surface, |_, _, cx| cx.notify()).detach();
		surface.update(cx, |s, _| {
			s.apply_result(Ok(ChiefSnapshotResult::Available(snapshot())));
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
fn model_settings_snapshot_identity_aba_requires_a_fresh_read(cx: &mut gpui::TestAppContext) {
	let surface = cx.new(ChiefSurface::new);
	for change in ["thread", "turn", "running", "removed", "source"] {
		surface.update(cx, |s, _| {
			s.apply_result(Ok(ChiefSnapshotResult::Available(snapshot())));
			s.model_settings.work = Some("root".into());
			s.model_settings.observations.insert("root".into(), State::NotReported);
			let before = s.model_settings.epoch;
			let mut next = snapshot();
			match change {
				"thread" => next.work_items[0].codex_thread_id = Some("other".into()),
				"turn" => next.work_items[0].active_turn_id = Some("other".into()),
				"running" => next.work_items[0].dispatch_state = ChiefDispatchStateDto::Running,
				"removed" => next.work_items.clear(),
				_ => next.runtime_source = Some(EntityId::new("other").unwrap()),
			}
			s.apply_result(Ok(ChiefSnapshotResult::Available(next)));
			assert_ne!(s.model_settings.epoch, before, "{change}");
			s.apply_result(Ok(ChiefSnapshotResult::Available(snapshot())));
			assert!(s.model_settings.work.is_none());
			assert!(s.model_settings.observations.is_empty());
		});
	}
}

#[gpui::test]
fn observed_settings_do_not_replace_newer_manual_intent(cx: &mut gpui::TestAppContext) {
	let surface = cx.new(ChiefSurface::new);
	surface.update(cx, |s, cx| {
		s.visual_workspace_fixture(cx);
		let version = s.draft_profiles.execution.revision();
		let input_at_read = s.model.read(cx).content().to_owned();
		let observation = State::Available {
			work_id: EntityId::new("chief").unwrap(),
			thread_id: EntityId::new("native-thread").unwrap(),
			account_id: EntityId::new("account").unwrap(),
			model_provider: Some(WireText::new("server-provider").unwrap()),
			model: Some(WireText::new("native-model").unwrap()),
			reasoning_effort: Some(WireText::new("medium").unwrap()),
		};
		s.model_settings.work = Some("chief".into());
		s.model_settings.observations.insert("chief".into(), observation.clone());
		s.adopt_composer_observation("chief", &observation, version, &input_at_read, cx);
		assert_eq!(s.model.read(cx).content(), input_at_read);
		assert_eq!(s.composer_model_label(cx), "native-model");
		assert_eq!(s.effort, ConversationReasoningEffort::Medium);
		assert!(s.draft_profiles.execution.choice("chief").is_empty());
		let observed_input = s.model.read(cx).content().to_owned();
		s.model.update(cx, |input, cx| input.set_content("typed-model-draft", cx));
		s.adopt_composer_observation("chief", &observation, version, &observed_input, cx);
		assert_eq!(s.model.read(cx).content(), "typed-model-draft");

		s.select_composer_option("model", "user-choice", cx);
		s.adopt_composer_observation("chief", &observation, version, &input_at_read, cx);
		assert_eq!(s.model.read(cx).content(), "user-choice");
		assert_eq!(
			s.draft_profiles.execution.choice("chief").model.unwrap().as_str(),
			"user-choice"
		);
	});
}

#[gpui::test]
fn a_new_explicit_model_uses_its_own_capabilities_not_the_observed_model(
	cx: &mut gpui::TestAppContext,
) {
	let surface = cx.new(ChiefSurface::new);
	surface.update(cx, |s, cx| {
		s.visual_workspace_fixture(cx);
		s.model_settings.work = Some("chief".into());
		s.model_settings.observations.insert(
			"chief".into(),
			State::Available {
				work_id: EntityId::new("chief").unwrap(),
				thread_id: EntityId::new("native-thread").unwrap(),
				account_id: EntityId::new("account").unwrap(),
				model_provider: None,
				model: Some(WireText::new("old-native-model").unwrap()),
				reasoning_effort: Some(WireText::new("high").unwrap()),
			},
		);
		s.capabilities = Some(decodex_protocol::ChiefCapabilitiesResult::Available {
			memory_enabled: None,
			models: [
				("old-native-model", ConversationReasoningEffort::High),
				("new-choice", ConversationReasoningEffort::Low),
			]
			.into_iter()
			.map(|(model, effort)| decodex_protocol::ChiefModelDto {
				model: ConversationModel::new(model).unwrap(),
				name: model.into(),
				efforts: vec![effort.clone()],
				default_effort: Some(effort),
				supports_fast: false,
				available_cyber_programs: None,
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
			s.draft_profiles.execution.choice("chief").reasoning_effort,
			Some(ConversationReasoningEffort::Low)
		);
		assert_eq!(s.composer_effort_value(), "low");
	});
}

#[gpui::test]
fn inspecting_a_worker_does_not_replace_the_composers_model_observation(
	cx: &mut gpui::TestAppContext,
) {
	let surface = cx.new(ChiefSurface::new);
	surface.update(cx, |s, cx| {
		s.visual_workspace_fixture(cx);
		s.composer_manager = Some("chief".into());
		s.selected = Some("worker".into());
		s.model_settings.observations.insert(
			"chief".into(),
			State::Available {
				work_id: EntityId::new("chief").unwrap(),
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
