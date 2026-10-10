//! Confirmation crosses the socket only after the exact local record is durable.
use std::{
	fs::{self, Permissions},
	os::unix::fs::PermissionsExt as _,
	sync::{mpsc, mpsc::Sender},
	thread::{self, JoinHandle},
	time::Duration,
};

use futures_util::{SinkExt as _, StreamExt as _};
#[cfg(test)] use gpui::AppContext as _;
use gpui::TestAppContext;
use tokio::{runtime::Builder, time};
use tokio_tungstenite::tungstenite::Message;

#[cfg(test)]
use crate::shell::agent_surface::drafts::storage::{
	self, AgentActionDto, Entity, IntoElement, LoadState, Render, Window,
};
use crate::shell::agent_surface::drafts::{
	storage::{
		AgentSurface, ClientDraftStore, ClientProfile, Context, DesktopDraftDocument, EntityId,
		Storage, WireText,
	},
	tests,
};
use decodex_protocol::{
	AgentModelSettingsResult, CURRENT_VERSION, ClientMessage, CommandError, CommandOutcome,
	CommandPayload, CommandReceipt, CommandResultEnvelope, Cursor, EntityRevision,
	PromptEditEvidence, PromptEditPhase, PromptEditStatus, PromptForkBoundary, PromptForkIntent,
	QueryPayload, QueryResultEnvelope, QueryResultPayload, ReceiptDisposition, ReconnectMode,
	ResultPayload, ServerId, ServerMessage, ServerWelcome, SnapshotEnvelope,
};

const SERVER: &str = "018f0f9e-7b6e-4a31-8f4c-1d2e3f405162";
struct View {
	surface: Entity<AgentSurface>,
	work: String,
}
impl Render for View {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		self.surface.update(cx, |s, cx| s.prompt_edit_panel(&self.work, cx))
	}
}

#[gpui::test]
fn confirmation_waits_for_disk_and_keeps_the_main_composer(cx: &mut TestAppContext) {
	run_confirmation(cx, None);
}
#[gpui::test]
fn branch_confirmation_saves_choice_before_native_dispatch(cx: &mut TestAppContext) {
	run_confirmation(cx, Some(PromptForkBoundary::BeforeInput));
}
#[gpui::test]
fn branch_after_turn_needs_no_input_preflight(cx: &mut TestAppContext) {
	run_confirmation(cx, Some(PromptForkBoundary::AfterTurn));
}
fn run_confirmation(cx: &mut TestAppContext, boundary: Option<PromptForkBoundary>) {
	cx.background_executor.allow_parking();

	let (service, profile, _) = tests::profiles();
	let directory = tempfile::tempdir().unwrap();
	let store =
		ClientDraftStore::open_at(&directory.path().canonicalize().unwrap().join("desktop"))
			.unwrap();
	let socket_path = service.path().join("server/decodex.sock");
	let listener = std::os::unix::net::UnixListener::bind(&socket_path).unwrap();

	fs::set_permissions(socket_path, Permissions::from_mode(0o600)).unwrap();

	listener.set_nonblocking(true).unwrap();

	let inspect = store.clone();
	let scope = profile.draft_scope_key();
	let (done, finished) = mpsc::channel();
	let server = spawn_confirmation_server(listener, inspect, scope, boundary, done);
	let (view, visual) =
		cx.add_window_view(|_, cx| confirmation_view(store.clone(), profile.clone(), cx));

	for button in [
		"prompt-confirm-review",
		match boundary {
			None => "prompt-confirm-apply",
			Some(PromptForkBoundary::BeforeInput) => "prompt-confirm-branch",
			Some(PromptForkBoundary::AfterTurn) => "prompt-confirm-after",
		},
	] {
		let mut bounds = None;

		for _ in 0..200 {
			visual.run_until_parked();
			visual.update(|window, cx| {
				window.resize(gpui::size(storage::px(1_000.), storage::px(900.)));
				window.draw(cx).clear(cx);
			});

			bounds = visual.debug_bounds(button);

			if bounds.is_some() {
				break;
			}

			thread::sleep(Duration::from_millis(5));
		}

		visual.simulate_click(bounds.expect("confirmation control").center(), Default::default());
	}
	for _ in 0..300 {
		visual.run_until_parked();
		visual.executor().advance_clock(Duration::from_millis(50));

		if finished.try_recv().is_ok() {
			break;
		}

		thread::sleep(Duration::from_millis(5));
	}

	server.join().unwrap();
	visual.run_until_parked();

	let surface = view.read_with(visual, |v, _| v.surface.clone());

	surface.read_with(visual, |s, cx| {
		assert_eq!(s.composer.read(cx).content(), "Unrelated main input")
	});
}

