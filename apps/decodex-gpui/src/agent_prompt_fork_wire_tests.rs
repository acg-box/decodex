//! Recovery must retain unresolved intent and never create another branch.
use std::{
	fs::{self, Permissions},
	os::unix::fs::PermissionsExt as _,
	thread::{self, JoinHandle},
	time::{Duration, Instant},
};

use futures_util::{SinkExt as _, StreamExt as _};
#[cfg(test)] use gpui::AppContext as _;
use gpui::TestAppContext;
use tokio::{runtime::Builder, time};
use tokio_tungstenite::tungstenite::Message;

#[cfg(test)] use crate::shell::agent_surface::prompt_edit::fork::Entity;
use crate::shell::agent_surface::{
	LoadState,
	drafts::tests,
	prompt_edit::fork::{
		AgentActionDto, AgentSnapshotResult, AgentSurface, DesktopPromptEditDraft, EntityId,
		IdempotencyKey, Panel, PromptDraft, PromptForkBoundary, PromptForkPhase, PromptForkResult,
		WireText,
	},
};
use decodex_protocol::{
	AgentSnapshotDto, CURRENT_VERSION, ClientMessage, ClientProfile, CommandPayload, Cursor,
	PromptEditEvidence, PromptEditPhase, PromptEditStatus, PromptForkIntent, PromptForkStatus,
	QueryPayload, QueryResultEnvelope, QueryResultPayload, ReconnectMode, ServerId, ServerMessage,
	ServerWelcome, SnapshotEnvelope,
};

const SERVER: &str = "018f0f9e-7b6e-4a31-8f4c-1d2e3f405162";

#[gpui::test]
fn fork_recovery_retains_uncertainty_and_releases_only_matching_rejection(cx: &mut TestAppContext) {
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
		let (service, profile, _) = tests::profiles();
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

		fs::set_permissions(socket, Permissions::from_mode(0o600)).unwrap();

		listener.set_nonblocking(true).unwrap();

		let server = serve_recovery(listener, original.clone(), response, vec![]);

		surface.update(cx, |s, cx| s.recover_prompt_branch(original.clone(), cx));

		let deadline = Instant::now() + Duration::from_secs(5);

		loop {
			cx.run_until_parked();

			if surface.read_with(cx, |s, _| s.prompt_edit.task.is_none()) {
				break;
			}

			assert!(std::time::Instant::now() < deadline, "recovery did not finish: {case}");

			thread::sleep(Duration::from_millis(5));
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
	cx: &mut TestAppContext,
) -> DesktopPromptEditDraft {
	surface.update(cx, |s, cx| {
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
	tail: Vec<(QueryPayload, QueryResultPayload)>,
) -> JoinHandle<()> {
	thread::spawn(move || {
		Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
			let listener = tokio::net::UnixListener::from_std(listener).unwrap();

			time::timeout(Duration::from_secs(5), async {
				for step in 0..(2 + tail.len()) {
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
							.send(Message::Text(serde_json::to_string(&message).unwrap().into()))
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
								.send(Message::Text(serde_json::to_string(&result).unwrap().into()))
								.await
								.unwrap();
						},
						ClientMessage::Query(query) if step >= 2 => {
							let (expected, payload) = &tail[step - 2];

							assert_eq!(&query.payload, expected);

							let result = ServerMessage::QueryResult(QueryResultEnvelope {
								version: CURRENT_VERSION,
								server_id: ServerId::new(SERVER).unwrap(),
								query_id: query.query_id,
								payload: payload.clone(),
							});

							socket
								.send(Message::Text(serde_json::to_string(&result).unwrap().into()))
								.await
								.unwrap();
						},

						_ => panic!("unexpected recovery request"),
					}
				}

				assert!(
					!tail.is_empty()
						|| time::timeout(std::time::Duration::from_millis(100), listener.accept())
							.await
							.is_err(),
					"recovery opened an unexpected third connection",
				);
			})
			.await
			.unwrap();
		});
	})
}

