//! Rendered current-turn reviewer changes cross the public same-UID socket.
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
			let ChiefActionDto::SetLiveReviewer { work_id, turn_id, review_token, reviewer } =
				&*action
			else {
				panic!("account setting")
			};
			assert_eq!(work_id.as_str(), "root");
			assert_eq!(turn_id.as_str(), "turn");
			assert_eq!(review_token.as_str(), "a".repeat(64));
			assert_eq!(*reviewer, Reviewer::User);
			actions.push(*action);
			// Lose the response after dispatch. The client must read, never resend.
			socket.close(None).await.unwrap();
			continue;
		}
		let ClientMessage::Query(query) = request else { panic!("settings read") };
		let QueryPayload::GetChiefLiveReviewer { work_id } = query.payload else {
			panic!("account query")
		};
		assert_eq!(work_id.as_str(), "root");
		let state = State::Available {
			thread_id: EntityId::new("thread").unwrap(),
			turn_id: EntityId::new("turn").unwrap(),
			review_token: WireText::new(if index == 0 { "a" } else { "b" }.repeat(64)).unwrap(),
			can_update: true,
			last_reviewer: (index != 0).then_some(Reviewer::User),
			last_outcome: (index != 0)
				.then_some(decodex_protocol::ChiefLiveReviewerOutcome::Unknown),
		};
		let result = ServerMessage::QueryResult(QueryResultEnvelope {
			version: CURRENT_VERSION,
			server_id: ServerId::new(SERVER).unwrap(),
			query_id: query.query_id,
			payload: QueryResultPayload::ChiefLiveReviewer(state),
		});
		socket.send(Message::Text(serde_json::to_string(&result).unwrap().into())).await.unwrap();
	}
	actions
}

fn work() -> ChiefWorkItemDto {
	ChiefWorkItemDto {
		id: "root".into(),
		parent_goal_id: None,
		kind: decodex_protocol::ChiefWorkKindDto::Goal,
		title: "Root".into(),
		codex_thread_id: Some("thread".into()),
		active_turn_id: Some("turn".into()),
		dispatch_state: ChiefDispatchStateDto::Running,
		status: ChiefWorkStatusDto::Open,
		next_check_at_micros: None,
		created_at_micros: 1,
		updated_at_micros: 1,
	}
}

#[gpui::test]
fn reviewed_live_turn_is_invalidated_even_if_the_old_identity_returns(
	cx: &mut gpui::TestAppContext,
) {
	let surface = cx.new(ChiefSurface::new);
	for change in ["thread", "turn", "idle", "removed", "source"] {
		let original = ChiefSnapshotDto {
			runtime_source: Some(EntityId::new("source").unwrap()),
			workspaces: vec![],
			work_items: vec![work()],
			dependencies: vec![],
			pending_events: vec![],
		};
		surface.update(cx, |s, _| {
			s.apply_result(Ok(ChiefSnapshotResult::Available(original.clone())));
			s.live_reviewer.work = Some("root".into());
			s.live_reviewer.state = Some(State::Available {
				thread_id: EntityId::new("thread").unwrap(),
				turn_id: EntityId::new("turn").unwrap(),
				review_token: WireText::new("a".repeat(64)).unwrap(),
				can_update: true,
				last_reviewer: None,
				last_outcome: None,
			});
			let before = s.live_reviewer.epoch;
			let mut changed = original.clone();
			match change {
				"thread" => changed.work_items[0].codex_thread_id = Some("other-thread".into()),
				"turn" => changed.work_items[0].active_turn_id = Some("other-turn".into()),
				"idle" => changed.work_items[0].dispatch_state = ChiefDispatchStateDto::Idle,
				"removed" => changed.work_items.clear(),
				_ => changed.runtime_source = Some(EntityId::new("other-source").unwrap()),
			}
			s.apply_result(Ok(ChiefSnapshotResult::Available(changed)));
			assert_ne!(s.live_reviewer.epoch, before, "{change}");
			s.apply_result(Ok(ChiefSnapshotResult::Available(original)));
			assert!(s.live_reviewer.state.is_none(), "old review returned after {change}");
			assert!(s.live_reviewer.work.is_none());
		});
	}
}
struct ReviewerView {
	surface: Entity<ChiefSurface>,
}
impl Render for ReviewerView {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		self.surface.update(cx, |s, cx| s.live_reviewer_panel(&work(), cx))
	}
}
#[gpui::test]
fn reviewer_click_sends_exact_turn_once_and_reads_unknown_receipt(cx: &mut gpui::TestAppContext) {
	let (_dir, profile, server) = fixture();
	let (view, visual) = cx.add_window_view(|_, cx| {
		let surface = cx.new(ChiefSurface::new);
		cx.observe(&surface, |_, _, cx| cx.notify()).detach();
		surface.update(cx, |s, _| {
			s.apply_result(Ok(ChiefSnapshotResult::Available(ChiefSnapshotDto {
				runtime_source: Some(EntityId::new("source").unwrap()),
				workspaces: vec![],
				work_items: vec![work()],
				dependencies: vec![],
				pending_events: vec![],
			})));
			s.profile = Some(profile);
		});
		ReviewerView { surface }
	});
	visual.update(|w, cx| {
		w.resize(gpui::size(px(900.), px(600.)));
		w.draw(cx).clear();
	});
	let button = visual.debug_bounds("live-reviewer-read").unwrap();
	visual.simulate_click(button.center(), Default::default());
	visual.run_until_parked();
	visual.update(|w, cx| {
		w.draw(cx).clear();
	});
	let button = visual.debug_bounds("live-reviewer-user").unwrap();
	visual.simulate_click(button.center(), Default::default());
	visual.run_until_parked();
	assert_eq!(server.join().unwrap().len(), 1);
	let surface = view.read_with(visual, |v, _| v.surface.clone());
	surface.read_with(visual, |s, _| {
		assert!(s.live_reviewer.task.is_none());
		assert!(s.live_reviewer.feedback.contains("could not be confirmed"));
		assert!(matches!(
			s.live_reviewer.state,
			Some(State::Available {
				last_outcome: Some(decodex_protocol::ChiefLiveReviewerOutcome::Unknown),
				..
			})
		));
	});
	surface.update(visual, |s, _| s.app_settings_disconnected());
	surface.read_with(visual, |s, _| {
		assert!(s.live_reviewer.state.is_none());
		assert!(s.live_reviewer.work.is_none());
	});
}
