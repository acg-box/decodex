//! Recap transport tests use a synthetic same-UID socket, not a model provider.
use std::{
	fs::{self, Permissions},
	os::unix::fs::{MetadataExt as _, PermissionsExt as _},
	thread::{self, JoinHandle},
	time::Duration,
};

use futures_util::{SinkExt as _, StreamExt as _};
use gpui::TestAppContext;
use tempfile::TempDir;
use tokio::{runtime::Builder, time};
use tokio_tungstenite::tungstenite::Message;

use crate::shell::agent_surface::recap::*;
use decodex_protocol::{
	CURRENT_VERSION, ClientMessage, CommandError, CommandOutcome, CommandPayload, CommandReceipt,
	CommandResultEnvelope, Cursor, QueryPayload, QueryResultEnvelope, QueryResultPayload,
	ReceiptDisposition, ReconnectMode, ServerId, ServerMessage, ServerWelcome, SnapshotEnvelope,
	TaskRecap,
};

const SERVER: &str = "018f0f9e-7b6e-4a31-8f4c-1d2e3f405162";

struct RecapPanel(Entity<AgentSurface>);
impl Render for RecapPanel {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		self.0.update(cx, |s, cx| {
			let work = s.selected.clone().unwrap();

			s.recap_panel(&work, cx)
		})
	}
}

fn fixture(
	generate: bool,
	rejection: Option<&'static str>,
) -> (TempDir, ClientProfile, JoinHandle<Vec<AgentActionDto>>) {
	let root = tempfile::tempdir_in("/tmp").unwrap();
	let path = root.path().canonicalize().unwrap();
	let server = path.join("server");

	fs::create_dir(&server).unwrap();
	fs::set_permissions(&server, Permissions::from_mode(0o700)).unwrap();

	let uid = fs::metadata(&path).unwrap().uid();
	let config = path.join("config.toml");

	fs::write(&config,format!("version = 1\nactive_profile = \"local\"\ncache = {{}}\n[profiles.local]\nkind = \"local\"\npolicy = \"same_uid\"\nservice_owner_uid = {uid}\nexpected_server_identity = \"{SERVER}\"\n")).unwrap();
	fs::set_permissions(config, Permissions::from_mode(0o600)).unwrap();

	let socket_path = server.join("decodex.sock");
	let listener = std::os::unix::net::UnixListener::bind(&socket_path).unwrap();

	fs::set_permissions(socket_path, Permissions::from_mode(0o600)).unwrap();

	listener.set_nonblocking(true).unwrap();

	let profile = ClientProfile::load(&path, None).unwrap();
	let thread = thread::spawn(move || {
		let runtime = Builder::new_current_thread().enable_all().build().unwrap();

		runtime.block_on(async {
			let listener = tokio::net::UnixListener::from_std(listener).unwrap();

			time::timeout(Duration::from_secs(15), serve(listener, generate, rejection))
				.await
				.unwrap()
		})
	});

	(root, profile, thread)
}

#[test]
fn lost_recap_reply_is_read_back_and_cancelled_by_exact_request_without_replay() {
	let (_root, profile, server) = fixture(true, None);
	let runtime = Builder::new_current_thread().enable_all().build().unwrap();

	runtime.block_on(async {
		let (cancel, cancellation) = watch::channel(false);
		let (updates, mut results) = watch::channel(None);
		let worker = tokio::spawn(request::run(
			profile,
			EntityId::new("root").unwrap(),
			WireText::new("native-root").unwrap(),
			true,
			cancellation,
			updates,
		));

		time::timeout(Duration::from_secs(10), results.changed()).await.unwrap().unwrap();

		assert_eq!(results.borrow().as_ref().unwrap().0.as_ref().unwrap().phase, Phase::Pending);
		// Closing the panel drops its sender while the native request is pending.
		drop(cancel);

		time::timeout(Duration::from_secs(10), worker).await.unwrap().unwrap();

		assert!(results.borrow().as_ref().unwrap().0.is_none());
	});

	assert_eq!(server.join().unwrap().len(), 2);
}

