//! Lost send replies are reconciled through read-only receipts, with one submission.
use super::*;
use decodex_protocol::*;
use futures_util::{SinkExt, StreamExt};
use std::os::unix::fs::PermissionsExt;
use tokio_tungstenite::tungstenite::Message;
const SERVER: &str = "018f0f9e-7b6e-4a31-8f4c-1d2e3f405162";
struct View {
	surface: Entity<ChiefSurface>,
	work: String,
}
impl Render for View {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		self.surface.update(cx, |s, cx| s.prompt_edit_panel(&self.work, cx))
	}
}

#[gpui::test]
fn prompt_send_lost_reply_uses_readback_without_replay(cx: &mut gpui::TestAppContext) {
	cx.background_executor.allow_parking();
	let (service, profile, _) = super::super::tests::profiles();
	let directory = tempfile::tempdir().unwrap();
	let store =
		ClientDraftStore::open_at(&directory.path().canonicalize().unwrap().join("desktop"))
			.unwrap();
	let socket_path = service.path().join("server/decodex.sock");
	let listener = std::os::unix::net::UnixListener::bind(&socket_path).unwrap();
	std::fs::set_permissions(socket_path, std::fs::Permissions::from_mode(0o600)).unwrap();
	listener.set_nonblocking(true).unwrap();
	let scope = profile.draft_scope_key();
	let inspect = store.clone();
	let inspect_scope = scope.clone();
	let (unknown, unknown_reply) = std::sync::mpsc::channel();
	let server = spawn_server(listener, inspect, inspect_scope, unknown);
	let (view, visual) = cx.add_window_view(|_, cx| {
		let surface = cx.new(ChiefSurface::new);
		cx.observe(&surface, |_, _, cx| cx.notify()).detach();
		let work = surface.update(cx, |s, cx| {
			s.draft_profiles.storage = Storage::open(Ok(store.clone()));
			s.bind_profile(Some(profile.clone()), cx);
			s.poll_task = None;
			s.visual_workspace_fixture(cx);
			s.state = LoadState::Ready;
			let work = s.selected.clone().unwrap();
			s.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|item| item.id == work)
				.unwrap()
				.codex_thread_id = Some("thread".into());
			s.composer.update(cx, |input, cx| input.set_content("Unrelated main input", cx));
			let input = PromptDraft::new(vec![
				serde_json::json!({"type":"text","text":"Edited input"}),
				serde_json::json!({"type":"image","fileId":"retained-file","detail":"original"}),
			])
			.unwrap();
			s.stage_prompt_editor(
				DesktopPromptEditDraft {
					work_id: EntityId::new(&work).unwrap(),
					thread_id: WireText::new("thread").unwrap(),
					before_turn_id: WireText::new("turn").unwrap(),
					item_id: WireText::new("item").unwrap(),
					original_hash: input.fingerprint().unwrap(),
					review_token: WireText::new("a".repeat(64)).unwrap(),
					receipt_id: Some(10),
					confirmation_key: None,
					pending_send: None,
					handback_pending: false,
					input,
				},
				cx,
			)
			.unwrap();
			work
		});
		View { surface, work }
	});
	let surface = view.read_with(visual, |v, _| v.surface.clone());
	for button in [
		"saved-prompt-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
		"prompt-send",
	] {
		visual.run_until_parked();
		visual.update(|window, cx| {
			window.resize(gpui::size(px(1000.), px(900.)));
			window.draw(cx).clear();
		});
		let bounds = visual.debug_bounds(button).expect("send control");
		visual.simulate_click(bounds.center(), Default::default());
	}
	let mut check_receipt = false;
	for _ in 0..400 {
		visual.run_until_parked();
		visual.executor().advance_clock(std::time::Duration::from_millis(20));
		check_receipt |= unknown_reply.try_recv().is_ok();
		let settled = surface.read_with(visual, |s, _| {
			!s.draft_profiles.storage.document.profiles[&scope]
				.prompt_edits
				.contains_key(&"a".repeat(64))
		});
		if settled {
			break;
		}
		if check_receipt {
			visual.update(|window, cx| {
				window.draw(cx).clear();
			});
			if let Some(bounds) = visual.debug_bounds("prompt-send") {
				visual.simulate_click(bounds.center(), Default::default());
			}
		}
		std::thread::sleep(std::time::Duration::from_millis(5));
	}
	server.join().unwrap();
	visual.run_until_parked();
	let disk = DesktopDraftDocument::decode(&store.load().unwrap().payload).unwrap();
	assert!(!disk.profiles[&scope].prompt_edits.contains_key(&"a".repeat(64)));
	assert_eq!(disk.profiles[&scope].composer.text, "Unrelated main input");
	assert_eq!(
		disk.recovered.last().unwrap().draft.prompt_edits[&"a".repeat(64)].input.parts()[1]["fileId"],
		"retained-file"
	);
}

