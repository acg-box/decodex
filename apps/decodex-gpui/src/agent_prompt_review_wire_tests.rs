//! A rejected preparation keeps its real reason beside the Side chat entry point.
use std::{fs, os::unix::fs::PermissionsExt as _, thread, time::Duration};

use futures_util::{SinkExt as _, StreamExt as _};
use gpui::{AppContext as _, Entity, IntoElement, Render, TestAppContext, Window};
use tokio_tungstenite::tungstenite::Message;

use super::{AgentSurface, Context, PromptForkBoundary};
use crate::shell::agent_surface::drafts::tests;
use decodex_protocol::{
	AgentActionDto, CURRENT_VERSION, ClientMessage, CommandError, CommandOutcome, CommandPayload,
	CommandReceipt, CommandResultEnvelope, Cursor, ReceiptDisposition, ReconnectMode,
	ServerMessage, ServerWelcome, SnapshotEnvelope, WireText,
};

const REASON: &str = "This conversation is in use by another app.";

struct View(Entity<AgentSurface>);
impl Render for View {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		self.0.update(cx, |s, cx| s.render_workspace(window, cx))
	}
}

#[gpui::test]
fn side_chat_rejection_keeps_the_reason_at_the_tabs_and_can_be_dismissed(cx: &mut TestAppContext) {
	cx.background_executor.allow_parking();
	let (service, profile, _) = tests::profiles();
	let server_id = profile.expected_server_id().clone();
	let socket = service.path().join("server/decodex.sock");
	let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
	fs::set_permissions(socket, fs::Permissions::from_mode(0o600)).unwrap();
	listener.set_nonblocking(true).unwrap();
	let server = thread::spawn(move || {
		tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
			tokio::time::timeout(Duration::from_secs(5), async {
				let listener = tokio::net::UnixListener::from_std(listener).unwrap();
				// Viewport reads can connect before preparation. Close those one-shot reads.
				let (mut socket, command) = loop {
					let mut socket = tokio_tungstenite::accept_async(listener.accept().await.unwrap().0).await.unwrap();
					let _ = socket.next().await;
					for message in [
						ServerMessage::Welcome(ServerWelcome { version: CURRENT_VERSION, server_id: server_id.clone(), instance_id: None, cursor: Cursor(0), reconnect: ReconnectMode::Snapshot }),
						ServerMessage::Snapshot(SnapshotEnvelope { version: CURRENT_VERSION, server_id: server_id.clone(), cursor: Cursor(0), items: vec![] }),
					] {
						socket.send(Message::Text(serde_json::to_string(&message).unwrap().into())).await.unwrap();
					}
					let Message::Text(text) = socket.next().await.unwrap().unwrap() else { panic!("request") };
					if let ClientMessage::Command(command) = serde_json::from_str(&text).unwrap() {
						break (socket, command);
					}
				};
				assert!(matches!(command.payload, CommandPayload::Agent { action } if matches!(*action, AgentActionDto::PreparePromptEdit { .. })));
				for message in [
					ServerMessage::CommandReceipt(CommandReceipt { version: CURRENT_VERSION, server_id: server_id.clone(), client_command_id: command.client_command_id.clone(), idempotency_key: command.idempotency_key.clone(), disposition: ReceiptDisposition::Refused, original_client_command_id: command.client_command_id.clone() }),
					ServerMessage::CommandResult(CommandResultEnvelope { version: CURRENT_VERSION, server_id, client_command_id: command.client_command_id, idempotency_key: command.idempotency_key, outcome: CommandOutcome::Rejected, entity_revision: None, payload: None, error: Some(CommandError::ApplicationUnavailable { message: WireText::new(REASON).unwrap() }) }),
				] {
					socket.send(Message::Text(serde_json::to_string(&message).unwrap().into())).await.unwrap();
				}
			}).await.unwrap();
		});
	});
	let (view, visual) = cx.add_window_view(|_, cx| {
		let surface = cx.new(AgentSurface::new);
		cx.observe(&surface, |_, _, cx| cx.notify()).detach();
		surface.update(cx, |s, cx| {
			s.visual_side_chat_failure(profile, cx);
			s.request_prompt_review(
				("agent", "source-agent", "turn", "item"),
				None,
				Some(PromptForkBoundary::AttachedAfterTurn),
				cx,
			);
		});
		View(surface)
	});
	let surface = view.read_with(visual, |view, _| view.0.clone());
	for _ in 0..500 {
		visual.run_until_parked();
		if surface.read_with(visual, |s, _| s.prompt_edit.task.is_none()) {
			break;
		}
		thread::sleep(Duration::from_millis(5));
	}
	server.join().unwrap();
	visual.run_until_parked();
	visual.simulate_resize(gpui::size(gpui::px(1_200.), gpui::px(760.)));
	visual.update(crate::ui_frame::draw_current_frame);
	thread::sleep(Duration::from_millis(240));
	visual.update(crate::ui_frame::draw_frame);
	surface.read_with(visual, |s, cx| {
		assert_eq!(s.prompt_edit.feedback, REASON);
		assert!(s.prompt_edit.draft.is_none());
		assert_eq!(s.composer.read(cx).content(), "Keep this unsent input.");
	});
	let header = visual.debug_bounds("conversation-header").unwrap();
	let notice = visual.debug_bounds("side-chat-status").unwrap();
	assert!(notice.top() >= header.bottom());
	assert!(notice.bottom() < visual.debug_bounds("floating-composer").unwrap().top());
	assert!(visual.debug_bounds("fork-retry").is_some());
	let close = visual.debug_bounds("prompt-review-close").unwrap().center();
	visual.simulate_click(close, Default::default());
	visual.update(crate::ui_frame::draw_current_frame);
	assert!(visual.debug_bounds("side-chat-status").is_none());
	surface.read_with(visual, |s, cx| {
		assert!(s.prompt_edit.work.is_empty());
		assert_eq!(s.selected.as_deref(), Some("agent"));
		assert_eq!(s.composer.read(cx).content(), "Keep this unsent input.");
		assert_eq!(
			s.conversation_record("agent").unwrap().codex_thread_id.as_deref(),
			Some("source-agent")
		);
	});
}