#[gpui::test]
fn fork_recovery_opens_verified_target_and_keeps_edited_input(cx: &mut TestAppContext) {
	cx.background_executor.allow_parking();

	for boundary in [PromptForkBoundary::BeforeInput, PromptForkBoundary::AfterTurn] {
		let (service, profile, _) = tests::profiles();
		let surface = cx.new(AgentSurface::new);
		let mut pending = install_pending(&surface, profile, cx);
		let canonical = pending.input.clone();

		pending.input.replace_text(0, 0..12, "Locally revised input").unwrap();

		pending.fork.as_mut().unwrap().boundary = boundary;

		surface.update(cx, |s, cx| s.install_prompt_editors(pending.clone(), cx).unwrap());

		let (snapshot, source) = surface.read_with(cx, |s, _| {
			let mut snapshot = s.snapshot.clone().unwrap();
			let source = snapshot
				.work_items
				.iter()
				.find(|w| w.id == pending.work_id.as_str())
				.unwrap()
				.clone();
			let mut target = source.clone();

			target.id = "branch".into();
			target.codex_thread_id = Some("branch-thread".into());

			snapshot.work_items.push(target);

			assert!(snapshot.is_valid());

			(snapshot, source)
		});
		let status = PromptForkStatus {
			work_id: pending.work_id.clone(),
			thread_id: pending.thread_id.clone(),
			review_token: pending.review_token.clone(),
			target_work_id: EntityId::new("branch").unwrap(),
			target_thread_id: Some(WireText::new("branch-thread").unwrap()),
			boundary,
			phase: PromptForkPhase::Forked,
			edit_receipt_id: (boundary == PromptForkBoundary::BeforeInput).then_some(42),
		};
		let tail = successful_reads(&pending, &canonical, &status, snapshot);
		let socket = service.path().join("server/decodex.sock");
		let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();

		fs::set_permissions(socket, Permissions::from_mode(0o600)).unwrap();

		listener.set_nonblocking(true).unwrap();

		let server = serve_recovery(
			listener,
			pending.clone(),
			PromptForkResult::Available(Some(status)),
			tail,
		);

		surface.update(cx, |s, cx| s.recover_prompt_branch(pending.clone(), cx));

		let deadline = Instant::now() + Duration::from_secs(5);

		loop {
			cx.run_until_parked();

			if surface.read_with(cx, |s, _| s.prompt_edit.task.is_none()) {
				break;
			}

			assert!(std::time::Instant::now() < deadline, "recovery did not finish");

			thread::sleep(Duration::from_millis(5));
		}

		server.join().unwrap();

		surface.update(cx, |s, cx| {
			assert_eq!(s.selected.as_deref(), Some("branch"));
			assert_eq!(
				s.snapshot.as_ref().unwrap().work_items.iter().find(|w| w.id == source.id),
				Some(&source)
			);

			let mut expected = pending.clone();

			expected.fork = None;
			expected.confirmation_key = None;
			expected.handback_pending = boundary == PromptForkBoundary::BeforeInput;

			if boundary == PromptForkBoundary::BeforeInput {
				expected.work_id = EntityId::new("branch").unwrap();
				expected.thread_id = WireText::new("branch-thread").unwrap();
				expected.receipt_id = Some(42);

				assert_eq!(s.prompt_edit.draft.as_ref(), Some(&expected));
				assert_eq!(s.prompt_edit.editors[0].1.read(cx).content(), "Locally revised input");
				assert!(s.saved_prompt_editors(pending.work_id.as_str()).is_empty());
			} else {
				assert!(s.prompt_edit.draft.is_none());
				assert!(s.saved_prompt_editors("branch").is_empty());
			}

			if boundary == PromptForkBoundary::BeforeInput {
				assert_eq!(s.saved_prompt_editors(expected.work_id.as_str()), vec![expected]);
			} else {
				assert!(s.saved_prompt_editors(pending.work_id.as_str()).is_empty());
			}
			assert!(s.composer.read(cx).content().is_empty());

			s.open_page(pending.work_id.as_str(), cx);

			assert_eq!(s.composer.read(cx).content(), "Separate unsent input");
		});
	}
}

fn successful_reads(
	pending: &DesktopPromptEditDraft,
	canonical: &PromptDraft,
	status: &PromptForkStatus,
	snapshot: AgentSnapshotDto,
) -> Vec<(QueryPayload, QueryResultPayload)> {
	let mut replies = Vec::new();

	if status.boundary == PromptForkBoundary::BeforeInput {
		let fragment = serde_json::to_string(canonical.parts()).unwrap();

		replies.push((
			QueryPayload::GetAgentPromptEdit {
				work_id: status.target_work_id.clone(),
				thread_id: status.target_thread_id.clone().unwrap(),
				review_token: None,
				offset: 0,
			},
			QueryResultPayload::AgentPromptEdit(PromptEditStatus {
				work_id: status.target_work_id.clone(),
				thread_id: status.target_thread_id.clone().unwrap(),
				phase: PromptEditPhase::Applied,
				evidence: Some(PromptEditEvidence {
					review_token: pending.review_token.clone(),
					receipt_id: Some(42),
					before_turn_id: pending.before_turn_id.clone(),
					item_id: pending.item_id.clone(),
					removed_turns: 1,
					content_bytes: fragment.len() as u64,
					offset: 0,
					fragment,
				}),
			}),
		));
	}

	replies.push((
		QueryPayload::GetAgentSnapshot,
		QueryResultPayload::AgentSnapshot(AgentSnapshotResult::Available(snapshot)),
	));

	replies
}
