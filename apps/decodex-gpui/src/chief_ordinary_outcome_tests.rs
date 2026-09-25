//! Exercise outcome acknowledgement through the shared writer and cold restoration.
use super::*;
use crate::{
	client_lifecycle::ConnectionView,
	conversations::tests::{recorded_turn_fixture, reply_recorded_turn, take_ready_command},
	shell::{Destination, Shell},
};
use decodex_protocol::{ConversationTurnOutcomeState as Outcome, DesktopRecoveredDraft};

#[gpui::test]
fn acknowledged_turn_stays_removed_after_store_reopen(cx: &mut gpui::TestAppContext) {
	for (outcome, text, expected, other_owner) in [
		(Outcome::Completed, "Original message", "", false),
		(Outcome::Completed, "Original message", "Original message", true),
		(Outcome::Completed, "Later message", "Later message", false),
		(Outcome::Failed, "Original message", "Original message", false),
		(Outcome::NotSubmitted, "Original message", "Original message", false),
	] {
		let (_service, profile, other) = super::super::tests::profiles();
		let scope = profile.draft_scope_key();
		let directory = tempfile::tempdir().unwrap();
		let root = directory.path().canonicalize().unwrap().join("desktop");
		let store = ClientDraftStore::open_at(&root).unwrap();
		let (conversations, server, original) = recorded_turn_fixture(outcome);
		let mut draft = conversations.ordinary_draft(text).unwrap();
		if other_owner {
			let editor = draft.composer.clone();
			draft.parked.insert(editor.conversation_id.as_ref().unwrap().as_str().into(), editor);
			draft.composer.conversation_id = Some(
				decodex_protocol::EntityId::new("30000000-0000-4000-8000-000000000099").unwrap(),
			);
		}
		let mut saved_profile = DesktopProfileDraft::default();
		saved_profile.ordinary.insert("/tmp".into(), draft.clone());
		let mut document = DesktopDraftDocument::default();
		document.profiles.insert(scope.clone(), saved_profile.clone());
		document.recovered.push(DesktopRecoveredDraft {
			scope: Some(scope.clone()),
			draft: saved_profile.clone(),
		});
		let foreign_copy =
			DesktopRecoveredDraft { scope: Some(other.draft_scope_key()), draft: saved_profile };
		document.recovered.push(foreign_copy.clone());
		store.save(0, &document.encode().unwrap()).unwrap();
		let (shell, visual) =
			cx.add_window_view(|window, cx| Shell::new(window, cx, ConnectionView::Stopped));
		shell.update(visual, |s, cx| {
			s.conversations = conversations.clone();
			s.reset_cards.profile = Some(profile.clone());
			s.selected = Destination::Conversations;
			s.chief.update(cx, |chief, cx| {
				chief.draft_profiles.storage = Storage::open(Ok(store.clone()));
				chief.bind_profile(Some(profile.clone()), cx);
			});
			s.reset_ordinary_draft_binding(cx);
			assert_eq!(s.composer.read(cx).content(), text);
		});
		reply_recorded_turn(&conversations, &server, &original, outcome);
		shell.update(visual, |s, cx| s.synchronize_conversations(cx));
		visual.run_until_parked();
		visual.update(|window, cx| {
			window.resize(gpui::size(gpui::px(1440.), gpui::px(1000.)));
			window.draw(cx).clear();
		});
		let button = visual.debug_bounds("ordinary-turn-acknowledge-0").unwrap();
		visual.simulate_click(button.center(), gpui::Modifiers::default());
		visual.run_until_parked();
		assert!(take_ready_command(&conversations, &server).is_none());
		let reopened = ClientDraftStore::open_at(&root).unwrap();
		let saved = DesktopDraftDocument::decode(&reopened.load().unwrap().payload).unwrap();
		let active = &saved.profiles[&scope].ordinary["/tmp"];
		assert!(active.unconfirmed.is_empty());
		assert_eq!(active.composer.text, expected);
		assert_eq!(active.composer.conversation_id, draft.composer.conversation_id);
		assert_eq!(active.parked, draft.parked);
		assert!(
			saved
				.recovered
				.iter()
				.filter(|copy| copy.scope.as_ref() == Some(&scope))
				.all(|copy| copy.draft.ordinary["/tmp"].unconfirmed.is_empty())
		);
		assert!(saved.recovered.contains(&foreign_copy));
		let (cold, cold_visual) =
			cx.add_window_view(|window, cx| Shell::new(window, cx, ConnectionView::Stopped));
		cold.update(cold_visual, |s, cx| {
			s.conversations = recorded_turn_fixture(Outcome::Unknown).0;
			s.reset_cards.profile = Some(profile.clone());
			s.chief.update(cx, |chief, cx| {
				chief.draft_profiles.storage = Storage::open(Ok(reopened));
				chief.bind_profile(Some(profile), cx);
			});
			s.reset_ordinary_draft_binding(cx);
			assert_eq!(s.composer.read(cx).content(), expected);
			assert!(s.conversations.ordinary_turn_outcomes().is_empty());
		});
	}
}
