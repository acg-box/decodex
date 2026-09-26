//! Read-only progress qualification through the same-UID protocol client.
use super::*;
use decodex_protocol::{
	CURRENT_VERSION, ClientMessage, Cursor, QueryPayload, QueryResultEnvelope, QueryResultPayload,
	ReconnectMode, ServerId, ServerMessage, ServerWelcome, SnapshotEnvelope,
};
use futures_util::{SinkExt, StreamExt};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use tokio_tungstenite::tungstenite::Message;
const SERVER: &str = "018f0f9e-7b6e-4a31-8f4c-1d2e3f405162";

#[test]
fn automatic_progress_uses_exact_completed_turns_across_pages_and_refuses_account_changes() {
	for changed_account in [false, true] {
		let (_root, profile, server) = fixture(changed_account, false);
		let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
		let progress = runtime.block_on(read_progress(profile, "work", "thread"));
		if changed_account {
			assert!(progress.is_none());
		} else {
			assert_eq!(progress.unwrap(), vec!["c", "b", "a"]);
		}
		server.join().unwrap();
	}
}
fn fixture(
	changed_account: bool,
	driver: bool,
) -> (tempfile::TempDir, ClientProfile, std::thread::JoinHandle<usize>) {
	let root = tempfile::tempdir().unwrap();
	let path = root.path().canonicalize().unwrap();
	std::fs::create_dir(path.join("server")).unwrap();
	std::fs::set_permissions(path.join("server"), std::fs::Permissions::from_mode(0o700)).unwrap();
	let uid = std::fs::metadata(&path).unwrap().uid();
	let config = path.join("config.toml");
	std::fs::write(&config,format!("version=1\nactive_profile=\"local\"\ncache={{}}\n[profiles.local]\nkind=\"local\"\npolicy=\"same_uid\"\nservice_owner_uid={uid}\nexpected_server_identity=\"{SERVER}\"\n")).unwrap();
	std::fs::set_permissions(config, std::fs::Permissions::from_mode(0o600)).unwrap();
	let socket = path.join("server/decodex.sock");
	let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
	std::fs::set_permissions(socket, std::fs::Permissions::from_mode(0o600)).unwrap();
	listener.set_nonblocking(true).unwrap();
	let profile = ClientProfile::load(&path, None).unwrap();
	let server = std::thread::spawn(move || {
		let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
		runtime.block_on(async {
			let listener = tokio::net::UnixListener::from_std(listener).unwrap();
			tokio::time::timeout(Duration::from_secs(10), async {
				serve(&listener, changed_account).await;
				if driver { serve_generation(&listener).await } else { 0 }
			})
			.await
			.unwrap()
		})
	});
	(root, profile, server)
}

async fn serve(listener: &tokio::net::UnixListener, changed_account: bool) {
	for page_index in 0..2 {
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
		let Message::Text(text) = socket.next().await.unwrap().unwrap() else { panic!("query") };
		let ClientMessage::Query(query) = serde_json::from_str(&text).unwrap() else {
			panic!("no inference command is allowed")
		};
		let QueryPayload::GetChiefTimeline { work_id, thread_id, cursor } = query.payload else {
			panic!("native timeline query")
		};
		assert_eq!(work_id.as_str(), "work");
		assert_eq!(thread_id.as_str(), "thread");
		assert_eq!(
			cursor.as_ref().map(WireText::as_str),
			if page_index == 0 { None } else { Some("older") }
		);
		let entries = if page_index == 0 {
			vec![
				boundary(20, "b", "completed"),
				boundary(30, "c", "completed"),
				boundary(40, "failed", "failed"),
			]
		} else {
			vec![boundary(10, "a", "completed"), boundary(20, "b", "completed")]
		};
		let result = ServerMessage::QueryResult(QueryResultEnvelope {
			version: CURRENT_VERSION,
			server_id: ServerId::new(SERVER).unwrap(),
			query_id: query.query_id,
			payload: QueryResultPayload::ChiefTimeline(
				decodex_protocol::ChiefTimelineResult::Available {
					work_id,
					account_id: EntityId::new(if page_index == 1 && changed_account {
						"other"
					} else {
						"account"
					})
					.unwrap(),
					page: decodex_protocol::ChiefTimelinePage {
						thread_id: "thread".into(),
						entries,
						next_cursor: (page_index == 0).then(|| "older".into()),
						active_realtime_session_at_page_start: None,
					},
				},
			),
		});
		socket.send(Message::Text(serde_json::to_string(&result).unwrap().into())).await.unwrap();
	}
}
fn boundary(position: u64, turn: &str, status: &str) -> decodex_protocol::ChiefTimelineEntry {
	decodex_protocol::ChiefTimelineEntry {
		position,
		content: decodex_protocol::ChiefTimelineContent::TurnBoundary {
			turn_id: turn.into(),
			completed: true,
			status: Some(status.into()),
			duration_ms: None,
			usage_summary: None,
			error: None,
		},
	}
}
#[test]
fn only_two_new_completed_turns_allow_another_recap() {
	let ids = |values: &[&str]| values.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
	let previous = ids(&["c", "b", "a"]);
	assert!(!has_new_progress(&ids(&["b", "a"]), &[]));
	assert!(has_new_progress(&previous, &[]));
	assert!(!has_new_progress(&previous, &previous));
	assert!(!has_new_progress(&ids(&["d", "c", "b"]), &previous));
	assert!(has_new_progress(&ids(&["e", "d", "c"]), &previous));
}