fn spawn_server(
	listener: std::os::unix::net::UnixListener,
	inspect: ClientDraftStore,
	inspect_scope: String,
	unknown: std::sync::mpsc::Sender<()>,
) -> std::thread::JoinHandle<()> {
	std::thread::spawn(move || {
		tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(
			async {
				let listener = tokio::net::UnixListener::from_std(listener).unwrap();
				tokio::time::timeout(std::time::Duration::from_secs(10), async {
					let mut submitted = None;
					for step in 0..5 {
						let mut socket =
							tokio_tungstenite::accept_async(listener.accept().await.unwrap().0)
								.await
								.unwrap();
						welcome(&mut socket).await;
						let Message::Text(text) = socket.next().await.unwrap().unwrap() else {
							panic!("request")
						};
						let request: ClientMessage = serde_json::from_str(&text).unwrap();
						match request {
							ClientMessage::Command(command) => {
								assert_eq!(step, 2, "only one model-input submission is permitted");
								let CommandPayload::Chief { action } = command.payload else {
									panic!("Chief command")
								};
								let ChiefActionDto::SendPromptInput {
									work_id,
									thread_id,
									input_id,
									edit_receipt_id,
									sha256,
									execution,
								} = *action
								else {
									panic!("send action")
								};
								let identity = PromptInputSendIdentity {
									work_id,
									thread_id,
									edit_receipt_id,
									send: PromptInputSend {
										input_id,
										sha256,
										execution,
										command_key: command.idempotency_key,
									},
								};
								let disk =
									DesktopDraftDocument::decode(&inspect.load().unwrap().payload)
										.unwrap();
								let draft =
									&disk.profiles[&inspect_scope].prompt_edits[&"a".repeat(64)];
								assert_eq!(draft.send_identity().unwrap(), identity);
								assert_eq!(
									disk.profiles[&inspect_scope].composer.text,
									"Unrelated main input"
								);
								submitted = Some(identity);
								// Lose the reply after simulated durable acceptance.
							},
							ClientMessage::Query(query) => {
								let payload = match query.payload {
									QueryPayload::GetChiefModelSettings { work_id }
										if step == 0 =>
										QueryResultPayload::ChiefModelSettings(
											ChiefModelSettingsResult::Available {
												work_id,
												thread_id: EntityId::new("thread").unwrap(),
												account_id: EntityId::new("account").unwrap(),
												model_provider: None,
												model: Some(WireText::new("model").unwrap()),
												reasoning_effort: None,
											},
										),
									QueryPayload::GetChiefPromptInputUpload { upload }
										if step == 1 =>
										QueryResultPayload::ChiefPromptInputUpload(
											PromptInputUploadStatus::Ready { upload, input_id: 7 },
										),
									QueryPayload::GetChiefPromptInputSend { identity }
										if step == 3 || step == 4 =>
									{
										assert_eq!(Some(&identity), submitted.as_ref());
										QueryResultPayload::ChiefPromptInputSend(
											PromptInputSendStatus {
												identity,
												accepted_event_id: (step == 4).then_some(42),
											},
										)
									},
									_ => panic!("unexpected query at step {step}"),
								};
								let response = ServerMessage::QueryResult(QueryResultEnvelope {
									version: CURRENT_VERSION,
									server_id: ServerId::new(SERVER).unwrap(),
									query_id: query.query_id,
									payload,
								});
								socket
									.send(Message::Text(
										serde_json::to_string(&response).unwrap().into(),
									))
									.await
									.unwrap();
								if step == 3 {
									unknown.send(()).unwrap();
								}
							},
							_ => panic!("unexpected request"),
						}
					}
				})
				.await
				.unwrap();
			},
		);
	})
}

async fn welcome(socket: &mut tokio_tungstenite::WebSocketStream<tokio::net::UnixStream>) {
	let _ = socket.next().await;
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
		socket.send(Message::Text(serde_json::to_string(&message).unwrap().into())).await.unwrap();
	}
}
