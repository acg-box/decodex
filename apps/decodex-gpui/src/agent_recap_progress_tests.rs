//! Read-only progress qualification through the same-UID protocol client.
use std::{
	fs::{self, Permissions},
	os::unix::fs::{MetadataExt as _, PermissionsExt as _},
	thread::{self, JoinHandle},
};

use futures_util::{SinkExt as _, StreamExt as _};
use gpui::{AppContext as _, TestAppContext};
use tempfile::TempDir;
use tokio::{runtime::Builder, time};
use tokio_tungstenite::tungstenite::Message;

use crate::shell::agent_surface::recap::automatic::{
	self, AgentActionDto, AgentDispatchStateDto, AgentSnapshotDto, AgentSurface, AgentWorkItemDto,
	ClientProfile, Context, DELAY, Duration, Entity, EntityId, Instant, IntoElement, LoadState,
	Render, TaskRecapPhase, TaskRecapStatus, Window, WireText,
};
use decodex_protocol::{
	AgentTimelineContent, AgentTimelinePage, AgentTimelineResult, CURRENT_VERSION, ClientMessage,
	CommandPayload, Cursor, QueryPayload, QueryResultEnvelope, QueryResultPayload, ReconnectMode,
	ServerId, ServerMessage, ServerWelcome, SnapshotEnvelope,
};

const SERVER: &str = "018f0f9e-7b6e-4a31-8f4c-1d2e3f405162";

struct EmptyView(Entity<AgentSurface>);
impl Render for EmptyView {
	fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
		automatic::div()
	}
}

#[test]
fn automatic_progress_uses_exact_completed_turns_across_pages_and_refuses_account_changes() {
	for changed_account in [false, true] {
		let (_root, profile, server) = fixture(changed_account, false);
		let runtime = Builder::new_current_thread().enable_all().build().unwrap();
		let progress = runtime.block_on(automatic::read_progress(profile, "work", "thread"));

		if changed_account {
			assert!(progress.is_none());
		} else {
			assert_eq!(progress.unwrap(), vec!["c", "b", "a"]);
		}

		server.join().unwrap();
	}
}

fn fixture(changed_account: bool, driver: bool) -> (TempDir, ClientProfile, JoinHandle<usize>) {
	let root = tempfile::tempdir().unwrap();
	let path = root.path().canonicalize().unwrap();

	fs::create_dir(path.join("server")).unwrap();
	fs::set_permissions(path.join("server"), Permissions::from_mode(0o700)).unwrap();

	let uid = fs::metadata(&path).unwrap().uid();
	let config = path.join("config.toml");

	fs::write(&config,format!("version=1\nactive_profile=\"local\"\ncache={{}}\n[profiles.local]\nkind=\"local\"\npolicy=\"same_uid\"\nservice_owner_uid={uid}\nexpected_server_identity=\"{SERVER}\"\n")).unwrap();
	fs::set_permissions(config, Permissions::from_mode(0o600)).unwrap();

	let socket = path.join("server/decodex.sock");
	let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();

	fs::set_permissions(socket, Permissions::from_mode(0o600)).unwrap();

	listener.set_nonblocking(true).unwrap();

	let profile = ClientProfile::load(&path, None).unwrap();
	let server = thread::spawn(move || {
		let runtime = Builder::new_current_thread().enable_all().build().unwrap();

		runtime.block_on(async {
			let listener = tokio::net::UnixListener::from_std(listener).unwrap();

			time::timeout(Duration::from_secs(10), async {
				serve(&listener, changed_account).await;

				if driver { serve_generation(&listener).await } else { 0 }
			})
			.await
			.unwrap()
		})
	});

	(root, profile, server)
}

fn boundary(position: u64, turn: &str, status: &str) -> decodex_protocol::AgentTimelineEntry {
	decodex_protocol::AgentTimelineEntry {
		position,
		content: AgentTimelineContent::TurnBoundary {
			turn_id: turn.into(),
			completed: true,
			status: Some(status.into()),
			duration_ms: None,
			usage: None,
			usage_summary: None,
			error: None,
		},
	}
}

#[test]
fn only_two_new_completed_turns_allow_another_recap() {
	let ids = |values: &[&str]| values.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
	let previous = ids(&["c", "b", "a"]);

	assert!(!automatic::has_new_progress(&ids(&["b", "a"]), &[]));
	assert!(automatic::has_new_progress(&previous, &[]));
	assert!(!automatic::has_new_progress(&previous, &previous));
	assert!(!automatic::has_new_progress(&ids(&["d", "c", "b"]), &previous));
	assert!(automatic::has_new_progress(&ids(&["e", "d", "c"]), &previous));
}