#[gpui::test]
fn recap_renders_plain_result_and_hides_it_after_source_changes(cx: &mut TestAppContext) {
	let (view, visual) = cx.add_window_view(|_, cx| {
		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);

			let work = s.selected.clone().unwrap();

			s.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|w| w.id == work)
				.unwrap()
				.codex_thread_id = Some("thread".into());
			s.recap.work = Some(work.clone());
			s.recap.state = Some(TaskRecapStatus {
				work_id: EntityId::new(work).unwrap(),
				thread_id: Some(WireText::new("thread").unwrap()),
				request_id: Some(WireText::new("request").unwrap()),
				phase: Phase::Ready,
				recap: Some(TaskRecap {
					summary: WireText::new("First batch complete.").unwrap(),
					next_action: None,
				}),
			});
		});

		RecapPanel(surface)
	});

	visual.update(|w, cx| {
		w.resize(gpui::size(px(600.), px(500.)));
		w.draw(cx).clear();
	});

	assert!(visual.debug_bounds("recap-generate").is_some());

	let surface = view.read_with(visual, |v, _| v.0.clone());

	surface.update(visual, |s, _| {
		let mut next = s.snapshot.clone().unwrap();

		next.runtime_source = Some(EntityId::new("replacement").unwrap());

		let (cancel, receiver) = watch::channel(false);

		s.recap.cancel = Some(cancel);

		s.invalidate_recap(&next);

		assert!(s.recap.state.is_none());
		assert!(receiver.has_changed().is_err());
	});

	visual.update(|w, cx| {
		w.draw(cx).clear();
	});

	assert!(visual.debug_bounds("recap-generate").is_none());
}

#[test]
fn opening_a_cold_recap_only_reads_and_does_not_start_inference() {
	let (_root, profile, server) = fixture(false, None);
	let runtime = Builder::new_current_thread().enable_all().build().unwrap();

	runtime.block_on(async {
		let (_cancel, cancellation) = watch::channel(false);
		let (updates, results) = watch::channel(None);

		request::run(
			profile,
			EntityId::new("root").unwrap(),
			WireText::new("native-root").unwrap(),
			false,
			cancellation,
			updates,
		)
		.await;

		assert_eq!(results.borrow().as_ref().unwrap().0.as_ref().unwrap().phase, Phase::Idle);
	});

	assert!(server.join().unwrap().is_empty());
}

#[test]
fn active_voice_rejection_is_shown_without_retrying_generation() {
	let message = "Finish the voice conversation before generating a recap";
	let (_root, profile, server) = fixture(true, Some(message));
	let runtime = Builder::new_current_thread().enable_all().build().unwrap();

	runtime.block_on(async {
		let (_cancel, cancellation) = watch::channel(false);
		let (updates, results) = watch::channel(None);

		time::timeout(
			Duration::from_secs(5),
			request::run(
				profile,
				EntityId::new("root").unwrap(),
				WireText::new("native-root").unwrap(),
				true,
				cancellation,
				updates,
			),
		)
		.await
		.expect("bounded rejection feedback");

		let result = results.borrow();
		let (state, feedback) = result.as_ref().unwrap();

		assert!(state.is_none());
		assert_eq!(feedback, message);
	});

	server.join().unwrap();
}

