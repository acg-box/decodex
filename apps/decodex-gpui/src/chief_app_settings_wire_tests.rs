//! Real account settings clicks across the same-UID socket; no native provider is mocked as
//! delivered.
use super::*;
use decodex_protocol::{
	CURRENT_VERSION, ClientMessage, CommandPayload, Cursor, QueryPayload, QueryResultEnvelope,
	QueryResultPayload, ReconnectMode, ServerId, ServerMessage, ServerWelcome, SnapshotEnvelope,
};
use futures_util::{SinkExt, StreamExt};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use tokio_tungstenite::tungstenite::Message;
const SERVER: &str = "018f0f9e-7b6e-4a31-8f4c-1d2e3f405162";
fn fixture(
	saved: bool,
) -> (tempfile::TempDir, ClientProfile, std::thread::JoinHandle<Vec<ChiefActionDto>>) {
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
			tokio::time::timeout(std::time::Duration::from_secs(5), serve(listener, saved))
				.await
				.unwrap()
		})
	});
	(root, profile, thread)
}

async fn serve(listener: tokio::net::UnixListener, saved: bool) -> Vec<ChiefActionDto> {
	let mut actions = Vec::new();
	for index in 0..3 {
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
			panic!("text request")
		};
		let request: ClientMessage = serde_json::from_str(&text).unwrap();
		if index == 1 {
			let ClientMessage::Command(command) = request else { panic!("setting command") };
			let CommandPayload::Chief { action } = command.payload else { panic!("Chief command") };
			assert_action(&action, saved);
			actions.push(*action);
			// Lose the response after dispatch. The client must read, never resend.
			socket.close(None).await.unwrap();
			continue;
		}
		let ClientMessage::Query(query) = request else { panic!("settings read") };
		let payload = reply(query.payload, index, saved);
		let result = ServerMessage::QueryResult(QueryResultEnvelope {
			version: CURRENT_VERSION,
			server_id: ServerId::new(SERVER).unwrap(),
			query_id: query.query_id,
			payload,
		});
		socket.send(Message::Text(serde_json::to_string(&result).unwrap().into())).await.unwrap();
	}
	actions
}