#[gpui::test]
fn automatic_driver_generates_once_after_progress_and_cancels_exact_request_on_focus(
	cx: &mut TestAppContext,
) {
	// This fixture uses real socket I/O and the production recap I/O thread.
	cx.background_executor.allow_parking();

	let (_root, profile, server) = fixture(false, true);
	let (view, visual) = cx.add_window_view(|_, cx| EmptyView(cx.new(AgentSurface::new)));
	let surface = view.read_with(visual, |v, _| v.0.clone());

	surface.update(visual, |s, cx| {
		s.profile = Some(profile);
		s.state = LoadState::Ready;
		s.selected = Some("work".into());
		s.snapshot = Some(AgentSnapshotDto {
			context_references: vec![],
			connection_initializing: false,
			runtime_source: Some(EntityId::new("runtime").unwrap()),
			workspaces: vec![],
			dependencies: vec![],
			pending_events: vec![],
			work_items: vec![AgentWorkItemDto {
				id: "work".into(),
				parent_goal_id: None,
				kind: decodex_protocol::AgentWorkKindDto::Goal,
				title: "Work".into(),
				codex_thread_id: Some("thread".into()),
				active_turn_id: None,
				dispatch_state: AgentDispatchStateDto::Idle,
				status: decodex_protocol::AgentWorkStatusDto::Open,
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

			s.recap.state.as_ref().is_some_and(|v| v.phase == TaskRecapPhase::Pending)
		});

		if pending {
			break;
		}

		assert!(Instant::now() < deadline, "automatic request reached service");

		thread::sleep(Duration::from_millis(10));
	}

	surface.update(visual, |s, _| s.recap_focus(true));

	while !server.is_finished() {
		visual.run_until_parked();

		assert!(Instant::now() < deadline, "exact cancellation reached service");

		thread::sleep(Duration::from_millis(10));
	}

	assert_eq!(server.join().unwrap(), 2);
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
		let QueryPayload::GetAgentTimeline { work_id, thread_id, cursor } = query.payload else {
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
			payload: QueryResultPayload::AgentTimeline(AgentTimelineResult::Available {
				work_id,
				account_id: EntityId::new(if page_index == 1 && changed_account {
					"other"
				} else {
					"account"
				})
				.unwrap(),
				page: AgentTimelinePage {
					thread_id: "thread".into(),
					entries,
					next_cursor: (page_index == 0).then(|| "older".into()),
					weather: Default::default(),
					safety_buffering_turn_id: None,
					active_realtime_session_at_page_start: None,
				},
			}),
		});

		socket.send(Message::Text(serde_json::to_string(&result).unwrap().into())).await.unwrap();
	}
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
				let CommandPayload::Agent { action } = command.payload else {
					panic!("recap command")
				};

				commands += 1;

				match *action {
					AgentActionDto::GenerateRecap { work_id, thread_id } => {
						assert!(request.is_none(), "automatic generation must not repeat");
						assert_eq!(work_id.as_str(), "work");
						assert_eq!(thread_id.as_str(), "thread");

						request = Some(WireText::new(command.idempotency_key.as_str()).unwrap());
					},
					AgentActionDto::CancelRecap { request_id, .. } => {
						assert_eq!(Some(request_id), request);

						socket.close(None).await.unwrap();

						return commands;
					},
					_ => panic!("unexpected command"),
				}

				socket.close(None).await.unwrap(); // Positive readback must resolve this lost reply.
			},
			ClientMessage::Query(query) => {
				assert!(matches!(query.payload, QueryPayload::GetAgentRecap { .. }));

				let state = TaskRecapStatus {
					work_id: EntityId::new("work").unwrap(),
					thread_id: Some(WireText::new("thread").unwrap()),
					request_id: request.clone(),
					phase: TaskRecapPhase::Pending,
					recap: None,
				};
				let result = ServerMessage::QueryResult(QueryResultEnvelope {
					version: CURRENT_VERSION,
					server_id: ServerId::new(SERVER).unwrap(),
					query_id: query.query_id,
					payload: QueryResultPayload::AgentRecap(state),
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
