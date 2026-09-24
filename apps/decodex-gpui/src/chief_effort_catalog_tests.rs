//! Catalog refresh preserves user intent; empty choices do not block creation.
use super::{
	super::{ChiefActionDto, ClientProfile, LoadState},
	*,
};
use decodex_protocol::{
	CURRENT_VERSION, ClientMessage, CommandPayload, ConversationModel, Cursor, ReconnectMode,
	ServerId, ServerMessage, ServerWelcome, SnapshotEnvelope,
};
use futures_util::{SinkExt, StreamExt};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use tokio_tungstenite::tungstenite::Message;

fn catalog(efforts: Vec<ConversationReasoningEffort>) -> ChiefCapabilitiesResult {
	ChiefCapabilitiesResult::Available {
		memory_enabled: None,
		models: vec![ChiefModelDto {
			model: ConversationModel::new("configured-model").unwrap(),
			name: "Configured".into(),
			default_effort: efforts.first().cloned(),
			efforts,
			supports_fast: false,
			supports_images: true,
			availability: None,
			upgrade: None,
			service_tiers: vec![],
			default_service_tier: None,
		}],
	}
}

#[gpui::test]
fn catalog_refresh_preserves_explicit_effort_until_user_selects_model(
	cx: &mut gpui::TestAppContext,
) {
	let surface = cx.new(ChiefSurface::new);
	surface.update(cx, |s, cx| {
		s.visual_workspace_fixture(cx);
		s.model.update(cx, |input, cx| input.set_content("configured-model", cx));
		s.mark_model_intent(cx);
		s.effort = ConversationReasoningEffort::High;
		s.mark_effort_intent();
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

fn profile() -> (tempfile::TempDir, ClientProfile, std::thread::JoinHandle<ChiefActionDto>) {
	let root = tempfile::tempdir_in("/tmp").unwrap();
	let path = root.path().canonicalize().unwrap();
	std::fs::create_dir(path.join("server")).unwrap();
	std::fs::set_permissions(path.join("server"), std::fs::Permissions::from_mode(0o700)).unwrap();
	let uid = std::fs::metadata(&path).unwrap().uid();
	let config = path.join("config.toml");
	std::fs::write(&config, format!("version = 1\nactive_profile = \"local\"\ncache = {{}}\n[profiles.local]\nkind = \"local\"\npolicy = \"same_uid\"\nservice_owner_uid = {uid}\nexpected_server_identity = \"018f0f9e-7b6e-4a31-8f4c-1d2e3f405162\"\n")).unwrap();
	std::fs::set_permissions(config, std::fs::Permissions::from_mode(0o600)).unwrap();
	let profile = ClientProfile::load(&path, None).unwrap();
	let socket = path.join("server/decodex.sock");
	let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
	std::fs::set_permissions(socket, std::fs::Permissions::from_mode(0o600)).unwrap();
	listener.set_nonblocking(true).unwrap();
	let server = std::thread::spawn(move || {
		let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
		runtime.block_on(async {
			tokio::time::timeout(std::time::Duration::from_secs(5), async {
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
				let CommandPayload::Chief { action } = command.payload else {
					panic!("Chief action")
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
fn cold_creation_keeps_configured_effort_when_catalog_has_no_choices(
	cx: &mut gpui::TestAppContext,
) {
	let (_directory, profile, server) = profile();
	let surface = cx.new(ChiefSurface::new);
	surface.update(cx, |s, cx| {
		s.bind_profile(Some(profile), cx);
		s.state = LoadState::Ready;
		s.model.update(cx, |input, cx| input.set_content("configured-model", cx));
		s.cwd.update(cx, |input, cx| input.set_content("/tmp", cx));
		s.composer.update(cx, |input, cx| input.set_content("Create with configured effort", cx));
		let configured = ConversationReasoningEffort::new("provider-specific-effort").unwrap();
		s.effort = configured.clone();
		s.capabilities = Some(catalog(vec![]));
		s.reconcile_model_options(cx);
		assert!(s.model_efforts(cx).is_empty());
		s.set_effort_position(1., cx);
		s.submit(cx);
	});
	cx.run_until_parked();
	let action = server.join().unwrap();
	let ChiefActionDto::StartConfigured { start, execution, .. } = action else {
		panic!("configured start")
	};
	assert_eq!(start.effort.as_str(), "provider-specific-effort");
	assert_eq!(execution.reasoning_effort.as_str(), "provider-specific-effort");
	assert_eq!(start.model.as_str(), "configured-model");
}

struct EffortView {
	surface: gpui::Entity<ChiefSurface>,
}
impl gpui::Render for EffortView {
	fn render(&mut self, _: &mut gpui::Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
		self.surface.update(cx, |s, cx| s.effort_scale(cx))
	}
}

#[gpui::test]
fn empty_effort_catalog_renders_configured_value_without_slider(cx: &mut gpui::TestAppContext) {
	let (_view, visual) = cx.add_window_view(|_, cx| {
		let surface = cx.new(ChiefSurface::new);
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
