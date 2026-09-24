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
	for (index, target) in ["thread", "thread", "child", "child"].into_iter().enumerate() {
		let mut socket =
			tokio_tungstenite::accept_async(listener.accept().await.expect("accept").0)
				.await
				.expect("socket");
		let _hello = socket.next().await.expect("hello");
		for message in [
			ServerMessage::Welcome(ServerWelcome {
				version: CURRENT_VERSION,
				server_id: ServerId::new(SERVER).expect("server"),
				instance_id: None,
				cursor: Cursor(0),
				reconnect: ReconnectMode::Snapshot,
			}),
			ServerMessage::Snapshot(SnapshotEnvelope {
				version: CURRENT_VERSION,
				server_id: ServerId::new(SERVER).expect("server"),
				cursor: Cursor(0),
				items: vec![],
			}),
		] {
			socket
				.send(Message::Text(serde_json::to_string(&message).expect("serialize").into()))
				.await
				.expect("send");
		}
		let Message::Text(text) = socket.next().await.expect("request").expect("text") else {
			panic!("query")
		};
		let ClientMessage::Query(query) = serde_json::from_str(&text).expect("query") else {
			panic!("read only")
		};
		let QueryPayload::GetChiefNativeGoal { work_id, thread_id } = query.payload else {
			panic!("native goal query")
		};
		assert_eq!(work_id.as_str(), "root");
		assert_eq!(thread_id.as_str(), target);
		let returned = if index == 3 { "wrong-thread" } else { target };
		let goal = (index != 1).then(|| decodex_protocol::ChiefNativeGoal {
			thread_id: returned.into(),
			objective: format!("{target} objective"),
			objective_truncated: false,
			status: decodex_protocol::ChiefNativeGoalStatus::BudgetLimited,
			token_budget: Some(11),
			tokens_used: 12,
			time_used_seconds: 7,
			created_at: 1,
			updated_at: 2,
		});
		let message = ServerMessage::QueryResult(QueryResultEnvelope {
			version: CURRENT_VERSION,
			server_id: ServerId::new(SERVER).expect("server"),
			query_id: query.query_id,
			payload: QueryResultPayload::ChiefNativeGoal(Result::Available {
				work_id,
				thread_id: EntityId::new(target).expect("thread"),
				observed_at_micros: 1_000_000,
				goal,
			}),
		});
		socket
			.send(Message::Text(serde_json::to_string(&message).expect("serialize").into()))
			.await
			.expect("reply");
	}
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

fn snapshot() -> ChiefSnapshotDto {
	ChiefSnapshotDto {
		runtime_source: Some(EntityId::new("source").expect("source")),
		workspaces: vec![],
		work_items: vec![work()],
		dependencies: vec![],
		pending_events: vec![],
	}
}
struct GoalView {
	surface: Entity<ChiefSurface>,
}
impl Render for GoalView {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		self.surface.update(cx, |s, cx| s.native_goal_panel(cx))
	}
}
#[gpui::test]
fn native_goal_panel_reads_refreshes_and_switches_exact_child(cx: &mut gpui::TestAppContext) {
	let (_dir, profile, server) = fixture();
	let (view, visual) = cx.add_window_view(|_, cx| {
		let surface = cx.new(ChiefSurface::new);
		cx.observe(&surface, |_, _, cx| cx.notify()).detach();
		surface.update(cx, |s, _| {
			s.apply_result(Ok(ChiefSnapshotResult::Available(snapshot())));
			s.profile = Some(profile);
			s.selected = Some("root".into());
		});
		GoalView { surface }
	});
	visual.update(|w, cx| {
		w.resize(gpui::size(px(900.), px(600.)));
		w.draw(cx).clear();
	});
	let button = visual.debug_bounds("native-goal-read").expect("goal disclosure");
	visual.simulate_click(button.center(), Default::default());
	visual.run_until_parked();
	let surface = view.read_with(visual, |v, _| v.surface.clone());
	surface.update(visual, |s, cx| {
		let result = s.native_goal.result.as_ref().unwrap();
		assert!(goal_text(result).contains("Budget limited"));
		assert!(goal_text(result).contains("Goal tokens used: 12"));
		assert!(goal_text(result).contains("Goal elapsed: 7 seconds"));
		s.native_goal.read_at = None;
		s.refresh_native_goal(cx);
	});
	visual.run_until_parked();
	surface.update(visual, |s, cx| {
		assert!(matches!(s.native_goal.result, Some(Result::Available { goal: None, .. })));
		let profile = s.profile.take();
		s.open_page("root", cx);
		s.profile = profile;
		s.native_agents.selected = Some(("root".into(), "child".into()));
		assert!(s.native_goal.result.is_none());
		s.load_native_goal(cx);
	});
	visual.run_until_parked();
	surface.update(visual,|s,cx| {
  assert!(matches!(&s.native_goal.result,Some(Result::Available{thread_id,..}) if thread_id.as_str()=="child"));
  s.load_native_goal(cx);
 });
	visual.run_until_parked();
	surface.read_with(visual, |s, _| assert_eq!(s.native_goal.result, Some(Result::Unavailable)));
	server.join().unwrap();
}
#[gpui::test]
fn native_goal_observation_does_not_return_after_source_or_thread_restoration(
	cx: &mut gpui::TestAppContext,
) {
	let surface = cx.new(ChiefSurface::new);
	for change in ["source", "thread", "removed", "disconnect"] {
		surface.update(cx, |s, _| {
			s.apply_result(Ok(ChiefSnapshotResult::Available(snapshot())));
			s.selected = Some("root".into());
			s.native_goal.target = Some(("root".into(), "thread".into()));
			s.native_goal.result = Some(Result::Disabled);
			let mut next = snapshot();
			match change {
				"source" => next.runtime_source = Some(EntityId::new("other").unwrap()),
				"thread" => next.work_items[0].codex_thread_id = Some("other".into()),
				"removed" => next.work_items.clear(),
				_ => {},
			}
			if change == "disconnect" {
				s.apply_result(Err(()));
			} else {
				s.apply_result(Ok(ChiefSnapshotResult::Available(next)));
			}
			s.apply_result(Ok(ChiefSnapshotResult::Available(snapshot())));
			assert!(s.native_goal.result.is_none(), "{change}");
			assert!(s.native_goal.target.is_none());
		});
	}
}