#[gpui::test]
fn recap_opens_outside_transcript_and_reports_a_missing_connection(cx: &mut TestAppContext) {
	let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

	visual.simulate_resize(gpui::size(px(1_400.), px(900.)));
	surface.update(visual, |s, cx| {
		s.visual_workspace_fixture(cx);
		cx.notify();
	});
	visual.update(|window, cx| window.draw(cx).clear());

	thread::sleep(Duration::from_millis(240));

	visual.update(|window, cx| window.draw(cx).clear());

	assert!(visual.debug_bounds("recap-toggle").is_none(), "no recap row in the transcript");

	let scroll = surface.read_with(visual, |s, _| s.transcript_scroll["agent"].clone());
	let height = scroll.max_offset();

	surface.update(visual, |s, cx| {
		s.details_visible = true;

		s.open_recap("agent", cx);

		assert_eq!(s.recap.work.as_deref(), Some("agent"));
		assert!(s.recap.feedback.contains("Connect to the service"));
	});

	visual.update(|window, cx| window.draw(cx).clear());

	thread::sleep(Duration::from_millis(240));

	visual.update(|window, cx| window.draw(cx).clear());

	let panel = visual.debug_bounds("work-inspection-scroll").unwrap();
	let recap = visual.debug_bounds("recap-toggle").unwrap();

	assert!(panel.contains(&recap.center()));
	assert_eq!(scroll.max_offset(), height, "recap must not resize history");
	assert!(visual.debug_bounds("recap-refresh").is_some());
}

async fn serve(
	listener: tokio::net::UnixListener,
	generate: bool,
	rejection: Option<&str>,
) -> Vec<AgentActionDto> {
	let mut actions = Vec::new();
	let mut request_id = None;

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

		let Message::Text(text) = socket.next().await.unwrap().unwrap() else {
			panic!("text request")
		};

		match serde_json::from_str::<ClientMessage>(&text).unwrap() {
			ClientMessage::Command(command) => {
				if let Some(message) = rejection {
					let receipt = ServerMessage::CommandReceipt(CommandReceipt {
						version: CURRENT_VERSION,
						server_id: ServerId::new(SERVER).unwrap(),
						client_command_id: command.client_command_id.clone(),
						idempotency_key: command.idempotency_key.clone(),
						disposition: ReceiptDisposition::Executed,
						original_client_command_id: command.client_command_id.clone(),
					});

					socket
						.send(Message::Text(serde_json::to_string(&receipt).unwrap().into()))
						.await
						.unwrap();

					let result = ServerMessage::CommandResult(CommandResultEnvelope {
						version: CURRENT_VERSION,
						server_id: ServerId::new(SERVER).unwrap(),
						client_command_id: command.client_command_id,
						idempotency_key: command.idempotency_key,
						outcome: CommandOutcome::Rejected,
						entity_revision: None,
						payload: None,
						error: Some(CommandError::ApplicationUnavailable {
							message: WireText::new(message).unwrap(),
						}),
					});

					socket
						.send(Message::Text(serde_json::to_string(&result).unwrap().into()))
						.await
						.unwrap();

					let _ = socket.next().await;
					let _ = socket.close(None).await;

					return actions;
				}

				let CommandPayload::Agent { action } = command.payload else {
					panic!("Agent command")
				};
				let done = match &*action {
					AgentActionDto::GenerateRecap { work_id, thread_id } => {
						assert_eq!(work_id.as_str(), "root");
						assert_eq!(thread_id.as_str(), "native-root");
						assert!(request_id.is_none(), "Generation must never be replayed");

						request_id = Some(WireText::new(command.idempotency_key.as_str()).unwrap());

						false
					},
					AgentActionDto::CancelRecap { work_id, request_id: cancelled } => {
						assert_eq!(work_id.as_str(), "root");
						assert_eq!(Some(cancelled), request_id.as_ref());

						true
					},
					_ => panic!("Unexpected command"),
				};

				actions.push(*action);
				// Drop both command replies. There must be no mutation replay.
				socket.close(None).await.unwrap();

				if done {
					return actions;
				}
			},
			ClientMessage::Query(query) => {
				let QueryPayload::GetAgentRecap { work_id } = query.payload else {
					panic!("recap query")
				};
				let state = TaskRecapStatus {
					work_id,
					thread_id: generate.then(|| WireText::new("native-root").unwrap()),
					request_id: request_id.clone(),
					phase: if generate { Phase::Pending } else { Phase::Idle },
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

				if !generate {
					return actions;
				}
			},
			_ => panic!("Unexpected message"),
		}
	}
}
