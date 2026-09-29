//! Recovery must retain unresolved intent and never create another branch.
use super::*;
use decodex_protocol::*;
use futures_util::{SinkExt, StreamExt};
use std::os::unix::fs::PermissionsExt;
use tokio_tungstenite::tungstenite::Message;
const SERVER: &str = "018f0f9e-7b6e-4a31-8f4c-1d2e3f405162";

#[gpui::test]
fn fork_recovery_retains_uncertainty_and_releases_only_matching_rejection(
	cx: &mut gpui::TestAppContext,
) {
	cx.background_executor.allow_parking();
	for case in [
		"uncertain",
		"acknowledged",
		"rejected",
		"thread",
		"target",
		"boundary",
		"absent",
		"unavailable",
	] {
		let (service, profile, _) = super::super::super::drafts::tests::profiles();
		let surface = cx.new(AgentSurface::new);
		let original = install_pending(&surface, profile, cx);
		let mut status = PromptForkStatus {
			work_id: original.work_id.clone(),
			thread_id: original.thread_id.clone(),
			review_token: original.review_token.clone(),
			target_work_id: original.fork.as_ref().unwrap().target_work_id.clone(),
			target_thread_id: None,
			boundary: PromptForkBoundary::BeforeInput,
			phase: PromptForkPhase::Rejected,
			edit_receipt_id: None,
		};
		match case {
			"uncertain" => status.phase = PromptForkPhase::Uncertain,
			"acknowledged" => status.phase = PromptForkPhase::Acknowledged,
			"thread" => status.thread_id = WireText::new("another-thread").unwrap(),
			"target" => status.target_work_id = EntityId::new("another-target").unwrap(),
			"boundary" => status.boundary = PromptForkBoundary::AfterTurn,
			_ => {},
		}
		let response = match case {
			"absent" => PromptForkResult::Available(None),
			"unavailable" => PromptForkResult::Unavailable,
			_ => PromptForkResult::Available(Some(status)),
		};
		let socket = service.path().join("server/decodex.sock");
		let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
		std::fs::set_permissions(socket, std::fs::Permissions::from_mode(0o600)).unwrap();
		listener.set_nonblocking(true).unwrap();
		let server = serve_recovery(listener, original.clone(), response);
		surface.update(cx, |s, cx| s.recover_prompt_branch(original.clone(), cx));
		let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
		loop {
			cx.run_until_parked();
			if surface.read_with(cx, |s, _| s.prompt_edit.task.is_none()) {
				break;
			}
			assert!(std::time::Instant::now() < deadline, "recovery did not finish: {case}");
			std::thread::sleep(std::time::Duration::from_millis(5));
		}
		server.join().unwrap();
		surface.read_with(cx, |s, cx| {
			let mut expected = original.clone();
			if case == "rejected" {
				expected.fork = None;
				expected.confirmation_key = None;
				expected.handback_pending = false;
			}
			assert_eq!(s.prompt_edit.draft.as_ref(), Some(&expected), "{case}");
			assert_eq!(s.saved_prompt_editors(original.work_id.as_str()), vec![expected], "{case}");
			assert_eq!(s.selected.as_deref(), Some(original.work_id.as_str()));
			assert_eq!(s.composer.read(cx).content(), "Separate unsent input");
			assert!(!s.prompt_edit.feedback.is_empty());
		});
	}
}

fn install_pending(
	surface: &Entity<AgentSurface>,
	profile: ClientProfile,
	cx: &mut gpui::TestAppContext,
) -> DesktopPromptEditDraft {
	surface.update(cx, |s, cx| {
		s.bind_profile(Some(profile.clone()), cx);
		s.poll_task = None;
		s.visual_workspace_fixture(cx);
		s.state = super::super::super::LoadState::Ready;
		let work = s.selected.clone().unwrap();
		s.snapshot
			.as_mut()
			.unwrap()
			.work_items
			.iter_mut()
			.find(|w| w.id == work)
			.unwrap()
			.codex_thread_id = Some("thread".into());
		s.composer.update(cx, |input, cx| input.set_content("Separate unsent input", cx));
		let input = PromptDraft::new(vec![
			serde_json::json!({"type":"text","text":"Edited input"}),
			serde_json::json!({"type":"image","fileId":"retained-media"}),
		])
		.unwrap();
		let draft = DesktopPromptEditDraft {
			work_id: EntityId::new(&work).unwrap(),
			thread_id: WireText::new("thread").unwrap(),
			before_turn_id: WireText::new("turn").unwrap(),
			item_id: WireText::new("item").unwrap(),
			original_hash: input.fingerprint().unwrap(),
			review_token: WireText::new("a".repeat(64)).unwrap(),
			receipt_id: None,
			confirmation_key: Some(IdempotencyKey::new("original-confirmation").unwrap()),
			fork: Some(PromptForkIntent {
				target_work_id: EntityId::new("branch").unwrap(),
				boundary: PromptForkBoundary::BeforeInput,
			}),
			pending_send: None,
			handback_pending: true,
			input,
		};
		s.prompt_edit =
			Panel { profile: Some(profile), work, thread: "thread".into(), ..Default::default() };
		s.install_prompt_editors(draft.clone(), cx).unwrap();
		draft
	})
}

fn serve_recovery(
	listener: std::os::unix::net::UnixListener,
	original: DesktopPromptEditDraft,
	response: PromptForkResult,
) -> std::thread::JoinHandle<()> {
	std::thread::spawn(move || {
		tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(
			async {
				let listener = tokio::net::UnixListener::from_std(listener).unwrap();
				tokio::time::timeout(std::time::Duration::from_secs(5), async {
					for step in 0..2 {
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
						match serde_json::from_str::<ClientMessage>(&text).unwrap() {
							ClientMessage::Command(command) if step == 0 => {
								let CommandPayload::Agent { action } = command.payload else {
									panic!("agent action")
								};
								assert_eq!(
									*action,
									AgentActionDto::RecoverPromptFork {
										work_id: original.work_id.clone(),
										review_token: original.review_token.clone()
									}
								);
								// Lose the command reply. Only the subsequent receipt establishes
								// state.
							},
							ClientMessage::Query(query) if step == 1 => {
								assert_eq!(
									query.payload,
									QueryPayload::GetAgentPromptFork {
										work_id: original.work_id.clone(),
										review_token: original.review_token.clone()
									}
								);
								let result = ServerMessage::QueryResult(QueryResultEnvelope {
									version: CURRENT_VERSION,
									server_id: ServerId::new(SERVER).unwrap(),
									query_id: query.query_id,
									payload: QueryResultPayload::AgentPromptFork(response.clone()),
								});
								socket
									.send(Message::Text(
										serde_json::to_string(&result).unwrap().into(),
									))
									.await
									.unwrap();
							},
							_ => panic!("unexpected recovery request"),
						}
					}
					assert!(
						tokio::time::timeout(
							std::time::Duration::from_millis(100),
							listener.accept()
						)
						.await
						.is_err(),
						"recovery opened an unexpected third connection",
					);
				})
				.await
				.unwrap();
			},
		);
	})
}
