//! Recap transport tests use a synthetic same-UID socket, not a model provider.
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
	generate: bool,
	rejection: Option<&'static str>,
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
			tokio::time::timeout(
				std::time::Duration::from_secs(15),
				serve(listener, generate, rejection),
			)
			.await
			.unwrap()
		})
	});
	(root, profile, thread)
}

async fn serve(
	listener: tokio::net::UnixListener,
	generate: bool,
	rejection: Option<&str>,
) -> Vec<ChiefActionDto> {
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
					let receipt = ServerMessage::CommandReceipt(decodex_protocol::CommandReceipt {
						version: CURRENT_VERSION,
						server_id: ServerId::new(SERVER).unwrap(),
						client_command_id: command.client_command_id.clone(),
						idempotency_key: command.idempotency_key.clone(),
						disposition: decodex_protocol::ReceiptDisposition::Executed,
						original_client_command_id: command.client_command_id.clone(),
					});
					socket
						.send(Message::Text(serde_json::to_string(&receipt).unwrap().into()))
						.await
						.unwrap();

					let result =
						ServerMessage::CommandResult(decodex_protocol::CommandResultEnvelope {
							version: CURRENT_VERSION,
							server_id: ServerId::new(SERVER).unwrap(),
							client_command_id: command.client_command_id,
							idempotency_key: command.idempotency_key,
							outcome: decodex_protocol::CommandOutcome::Rejected,
							entity_revision: None,
							payload: None,
							error: Some(decodex_protocol::CommandError::ApplicationUnavailable {
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

				let CommandPayload::Chief { action } = command.payload else {
					panic!("Chief command")
				};
				let done = match &*action {
					ChiefActionDto::GenerateRecap { work_id, thread_id } => {
						assert_eq!(work_id.as_str(), "root");
						assert_eq!(thread_id.as_str(), "native-root");
						assert!(request_id.is_none(), "Generation must never be replayed");
						request_id = Some(WireText::new(command.idempotency_key.as_str()).unwrap());
						false
					},
					ChiefActionDto::CancelRecap { work_id, request_id: cancelled } => {
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
				let QueryPayload::GetChiefRecap { work_id } = query.payload else {
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
					payload: QueryResultPayload::ChiefRecap(state),
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

#[test]
fn lost_recap_reply_is_read_back_and_cancelled_by_exact_request_without_replay() {
	let (_root, profile, server) = fixture(true, None);
	let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
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
		tokio::time::timeout(std::time::Duration::from_secs(10), results.changed())
			.await
			.unwrap()
			.unwrap();
		assert_eq!(results.borrow().as_ref().unwrap().0.as_ref().unwrap().phase, Phase::Pending);
		// Closing the panel drops its sender while the native request is pending.
		drop(cancel);
		tokio::time::timeout(std::time::Duration::from_secs(10), worker).await.unwrap().unwrap();
		assert!(results.borrow().as_ref().unwrap().0.is_none());
	});
	assert_eq!(server.join().unwrap().len(), 2);
}

struct RecapPanel(Entity<ChiefSurface>);
impl Render for RecapPanel {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		self.0.update(cx, |s, cx| {
			let work = s.selected.clone().unwrap();
			s.recap_panel(&work, cx)
		})
	}
}

#[gpui::test]
fn recap_renders_plain_result_and_hides_it_after_source_changes(cx: &mut gpui::TestAppContext) {
	let (view, visual) = cx.add_window_view(|_, cx| {
		let surface = cx.new(ChiefSurface::new);
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
				recap: Some(decodex_protocol::TaskRecap {
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
	let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
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
	let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
	runtime.block_on(async {
		let (_cancel, cancellation) = watch::channel(false);
		let (updates, results) = watch::channel(None);
		tokio::time::timeout(
			std::time::Duration::from_secs(5),
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