fn spawn_confirmation_server(
	listener: std::os::unix::net::UnixListener,
	inspect: ClientDraftStore,
	scope: String,
	boundary: Option<PromptForkBoundary>,
	done: Sender<()>,
) -> JoinHandle<()> {
	thread::spawn(move || {
		Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
			let listener = tokio::net::UnixListener::from_std(listener).unwrap();

			time::timeout(Duration::from_secs(10), async {
				for step in (0..4).filter(|step| boundary != Some(PromptForkBoundary::AfterTurn) || *step != 2) {
					let mut socket = tokio_tungstenite::accept_async(listener.accept().await.unwrap().0).await.unwrap();
					let _ = socket.next().await;

					for message in [ServerMessage::Welcome(ServerWelcome { version: CURRENT_VERSION, server_id: ServerId::new(SERVER).unwrap(), instance_id: None, cursor: Cursor(0), reconnect: ReconnectMode::Snapshot }), ServerMessage::Snapshot(SnapshotEnvelope { version: CURRENT_VERSION, server_id: ServerId::new(SERVER).unwrap(), cursor: Cursor(0), items: vec![] })] {
						socket.send(Message::Text(serde_json::to_string(&message).unwrap().into())).await.unwrap();
					}

					let Message::Text(text) = socket.next().await.unwrap().unwrap() else { panic!("request") };
					let request: ClientMessage = serde_json::from_str(&text).unwrap();

					match request {
						ClientMessage::Command(command) => {
							let CommandPayload::Agent { action } = command.payload else { panic!("Agent command") };
							let work = match *action {
								AgentActionDto::PreparePromptEdit { work_id, .. } if step == 0 => work_id,
								AgentActionDto::ConfirmPromptEdit { work_id, review_token, .. } if step == 3 => {
									let disk = DesktopDraftDocument::decode(&inspect.load().unwrap().payload).unwrap();
									let saved = &disk.profiles[&scope].prompt_edits[review_token.as_str()];

									assert_eq!(saved.confirmation_key.as_ref(), Some(&command.idempotency_key));
									assert!(saved.handback_pending && saved.receipt_id.is_none());
									assert_eq!(saved.input.parts()[0]["text"], "Original retained input");
									assert_eq!(disk.profiles[&scope].composer.text, "Unrelated main input");

									work_id
								},
								AgentActionDto::ForkPromptEdit { work_id, review_token, target_work_id, boundary: actual, .. } if step == 3 => {
									assert_eq!(Some(actual), boundary);

									let disk = DesktopDraftDocument::decode(&inspect.load().unwrap().payload).unwrap();
									let saved = &disk.profiles[&scope].prompt_edits[review_token.as_str()];

									assert_eq!(saved.confirmation_key.as_ref(), Some(&command.idempotency_key));
									assert_eq!(saved.fork, Some(PromptForkIntent {target_work_id, boundary: actual}));
									assert!(saved.handback_pending && saved.receipt_id.is_none());
									assert_eq!(disk.profiles[&scope].composer.text, "Unrelated main input");

									work_id
								},
								_ => panic!("unexpected mutation"),
							};
							let receipt = ServerMessage::CommandReceipt(CommandReceipt { version: CURRENT_VERSION, server_id: ServerId::new(SERVER).unwrap(), client_command_id: command.client_command_id.clone(), idempotency_key: command.idempotency_key.clone(), disposition: ReceiptDisposition::Executed, original_client_command_id: command.client_command_id.clone() });
							let result = ServerMessage::CommandResult(CommandResultEnvelope { version: CURRENT_VERSION, server_id: ServerId::new(SERVER).unwrap(), client_command_id: command.client_command_id, idempotency_key: command.idempotency_key, outcome: if step == 0 { CommandOutcome::Succeeded } else { CommandOutcome::Rejected }, entity_revision: (step == 0).then_some(EntityRevision(0)), payload: (step == 0).then_some(ResultPayload::AgentAccepted { work_id: work }), error: (step == 3).then_some(CommandError::IdempotencyConflict) });

							for message in [receipt, result] { socket.send(Message::Text(serde_json::to_string(&message).unwrap().into())).await.unwrap(); }
						},
						ClientMessage::Query(query) => {
							let payload = match query.payload {
								QueryPayload::GetAgentPromptEdit { work_id, thread_id, .. } if step == 1 => {
									let fragment = serde_json::json!([{"type":"text","text":"Original retained input"}]).to_string();

									QueryResultPayload::AgentPromptEdit(PromptEditStatus { work_id, thread_id, phase: PromptEditPhase::Review, evidence: Some(PromptEditEvidence { review_token: WireText::new("a".repeat(64)).unwrap(), receipt_id: None, before_turn_id: WireText::new("turn").unwrap(), item_id: WireText::new("item").unwrap(), removed_turns: 2, content_bytes: fragment.len() as u64, offset: 0, fragment }) })
								},
								QueryPayload::GetAgentModelSettings { work_id } if step == 2 => QueryResultPayload::AgentModelSettings(AgentModelSettingsResult::Available { work_id, thread_id: EntityId::new("thread").unwrap(), account_id: EntityId::new("account").unwrap(), model_provider: None, model: Some(WireText::new("model").unwrap()), reasoning_effort: None }),
								_ => panic!("unexpected query"),
							};
							let message = ServerMessage::QueryResult(QueryResultEnvelope { version: CURRENT_VERSION, server_id: ServerId::new(SERVER).unwrap(), query_id: query.query_id, payload });

							socket.send(Message::Text(serde_json::to_string(&message).unwrap().into())).await.unwrap();
						},
						_ => panic!("unexpected request"),
					}
				}
			}).await.unwrap();
		});

		done.send(()).unwrap();
	})
}

fn confirmation_view(
	store: ClientDraftStore,
	profile: ClientProfile,
	cx: &mut Context<View>,
) -> View {
	let surface = cx.new(AgentSurface::new);

	cx.observe(&surface, |_, _, cx| cx.notify()).detach();

	let work = surface.update(cx, |s, cx| {
		s.draft_profiles.storage = Storage::open(Ok(store.clone()));

		s.bind_drafts(Some(&profile), cx);
		s.profile = Some(profile.clone());
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
		s.review_prompt(&work, "thread", "turn", "item", cx);

		work
	});

	View { surface, work }
}
