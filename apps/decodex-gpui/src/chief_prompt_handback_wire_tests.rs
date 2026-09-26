//! Handback requires saved input and refreshed presentation before acknowledgement.
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
fn prompt_handback_saves_and_refreshes_before_acknowledgement(cx: &mut gpui::TestAppContext) {
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
	let (acknowledged, acknowledgement) = std::sync::mpsc::channel();
	let (checked, check) = std::sync::mpsc::channel();
	let server = spawn_server(listener, inspect, inspect_scope, acknowledged, check);
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
			let mut original = input.clone();
			original.replace_text(0, 0..6, "Original").unwrap();
			s.stage_prompt_editor(
				DesktopPromptEditDraft {
					work_id: EntityId::new(&work).unwrap(),
					thread_id: WireText::new("thread").unwrap(),
					before_turn_id: WireText::new("turn").unwrap(),
					item_id: WireText::new("item").unwrap(),
					original_hash: original.fingerprint().unwrap(),
					review_token: WireText::new("a".repeat(64)).unwrap(),
					receipt_id: Some(10),
					confirmation_key: None,
					pending_send: None,
					handback_pending: true,
					input,
				},
				cx,
			)
			.unwrap();
			s.older_history.insert(work.clone(), (vec![], Some(99)));
			work
		});
		View { surface, work }
	});
	let surface = view.read_with(visual, |v, _| v.surface.clone());
	for button in [
		"saved-prompt-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
		"prompt-handback",
	] {
		visual.run_until_parked();
		visual.update(|window, cx| {
			window.resize(gpui::size(px(1000.), px(900.)));
			window.draw(cx).clear();
		});
		let bounds = visual.debug_bounds(button).expect("send control");
		visual.simulate_click(bounds.center(), Default::default());
	}
	let mut checked_presentation = false;
	for _ in 0..400 {
		visual.run_until_parked();
		visual.executor().advance_clock(std::time::Duration::from_millis(20));
		if acknowledgement.try_recv().is_ok() {
			surface.read_with(visual, |s, _| {
				let work = s.selected.as_ref().unwrap();
				assert!(!s.older_history.contains_key(work));
				assert!(s.history_cache.contains_key(work));
				assert_eq!(s.native_history.binding.as_ref().unwrap().thread, "thread");
				assert!(s.native_history.entries.is_empty());
			});
			checked_presentation = true;
			checked.send(()).unwrap();
		}
		let disk = DesktopDraftDocument::decode(&store.load().unwrap().payload).unwrap();
		if disk
			.profiles
			.get(&scope)
			.and_then(|p| p.prompt_edits.get(&"a".repeat(64)))
			.is_some_and(|draft| !draft.handback_pending)
		{
			break;
		}
		std::thread::sleep(std::time::Duration::from_millis(5));
	}
	server.join().unwrap();
	assert!(checked_presentation);
	visual.run_until_parked();
	let disk = DesktopDraftDocument::decode(&store.load().unwrap().payload).unwrap();
	let draft = &disk.profiles[&scope].prompt_edits[&"a".repeat(64)];
	assert!(!draft.handback_pending);
	assert_eq!(draft.receipt_id, Some(10));
	assert!(draft.pending_send.is_none());
	assert_eq!(draft.input.parts()[0]["text"], "Edited input");
	assert_eq!(draft.input.parts()[1]["fileId"], "retained-file");
	assert_eq!(disk.profiles[&scope].composer.text, "Unrelated main input");
}

fn spawn_server(
	listener: std::os::unix::net::UnixListener,
	inspect: ClientDraftStore,
	inspect_scope: String,
	acknowledged: std::sync::mpsc::Sender<()>,
	check: std::sync::mpsc::Receiver<()>,
) -> std::thread::JoinHandle<()> {
	std::thread::spawn(move || {
		tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(
			async {
				let listener = tokio::net::UnixListener::from_std(listener).unwrap();
				tokio::time::timeout(std::time::Duration::from_secs(10), async {

					for step in 0..6 {
						let mut socket =
							tokio_tungstenite::accept_async(listener.accept().await.unwrap().0)
								.await
								.unwrap();
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
							socket
								.send(Message::Text(
									serde_json::to_string(&message).unwrap().into(),
								))
								.await
								.unwrap();
						}
						let Message::Text(text) = socket.next().await.unwrap().unwrap() else {
							panic!("request")
						};
						let request: ClientMessage = serde_json::from_str(&text).unwrap();
						match request {
							ClientMessage::Command(command) => {
								let CommandPayload::Chief { action } = command.payload else { panic!("Chief command") };
								match *action {
									ChiefActionDto::RecoverPromptEdit { .. } if step == 0 => {},
									ChiefActionDto::AcknowledgePromptEditDraft { work_id, thread_id, receipt_id, review_token } if step == 4 => {
										let disk = DesktopDraftDocument::decode(&inspect.load().unwrap().payload).unwrap();
										let draft = &disk.profiles[&inspect_scope].prompt_edits[review_token.as_str()];
										assert_eq!(draft.work_id, work_id);
										assert_eq!(draft.thread_id, thread_id);
										assert_eq!(draft.receipt_id, Some(receipt_id));
										assert!(draft.handback_pending);
										assert_eq!(draft.input.parts()[0]["text"], "Edited input");
										assert_eq!(disk.profiles[&inspect_scope].composer.text, "Unrelated main input");
										acknowledged.send(()).unwrap();
										check.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
									},
									_ => panic!("unexpected mutation at step {step}"),
								}
								// Lose command replies; the next read must establish actual state.
							},
							ClientMessage::Query(query) => {
								let payload = match query.payload {
									QueryPayload::GetChiefPromptEdit { work_id, thread_id, .. } if step == 1 || step == 5 => {
										let fragment = serde_json::json!([{"type":"text","text":"Original input"},{"type":"image","fileId":"retained-file","detail":"original"}]).to_string();
										QueryResultPayload::ChiefPromptEdit(PromptEditStatus { work_id, thread_id, phase: if step == 1 { PromptEditPhase::Applied } else { PromptEditPhase::Restored }, evidence: Some(PromptEditEvidence { review_token: WireText::new("a".repeat(64)).unwrap(), receipt_id: Some(10), before_turn_id: WireText::new("turn").unwrap(), item_id: WireText::new("item").unwrap(), removed_turns: 2, content_bytes: fragment.len() as u64, offset: 0, fragment }) })
									},
									QueryPayload::GetChiefHistory { .. } if step == 2 => QueryResultPayload::ChiefHistory(ChiefHistoryResult::Available { questions: vec![], questions_truncated: false, questions_recovering: false, misalignment: None, usage: None, entries: vec![], has_more: false, next_before: None, live: vec![] }),
									QueryPayload::GetChiefTimeline { work_id, thread_id, .. } if step == 3 => QueryResultPayload::ChiefTimeline(ChiefTimelineResult::Available { work_id, account_id: EntityId::new("account").unwrap(), page: ChiefTimelinePage { thread_id: thread_id.as_str().into(), entries: vec![], next_cursor: None, active_realtime_session_at_page_start: None } }),
									_ => panic!("unexpected query at step {step}"),
								};
								let response = ServerMessage::QueryResult(QueryResultEnvelope { version: CURRENT_VERSION, server_id: ServerId::new(SERVER).unwrap(), query_id: query.query_id, payload });
								socket.send(Message::Text(serde_json::to_string(&response).unwrap().into())).await.unwrap();
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