fn assert_action(action: &ChiefActionDto, saved: bool) {
	if saved {
		let ChiefActionDto::SetSavedAppSetting {
			work_id,
			thread_id,
			connector_id,
			link_id,
			review_token,
			edit,
		} = action
		else {
			panic!("saved edit")
		};
		assert_eq!(work_id.as_str(), "root");
		assert_eq!(thread_id.as_str(), "thread");
		assert_eq!(connector_id.as_str(), "calendar");
		assert_eq!(link_id.as_str(), "work");
		assert_eq!(review_token.as_str(), "a".repeat(64));
		assert_eq!(*edit, Edit::ApprovalMode(None));
	} else {
		let ChiefActionDto::SetAppSetting { work_id, event_id, review_token, edit } = action else {
			panic!("request edit")
		};
		assert_eq!(work_id.as_str(), "root");
		assert_eq!(*event_id, 7);
		assert_eq!(review_token.as_str(), "a".repeat(64));
		assert_eq!(*edit, Edit::ApprovalMode(Some(Mode::Auto)));
	}
}
fn reply(payload: QueryPayload, index: usize, saved: bool) -> QueryResultPayload {
	if saved {
		let QueryPayload::GetChiefSavedAppSettings { work_id } = payload else {
			panic!("saved query")
		};
		assert_eq!(work_id.as_str(), "root");
		let connections = if index == 0 {
			vec![decodex_protocol::ChiefSavedAppConnection {
				connector_id: "calendar".into(),
				link_id: "work".into(),
				review_token: "a".repeat(64),
				user_mode: Some("approve".into()),
				user_reviewer: None,
				effective_mode: Some("approve".into()),
				effective_reviewer: None,
			}]
		} else {
			vec![]
		};
		return QueryResultPayload::ChiefSavedAppSettings(
			decodex_protocol::ChiefSavedAppSettingsResult::Available {
				work_id: "root".into(),
				thread_id: "thread".into(),
				config_file: "/native/config.toml".into(),
				connections,
				can_update: true,
				last_edit: None,
			},
		);
	}
	let QueryPayload::GetChiefAppSettings { work_id, event_id } = payload else {
		panic!("account query")
	};
	assert_eq!(work_id.as_str(), "root");
	assert_eq!(event_id, 7);
	let state = State::Available {
		can_update: true,
		config_file: "/native/config.toml".into(),
		last_edit: None,
		connector_id: "calendar".into(),
		link_id: "work".into(),
		review_token: if index == 0 { "a" } else { "b" }.repeat(64),
		effective_mode: Some(if index == 0 { "prompt" } else { "auto" }.into()),
		effective_reviewer: None,
		user_mode: Some(if index == 0 { "prompt" } else { "auto" }.into()),
		user_reviewer: None,
	};
	QueryResultPayload::ChiefAppSettings(state)
}
struct AccountView {
	surface: Entity<ChiefSurface>,
}
impl Render for AccountView {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		self.surface.update(cx, |s, cx| s.account_settings_panel(7, cx))
	}
}
#[gpui::test]
fn real_settings_click_reads_then_sends_once_and_refreshes_unknown_result(
	cx: &mut gpui::TestAppContext,
) {
	use decodex_protocol::{ChiefPendingEventDto, ChiefWorkKindDto};
	use serde_json::json;
	let (_directory, profile, server) = fixture(false);
	let (view, visual) = cx.add_window_view(|_, cx| {
		let surface = cx.new(ChiefSurface::new);
		cx.observe(&surface, |_, _, cx| cx.notify()).detach();
		AccountView { surface }
	});
	let surface = view.read_with(visual, |v, _| v.surface.clone());
	surface.update(visual,|s,_| {
  s.apply_result(Ok(ChiefSnapshotResult::Available(ChiefSnapshotDto {runtime_source:None,workspaces:vec![],dependencies:vec![],work_items:vec![ChiefWorkItemDto{id:"root".into(),parent_goal_id:None,kind:ChiefWorkKindDto::Goal,title:"Chief".into(),codex_thread_id:Some("thread".into()),active_turn_id:None,dispatch_state:ChiefDispatchStateDto::Idle,status:ChiefWorkStatusDto::Open,next_check_at_micros:None,created_at_micros:1,updated_at_micros:1}],pending_events:vec![ChiefPendingEventDto{id:7,source_event_id:"approval".into(),work_item_id:"root".into(),event_kind:"server_request_pending".into(),created_at_micros:1,delivery_claimed:false}]})));
  s.profile=Some(profile);
  s.request=Some(ChiefRequestResult::Available {event_id:7,work_id:"root".into(),method:"mcpServer/elicitation/request".into(),request_json:decodex_protocol::ChiefRequestText::new(json!({"serverName":"codex_apps","mode":"form","message":"Review","requestedSchema":{"type":"object","properties":{}},"_meta":{"connector_id":"calendar","link_id":"work"}}).to_string()).unwrap()});
 });
	visual.update(|w, cx| {
		w.resize(gpui::size(px(1180.0), px(1800.0)));
		w.draw(cx).clear();
	});
	let button = visual.debug_bounds("account-settings-read").unwrap();
	visual.simulate_click(button.center(), Default::default());
	visual.run_until_parked();
	visual.update(|w, cx| {
		w.draw(cx).clear();
	});
	let button = visual.debug_bounds("account-mode-auto").unwrap();
	visual.simulate_click(button.center(), Default::default());
	visual.run_until_parked();
	assert_eq!(server.join().unwrap().len(), 1);
	surface.read_with(visual, |s, _| {
		assert!(s.app_settings.task.is_none());
		assert!(!s.app_settings.reviewed);
		assert!(s.app_settings.feedback.contains("may have been saved"));
		assert!(
			matches!(&s.app_settings.state,Some(State::Available {user_mode:Some(mode),..}) if mode=="auto")
		);
		assert!(
			matches!(s.request, Some(ChiefRequestResult::Available { event_id: 7, .. })),
			"settings do not answer approval"
		);
	});
	visual.update(|w, cx| {
		w.draw(cx).clear();
	});
	assert!(
		visual.debug_bounds("account-mode-auto").is_none(),
		"post-write readback is not a new consent"
	);
}

fn saved_work() -> ChiefWorkItemDto {
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
struct SavedView {
	surface: Entity<ChiefSurface>,
}
impl Render for SavedView {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		self.surface.update(cx, |s, cx| s.saved_app_settings_panel(&saved_work(), cx))
	}
}
#[gpui::test]
fn saved_connection_can_restore_inheritance_without_a_pending_request(
	cx: &mut gpui::TestAppContext,
) {
	let (_dir, profile, server) = fixture(true);
	let (view, visual) = cx.add_window_view(|_, cx| {
		let surface = cx.new(ChiefSurface::new);
		cx.observe(&surface, |_, _, cx| cx.notify()).detach();
		surface.update(cx, |s, _| {
			s.apply_result(Ok(ChiefSnapshotResult::Available(ChiefSnapshotDto {
				runtime_source: Some(EntityId::new("source").unwrap()),
				workspaces: vec![],
				dependencies: vec![],
				work_items: vec![saved_work()],
				pending_events: vec![],
			})));
			s.profile = Some(profile);
		});
		SavedView { surface }
	});
	visual.update(|w, cx| {
		w.resize(gpui::size(px(900.), px(1100.)));
		w.draw(cx).clear();
	});
	for id in ["saved-app-settings-read", "saved-app-edit-0", "saved-app-0-account-mode-inherit"] {
		let button = visual.debug_bounds(id).unwrap();
		visual.simulate_click(button.center(), Default::default());
		visual.run_until_parked();
		visual.update(|w, cx| {
			w.draw(cx).clear();
		});
	}
	assert_eq!(server.join().unwrap().len(), 1);
	assert!(visual.debug_bounds("saved-app-edit-0").is_none());
	view.read_with(visual, |v, cx| v.surface.read_with(cx, |s, _| assert!(s.request.is_none())));
}
