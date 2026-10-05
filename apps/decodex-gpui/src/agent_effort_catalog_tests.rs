//! Catalog refresh preserves user intent; empty choices do not block creation.
use std::{
	fs::{self, Permissions},
	os::unix::fs::{MetadataExt as _, PermissionsExt as _},
	thread::{self, JoinHandle},
	time::Duration,
};

use futures_util::{SinkExt as _, StreamExt as _};
use gpui::{AppContext as _, Context, Entity, Render, TestAppContext, Window};
use tempfile::TempDir;
use tokio::{runtime::Builder, time};
use tokio_tungstenite::tungstenite::Message;

use crate::shell::agent_surface::{
	AgentActionDto, ClientProfile, LoadState,
	capabilities::{
		AgentCapabilitiesResult, AgentModelDto, AgentSurface, ConversationReasoningEffort,
		IntoElement, ParentElement,
	},
};
use decodex_protocol::{
	CURRENT_VERSION, ClientMessage, CommandPayload, ConversationModel,
	ConversationWorkingDirectory, Cursor, EntityId, InitialExecutionDefaults,
	InitialModelCatalogResult, InitialModelDefaults, ReconnectMode, ServerId, ServerMessage,
	ServerWelcome, ServiceTier, SnapshotEnvelope,
};

struct EffortView {
	surface: Entity<AgentSurface>,
}
impl Render for EffortView {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		self.surface.update(cx, |s, cx| {
			gpui::div().child(s.creation_effort_toggle(cx)).child(s.effort_scale(cx))
		})
	}
}

fn catalog(efforts: Vec<ConversationReasoningEffort>) -> AgentCapabilitiesResult {
	AgentCapabilitiesResult::Available {
		memory_enabled: None,
		models: vec![AgentModelDto {
			model: ConversationModel::new("configured-model").unwrap(),
			name: "Configured".into(),
			default_effort: efforts.first().cloned(),
			efforts,
			supports_fast: false,
			available_cyber_programs: None,
			specialty: None,
			supports_images: true,
			availability: None,
			upgrade: None,
			service_tiers: vec![],
			default_service_tier: None,
		}],
	}
}

#[gpui::test]
fn catalog_refresh_preserves_explicit_effort_until_user_selects_model(cx: &mut TestAppContext) {
	let surface = cx.new(AgentSurface::new);

	surface.update(cx, |s, cx| {
		s.visual_workspace_fixture(cx);
		s.model.update(cx, |input, cx| input.set_content("configured-model", cx));
		s.mark_model_intent(cx);

		s.effort = ConversationReasoningEffort::High;

		s.mark_effort_intent(cx);

		let owner = s.root_id().unwrap();

		s.capabilities = Some(catalog(vec![ConversationReasoningEffort::Low]));

		s.reconcile_model_options(cx);

		assert_eq!(s.effort, ConversationReasoningEffort::High);
		assert_eq!(
			s.draft_profiles.execution.choice(&owner).reasoning_effort,
			Some(ConversationReasoningEffort::High)
		);
		assert!(s.composer_capability_error(cx).is_some());

		s.select_composer_option("model", "configured-model", cx);

		assert_eq!(s.effort, ConversationReasoningEffort::Low);
		assert_eq!(
			s.draft_profiles.execution.choice(&owner).reasoning_effort,
			Some(ConversationReasoningEffort::Low)
		);
		assert!(s.composer_capability_error(cx).is_none());
	});
}

