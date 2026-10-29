//! Reconnect only the still-live, never-dispatched write-ahead slot.
use futures_util::StreamExt as _;
use gpui::AppContext as _;
use tokio_tungstenite::tungstenite::Message;

use super::Storage;
use crate::shell::agent_surface::{
	AgentActionDto, AgentSurface, EntityId, HistoryText, LoadState, wire_test_support,
};
use decodex_protocol::{ClientDraftStore, ClientMessage};

struct QueueView(gpui::Entity<AgentSurface>);
impl gpui::Render for QueueView {
	fn render(
		&mut self,
		_: &mut gpui::Window,
		_: &mut gpui::Context<Self>,
	) -> impl gpui::IntoElement {
		gpui::div()
	}
}

#[gpui::test]
fn reconnect_resumes_only_the_persisted_unsent_message_and_preserves_new_edits(
	cx: &mut gpui::TestAppContext,
) {
	let (_root, profile, server) = wire_test_support::fixture(|listener| async move {
		let mut socket = wire_test_support::accept(&listener).await;
		let Message::Text(text) = socket.next().await.unwrap().unwrap() else { panic!("command") };
		let ClientMessage::Command(command) = serde_json::from_str(&text).unwrap() else {
			panic!("input command")
		};
		assert!(matches!(&command.payload, decodex_protocol::CommandPayload::Agent { action }
			if matches!(action.as_ref(), AgentActionDto::Send { text, .. } if text.as_str() == "Queued input")));
		let key = command.idempotency_key.clone();
		// A lost result after submission is uncertain and must never queue a retry.
		socket.close(None).await.unwrap();
		key
	});
	let directory = tempfile::tempdir_in("/tmp").unwrap();
	let store = ClientDraftStore::open_at(&directory.path().canonicalize().unwrap().join("drafts"))
		.unwrap();
	let (view, visual) = cx.add_window_view(|_, cx| QueueView(cx.new(AgentSurface::new)));
	let surface = view.read_with(visual, |view, _| view.0.clone());
	surface.update(visual, |s, cx| {
		s.draft_profiles.storage = Storage::open(Ok(store));
		s.bind_drafts(Some(&profile), cx);
		s.profile = Some(profile);
		s.visual_workspace_fixture(cx);
		s.snapshot.as_mut().unwrap().runtime_source = Some(EntityId::new("runtime").unwrap());
		s.composer_manager = Some("agent".into());
		s.snapshot
			.as_mut()
			.unwrap()
			.work_items
			.iter_mut()
			.find(|work| work.id == "agent")
			.unwrap()
			.codex_thread_id = Some("thread".into());
		s.composer.update(cx, |input, cx| input.set_content("Queued input", cx));
		s.execute(
			AgentActionDto::Send {
				root_id: EntityId::new("agent").unwrap(),
				text: HistoryText::new("Queued input").unwrap(),
			},
			Some("Queued input".into()),
			cx,
		);
		assert!(s.submission.waiting.is_some());
		s.state = LoadState::Stale;
	});
	visual.run_until_parked();
	let key = surface.read_with(visual, |s, _| {
		assert!(s.waiting_for_reconnect());
		assert!(s.submission.command.is_none());
		s.submission.waiting.as_ref().unwrap().key.clone()
	});
	surface.update(visual, |s, cx| {
		s.composer.update(cx, |input, cx| input.set_content("Later editable input", cx));
		s.state = LoadState::Ready;
		s.snapshot.as_mut().unwrap().pending_events.push(decodex_protocol::AgentPendingEventDto {
			id: 1,
			source_event_id: "approval".into(),
			work_item_id: "agent".into(),
			event_kind: "permission_pending".into(),
			created_at_micros: 1,
			delivery_claimed: false,
		});
		s.save_draft_document(cx);
	});
	visual.run_until_parked();
	surface.update(visual, |s, cx| {
		assert!(s.waiting_for_reconnect() && s.submission.command.is_none());
		s.snapshot.as_mut().unwrap().pending_events.clear();
		s.save_draft_document(cx);
	});
	visual.run_until_parked();
	assert_eq!(server.join().unwrap(), key);
	surface.read_with(visual, |s, cx| {
		assert!(s.submission.waiting.is_none());
		assert!(s.uncertain);
		assert_eq!(s.composer.read(cx).content(), "Later editable input");
	});
}
