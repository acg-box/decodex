//! Permission clicks cross the public socket; lost replies trigger reads, not retries.
use super::*;
use decodex_protocol::{
	CURRENT_VERSION, ClientMessage, CommandPayload, Cursor, QueryPayload, QueryResultEnvelope,
	QueryResultPayload, ReconnectMode, ServerId, ServerMessage, ServerWelcome, SnapshotEnvelope,
};
use futures_util::{SinkExt, StreamExt};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use tokio_tungstenite::tungstenite::Message;
const SERVER: &str = "018f0f9e-7b6e-4a31-8f4c-1d2e3f405162";
fn fixture() -> (tempfile::TempDir, ClientProfile, std::thread::JoinHandle<Vec<ChiefActionDto>>) {
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

async fn serve(listener: tokio::net::UnixListener) -> Vec<ChiefActionDto> {
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
			let ChiefActionDto::SelectPermissions { work_id, thread_id, review_token, profile_id } =
				&*action
			else {
				panic!("account setting")
			};
			assert_eq!(work_id.as_str(), "root");
			assert_eq!(thread_id.as_str(), "thread");
			assert_eq!(review_token.as_str(), "a".repeat(64));
			assert_eq!(profile_id.as_str(), "scoped");
			actions.push(*action);
			// Lose the response after dispatch. The client must read, never resend.
			socket.close(None).await.unwrap();
			continue;
		}
		let ClientMessage::Query(query) = request else { panic!("settings read") };
		let QueryPayload::GetChiefPermissionProfiles { work_id } = query.payload else {
			panic!("account query")
		};
		assert_eq!(work_id.as_str(), "root");
		let state = if index == 0 {
			available()
		} else {
			State::Pending { profile_id: WireText::new("scoped").unwrap(), state: Outcome::Unknown }
		};

		let result = ServerMessage::QueryResult(QueryResultEnvelope {
			version: CURRENT_VERSION,
			server_id: ServerId::new(SERVER).unwrap(),
			query_id: query.query_id,
			payload: QueryResultPayload::ChiefPermissionProfiles(state),
		});
		socket.send(Message::Text(serde_json::to_string(&result).unwrap().into())).await.unwrap();
	}
	actions
}