fn profile() -> (TempDir, ClientProfile, JoinHandle<AgentActionDto>) {
	let root = tempfile::tempdir_in("/tmp").unwrap();
	let path = root.path().canonicalize().unwrap();
	fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();

	fs::create_dir(path.join("server")).unwrap();
	fs::set_permissions(path.join("server"), Permissions::from_mode(0o700)).unwrap();

	let uid = fs::metadata(&path).unwrap().uid();
	let config = path.join("config.toml");

	fs::write(&config, format!("version = 1\nactive_profile = \"local\"\ncache = {{}}\n[profiles.local]\nkind = \"local\"\npolicy = \"same_uid\"\nservice_owner_uid = {uid}\nexpected_server_identity = \"018f0f9e-7b6e-4a31-8f4c-1d2e3f405162\"\n")).unwrap();
	fs::set_permissions(config, Permissions::from_mode(0o600)).unwrap();

	let profile = ClientProfile::load(&path, None).unwrap();
	let socket = path.join("server/decodex.sock");
	let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();

	fs::set_permissions(socket, Permissions::from_mode(0o600)).unwrap();

	listener.set_nonblocking(true).unwrap();

	let server = thread::spawn(move || {
		let runtime = Builder::new_current_thread().enable_all().build().unwrap();

		runtime.block_on(async {
			time::timeout(Duration::from_secs(5), async {
				let listener = tokio::net::UnixListener::from_std(listener).unwrap();
				let mut socket =
					tokio_tungstenite::accept_async(listener.accept().await.unwrap().0)
						.await
						.unwrap();
				let _hello = socket.next().await.unwrap().unwrap();
				let server_id = ServerId::new("018f0f9e-7b6e-4a31-8f4c-1d2e3f405162").unwrap();

				for message in [
					ServerMessage::Welcome(ServerWelcome {
						version: CURRENT_VERSION,
						server_id: server_id.clone(),
						instance_id: None,
						cursor: Cursor(0),
						reconnect: ReconnectMode::Snapshot,
					}),
					ServerMessage::Snapshot(SnapshotEnvelope {
						version: CURRENT_VERSION,
						server_id,
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
					panic!("text request")
				};
				let ClientMessage::Command(command) = serde_json::from_str(&text).unwrap() else {
					panic!("creation command")
				};
				let CommandPayload::Agent { action } = command.payload else {
					panic!("Agent action")
				};

				socket.close(None).await.unwrap();

				*action
			})
			.await
			.unwrap()
		})
	});

	(root, profile, server)
}

#[gpui::test]
fn cold_creation_keeps_configured_effort_when_catalog_has_no_choices(cx: &mut TestAppContext) {
	let (_directory, profile, server) = profile();
	let surface = cx.new(AgentSurface::new);

	surface.update(cx, |s, cx| {
		s.bind_profile(Some(profile), cx);

		s.state = LoadState::Ready;

		s.model.update(cx, |input, cx| input.set_content("configured-model", cx));
		s.cwd.update(cx, |input, cx| input.set_content("/tmp", cx));
		s.composer.update(cx, |input, cx| input.set_content("Create with configured effort", cx));

		let configured = ConversationReasoningEffort::new("provider-specific-effort").unwrap();

		s.effort = configured.clone();

		s.mark_model_intent(cx);
		s.mark_effort_intent(cx);
		s.mark_tier_intent();

		s.capabilities = Some(catalog(vec![]));

		s.reconcile_model_options(cx);

		assert!(s.model_efforts(cx).is_empty());

		s.set_effort_position(1., cx);
		s.submit(cx);
	});

	cx.run_until_parked();

	let action = server.join().unwrap();
	let AgentActionDto::StartConfigured { start, execution, .. } = action else {
		panic!("configured start")
	};

	assert_eq!(start.effort.as_ref().unwrap().as_str(), "provider-specific-effort");
	assert_eq!(execution.reasoning_effort.as_ref().unwrap().as_str(), "provider-specific-effort");
	assert_eq!(start.model.as_str(), "configured-model");
}

#[gpui::test]
fn empty_effort_catalog_renders_configured_value_without_slider(cx: &mut TestAppContext) {
	let (_view, visual) = cx.add_window_view(|_, cx| {
		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, cx| {
			s.model.update(cx, |input, cx| input.set_content("configured-model", cx));

			s.capabilities = Some(catalog(vec![]));
		});

		EffortView { surface }
	});

	visual.update(|window, cx| {
		window.draw(cx).clear();
	});

	assert!(visual.debug_bounds("reasoning-configured").is_some());
	assert!(visual.debug_bounds("reasoning-slider").is_none());
}

#[gpui::test]
fn native_reasoning_click_preserves_inheritance_in_both_public_start_fields(
	cx: &mut TestAppContext,
) {
	let (_directory, profile, server) = profile();
	let surface = cx.new(AgentSurface::new);

	surface.update(cx, |s, cx| {
		s.bind_profile(Some(profile), cx);

		s.state = LoadState::Ready;

		s.model.update(cx, |input, cx| input.set_content("configured-model", cx));
		s.cwd.update(cx, |input, cx| input.set_content("/tmp", cx));
		s.composer.update(cx, |input, cx| input.set_content("Use native reasoning", cx));

		s.effort = ConversationReasoningEffort::new("old-choice").unwrap();

		s.mark_model_intent(cx);
		s.mark_tier_intent();

		s.capabilities = Some(catalog(vec![ConversationReasoningEffort::High]));

		assert!(s.composer_capability_error(cx).is_some());
	});

	let visible = surface.clone();
	let (_view, visual) = cx.add_window_view(|_, _| EffortView { surface: visible });

	visual.update(|window, cx| {
		window.draw(cx).clear();
	});

	let button = visual.debug_bounds("creation-native-effort").unwrap();

	visual.simulate_click(button.center(), Default::default());
	visual.run_until_parked();
	surface.update(visual, |s, cx| {
		assert!(s.creation_inherit_effort);
		assert_eq!(s.composer_effort_value(), "Inherited");
		assert!(s.composer_capability_error(cx).is_none());
		assert!(s.submission.command.is_none(), "selection alone does not send");

		s.submit(cx);
	});
	visual.run_until_parked();

	let AgentActionDto::StartConfigured { start, execution, .. } = server.join().unwrap() else {
		panic!("configured start")
	};

	assert!(start.effort.is_none());
	assert!(execution.reasoning_effort.is_none());
	assert_eq!(start.model.as_str(), "configured-model");
}

#[gpui::test]
fn configured_defaults_reach_both_creation_fields_without_freezing_inherited_effort(
	cx: &mut TestAppContext,
) {
	let (_directory, profile, server) = profile();
	let surface = cx.new(AgentSurface::new);

	surface.update(cx, |s, cx| {
		s.bind_profile(Some(profile), cx);

		s.state = LoadState::Ready;

		s.cwd.update(cx, |input, cx| input.set_content("/tmp", cx));
		s.composer.update(cx, |input, cx| input.set_content("Use native defaults", cx));

		s.capabilities = Some(catalog(vec![]));
		s.creation_defaults = Some(InitialModelCatalogResult::Available {
			account_id: EntityId::new("account").unwrap(),
			account_revision: 1,
			working_directory: ConversationWorkingDirectory::new("/tmp").unwrap(),
			models: vec![],
			defaults: Some(Box::new(InitialModelDefaults {
				configured: InitialExecutionDefaults {
					model: Some(ConversationModel::new("configured-model").unwrap()),
					reasoning_effort: None,
					service_tier: Some(ServiceTier::new("flex").unwrap()),
				},
				managed: Default::default(),
				catalog_model: None,
			})),
		});

		s.apply_creation_defaults(cx);

		assert_eq!(s.composer_effort_value(), "Inherited");
		assert!(!s.creation_intent.model && !s.creation_intent.reasoning);

		s.reconcile_model_options(cx);
		s.submit(cx);
	});

	cx.run_until_parked();

	let AgentActionDto::StartConfigured { start, execution, .. } = server.join().unwrap() else {
		panic!("configured start")
	};

	assert_eq!(start.model.as_str(), "configured-model");
	assert_eq!(execution.model.unwrap().as_str(), "configured-model");
	assert!(start.effort.is_none() && execution.reasoning_effort.is_none());
	assert_eq!(execution.service_tier.unwrap().as_str(), "flex");
}