async fn serve_generation(listener: &tokio::net::UnixListener) -> usize {
	let mut request = None;
	let mut commands = 0;
	loop {
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
		let Message::Text(text) = socket.next().await.unwrap().unwrap() else { panic!("request") };
		match serde_json::from_str::<ClientMessage>(&text).unwrap() {
			ClientMessage::Command(command) => {
				let decodex_protocol::CommandPayload::Chief { action } = command.payload else {
					panic!("recap command")
				};
				commands += 1;
				match *action {
					ChiefActionDto::GenerateRecap { work_id, thread_id } => {
						assert!(request.is_none(), "automatic generation must not repeat");
						assert_eq!(work_id.as_str(), "work");
						assert_eq!(thread_id.as_str(), "thread");
						request = Some(WireText::new(command.idempotency_key.as_str()).unwrap());
					},
					ChiefActionDto::CancelRecap { request_id, .. } => {
						assert_eq!(Some(request_id), request);
						socket.close(None).await.unwrap();
						return commands;
					},
					_ => panic!("unexpected command"),
				}
				socket.close(None).await.unwrap(); // Positive readback must resolve this lost reply.
			},
			ClientMessage::Query(query) => {
				assert!(matches!(query.payload, QueryPayload::GetChiefRecap { .. }));
				let state = TaskRecapStatus {
					work_id: EntityId::new("work").unwrap(),
					thread_id: Some(WireText::new("thread").unwrap()),
					request_id: request.clone(),
					phase: Phase::Pending,
					recap: None,
				};
				let result = ServerMessage::QueryResult(QueryResultEnvelope {
					version: CURRENT_VERSION,
					server_id: ServerId::new(SERVER).unwrap(),
					query_id: query.query_id,
					payload: QueryResultPayload::ChiefRecap(state),
				});
				socket
					.send(Message::Text(serde_json::to_string(&result).unwrap().into()))
					.await
					.unwrap();
			},
			_ => panic!("unexpected message"),
		}
	}
}
struct EmptyView(Entity<ChiefSurface>);
impl Render for EmptyView {
	fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
		div()
	}
}
#[gpui::test]
fn automatic_driver_generates_once_after_progress_and_cancels_exact_request_on_focus(
	cx: &mut gpui::TestAppContext,
) {
	// This fixture uses real socket I/O and the production recap I/O thread.
	cx.background_executor.allow_parking();
	let (_root, profile, server) = fixture(false, true);
	let (view, visual) = cx.add_window_view(|_, cx| EmptyView(cx.new(ChiefSurface::new)));
	let surface = view.read_with(visual, |v, _| v.0.clone());
	surface.update(visual, |s, cx| {
		s.profile = Some(profile);
		s.selected = Some("work".into());
		s.snapshot = Some(ChiefSnapshotDto {
			runtime_source: Some(EntityId::new("runtime").unwrap()),
			workspaces: vec![],
			dependencies: vec![],
			pending_events: vec![],
			work_items: vec![ChiefWorkItemDto {
				id: "work".into(),
				parent_goal_id: None,
				kind: decodex_protocol::ChiefWorkKindDto::Goal,
				title: "Work".into(),
				codex_thread_id: Some("thread".into()),
				active_turn_id: None,
				dispatch_state: ChiefDispatchStateDto::Idle,
				status: decodex_protocol::ChiefWorkStatusDto::Open,
				next_check_at_micros: None,
				created_at_micros: 1,
				updated_at_micros: 1,
			}],
		});
		s.poll_automatic_recap(true, cx);
		s.recap_focus(false);
		s.automatic_recap.away = Some(Instant::now() - DELAY);
		s.automatic_recap.quiet = s.automatic_recap.away;
		s.poll_automatic_recap(true, cx);
	});
	let deadline = Instant::now() + Duration::from_secs(5);
	loop {
		visual.run_until_parked();
		let pending = surface.update(visual, |s, cx| {
			s.poll_automatic_recap(true, cx);
			s.recap.state.as_ref().is_some_and(|v| v.phase == Phase::Pending)
		});
		if pending {
			break;
		}
		assert!(Instant::now() < deadline, "automatic request reached service");
		std::thread::sleep(Duration::from_millis(10));
	}
	surface.update(visual, |s, _| s.recap_focus(true));
	while !server.is_finished() {
		visual.run_until_parked();
		assert!(Instant::now() < deadline, "exact cancellation reached service");
		std::thread::sleep(Duration::from_millis(10));
	}
	assert_eq!(server.join().unwrap(), 2);
}