fn available() -> State {
	State::Available {
		work_id: EntityId::new("root").unwrap(),
		thread_id: EntityId::new("thread").unwrap(),
		review_token: WireText::new("a".repeat(64)).unwrap(),
		cwd: WireText::new("/native").unwrap(),
		profile_id: Some(WireText::new("readonly").unwrap()),
		approvals_reviewer: WireText::new("user").unwrap(),
		profiles: vec![
			decodex_protocol::ChiefPermissionProfile {
				id: WireText::new("scoped").unwrap(),
				allowed: true,
				can_select: true,
				description: None,
			},
			decodex_protocol::ChiefPermissionProfile {
				id: WireText::new(":full-access").unwrap(),
				allowed: false,
				can_select: false,
				description: None,
			},
		],
		can_update: true,
		last_outcome: None,
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
struct PermissionView {
	surface: Entity<ChiefSurface>,
}
impl Render for PermissionView {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		self.surface.update(cx, |s, cx| s.permission_profiles_panel(&work(), cx))
	}
}
#[gpui::test]
fn permission_click_sends_once_and_retains_unknown_after_lost_reply(cx: &mut gpui::TestAppContext) {
	let (_dir, profile, server) = fixture();
	let (view, visual) = cx.add_window_view(|_, cx| {
		let surface = cx.new(ChiefSurface::new);
		cx.observe(&surface, |_, _, cx| cx.notify()).detach();
		surface.update(cx, |s, cx| {
			s.composer.update(cx, |input, cx| input.set_content("Keep this draft", cx));
			s.apply_result(Ok(ChiefSnapshotResult::Available(snapshot())));
			s.profile = Some(profile);
		});
		PermissionView { surface }
	});
	visual.update(|w, cx| {
		w.resize(gpui::size(px(900.), px(700.)));
		w.draw(cx).clear();
	});
	let button = visual.debug_bounds("permission-profiles-read").unwrap();
	visual.simulate_click(button.center(), Default::default());
	visual.run_until_parked();
	visual.update(|w, cx| {
		w.draw(cx).clear();
	});
	assert!(
		visual.debug_bounds("permission-profile-1").is_none(),
		"policy-disabled profile must have no action"
	);
	let button = visual.debug_bounds("permission-profile-0").unwrap();
	visual.simulate_click(button.center(), Default::default());
	visual.run_until_parked();
	assert_eq!(server.join().unwrap().len(), 1);
	let surface = view.read_with(visual, |v, _| v.surface.clone());
	surface.read_with(visual, |s, cx| {
		assert_eq!(s.composer.read(cx).content(), "Keep this draft");
		assert!(s.permission_profiles.task.is_none());
		assert!(!s.permission_profiles.reviewed);
		assert!(s.permission_profiles.feedback.contains("could not be confirmed"));
		assert!(matches!(
			s.permission_profiles.state,
			Some(State::Pending { state: Outcome::Unknown, .. })
		));
	});
	visual.update(|w, cx| {
		w.draw(cx).clear();
	});
	assert!(visual.debug_bounds("permission-profile-0").is_none());
	surface.update(visual, |s, _| {
		s.apply_result(Err(()));
		s.apply_result(Err(()));
	});
	surface.read_with(visual, |s, _| {
		assert!(s.permission_profiles.state.is_none());
		assert!(s.permission_profiles.work.is_none());
	});
}
#[gpui::test]
fn permission_review_is_invalidated_on_task_or_source_transition(cx: &mut gpui::TestAppContext) {
	let surface = cx.new(ChiefSurface::new);
	for change in ["thread", "turn", "running", "removed", "source"] {
		surface.update(cx, |s, _| {
			let original = snapshot();
			s.apply_result(Ok(ChiefSnapshotResult::Available(original.clone())));
			s.permission_profiles.work = Some("root".into());
			s.permission_profiles.state = Some(available());
			let epoch = s.permission_profiles.epoch;
			let mut next = original.clone();
			match change {
				"thread" => next.work_items[0].codex_thread_id = Some("other".into()),
				"turn" => next.work_items[0].active_turn_id = Some("other".into()),
				"running" => next.work_items[0].dispatch_state = ChiefDispatchStateDto::Running,
				"removed" => next.work_items.clear(),
				_ => next.runtime_source = Some(EntityId::new("other-source").unwrap()),
			}
			s.apply_result(Ok(ChiefSnapshotResult::Available(next)));
			assert_ne!(s.permission_profiles.epoch, epoch, "{change}");
			s.apply_result(Ok(ChiefSnapshotResult::Available(original)));
			assert!(s.permission_profiles.state.is_none());
		});
	}
}

#[gpui::test]
fn running_permissions_offer_both_named_and_builtin_profiles(cx: &mut gpui::TestAppContext) {
	let (_view, visual) = cx.add_window_view(|_, cx| {
		let surface = cx.new(ChiefSurface::new);
		surface.update(cx, |s, _| {
			let mut snapshot = snapshot();
			snapshot.work_items[0].dispatch_state = ChiefDispatchStateDto::Running;
			snapshot.work_items[0].active_turn_id = Some("active".into());
			s.apply_result(Ok(ChiefSnapshotResult::Available(snapshot)));
			s.permission_profiles.work = Some("root".into());
			let mut state = available();
			if let State::Available { profiles, .. } = &mut state {
				profiles[0].can_select = true;
				profiles[1].id = WireText::new(":workspace").expect("builtin");
				profiles[1].allowed = true;
				profiles[1].can_select = true;
			}
			s.permission_profiles.state = Some(state);
			s.permission_profiles.reviewed = true;
		});
		PermissionView { surface }
	});
	visual.update(|window, cx| {
		window.resize(gpui::size(px(900.), px(700.)));
		window.draw(cx).clear();
	});
	assert!(visual.debug_bounds("permission-profile-0").is_some());
	assert!(visual.debug_bounds("permission-profile-1").is_some());
}

#[gpui::test]
fn child_navigation_and_disconnect_cannot_edit_parent_permissions(cx: &mut gpui::TestAppContext) {
	let surface = cx.new(ChiefSurface::new);
	surface.update(cx, |s, cx| {
		s.apply_result(Ok(ChiefSnapshotResult::Available(snapshot())));
		s.permission_profiles.work = Some("root".into());
		s.permission_profiles.state = Some(available());
		s.permission_profiles.reviewed = true;
		s.open_native_agent("root", "child", cx);
		assert!(!s.permission_profiles.reviewed);
		assert!(s.permission_profiles.state.is_none());
		s.update_permission_profiles("root".into(), Some(WireText::new("scoped").unwrap()), cx);
		assert!(s.permission_profiles.task.is_none());
		s.open_page("root", cx);
		s.apply_result(Ok(ChiefSnapshotResult::Unavailable));
		s.update_permission_profiles("root".into(), None, cx);
		assert!(s.permission_profiles.task.is_none());
	});
}
