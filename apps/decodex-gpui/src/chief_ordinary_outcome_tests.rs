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

#[gpui::test]
fn inherited_ordinary_choices_survive_storage_and_rendered_send(cx: &mut gpui::TestAppContext) {
	use crate::conversations::tests::{connected_conversations, reply_native_model_settings};
	use decodex_protocol::{CommandPayload, ConversationReasoningEffort, DesktopCreationIntent};
	let (_service, profile, _) = super::super::tests::profiles();
	let scope = profile.draft_scope_key();
	let directory = tempfile::tempdir().unwrap();
	let root = directory.path().canonicalize().unwrap().join("desktop");
	let store = ClientDraftStore::open_at(&root).unwrap();
	let (conversations, server, _) = connected_conversations();
	let mut draft = conversations.ordinary_draft("Native continuation").unwrap();
	draft.composer.creation_intent =
		DesktopCreationIntent { model: false, reasoning: true, service_tier: false };
	draft.composer.execution.reasoning_effort = Some(ConversationReasoningEffort::High);
	let mut document = DesktopDraftDocument::default();
	document.profiles.entry(scope.clone()).or_default().ordinary.insert("/tmp".into(), draft);
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
		assert_eq!(s.composer.read(cx).content(), "Native continuation");
		assert!(
			!s.conversations.snapshot().can_submit,
			"restore cannot retain observed-default authority"
		);
	});
	reply_native_model_settings(&conversations, &server);
	shell.update(visual, |s, cx| s.synchronize_conversations(cx));
	visual.run_until_parked();
	visual.update(|window, cx| {
		window.resize(gpui::size(gpui::px(1440.), gpui::px(1000.)));
		window.draw(cx).clear();
	});
	let button = visual.debug_bounds("conversation-send").unwrap();
	visual.simulate_click(button.center(), gpui::Modifiers::default());
	visual.run_until_parked();
	let original = take_ready_command(&conversations, &server).expect("saved rendered submission");
	let CommandPayload::SubmitConversationTurn { execution, overrides: Some(intent), .. } =
		&original.payload
	else {
		panic!("ordinary intent")
	};
	assert_eq!(execution.model.as_str(), "new-native-model");
	assert_eq!(execution.reasoning_effort, Some(ConversationReasoningEffort::High));
	assert!(!intent.model && intent.reasoning && !intent.service_tier);
	let reopened = ClientDraftStore::open_at(&root).unwrap();
	let saved = DesktopDraftDocument::decode(&reopened.load().unwrap().payload).unwrap();
	assert_eq!(saved.profiles[&scope].ordinary["/tmp"].unconfirmed, vec![original.clone()]);
	let (restored, restored_server, _) = connected_conversations();
	let (cold, cold_visual) =
		cx.add_window_view(|window, cx| Shell::new(window, cx, ConnectionView::Stopped));
	cold.update(cold_visual, |s, cx| {
		s.conversations = restored.clone();
		s.reset_cards.profile = Some(profile.clone());
		s.chief.update(cx, |chief, cx| {
			chief.draft_profiles.storage = Storage::open(Ok(reopened));
			chief.bind_profile(Some(profile), cx);
		});
		s.reset_ordinary_draft_binding(cx);
		assert_eq!(s.composer.read(cx).content(), "Native continuation");
		assert!(!s.conversations.snapshot().can_submit);
	});
	assert_eq!(restored.ordinary_draft("Native continuation").unwrap().unconfirmed, vec![original]);
	assert!(
		take_ready_command(&restored, &restored_server).is_none(),
		"restart never replays the saved command"
	);
}

#[gpui::test]
fn acknowledged_archive_stays_removed_after_store_reopen(cx: &mut gpui::TestAppContext) {
	use crate::conversations::tests::{
		prepare_control_check, recorded_archive_fixture, reply_archive_check,
	};
	let text = "Later unsent text";
	let expected = text;
	{
		let (_service, profile, other) = super::super::tests::profiles();
		let scope = profile.draft_scope_key();
		let directory = tempfile::tempdir().unwrap();
		let root = directory.path().canonicalize().unwrap().join("desktop");
		let store = ClientDraftStore::open_at(&root).unwrap();
		let (conversations, server, original) = recorded_archive_fixture();
		let draft = conversations.ordinary_draft(text).unwrap();
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
		prepare_control_check(&conversations);
		shell.update(visual, |s, cx| s.synchronize_conversations(cx));
		visual.run_until_parked();
		visual.update(|window, cx| {
			window.resize(gpui::size(gpui::px(1440.), gpui::px(1000.)));
			window.draw(cx).clear();
		});
		assert!(visual.debug_bounds("ordinary-control-acknowledge-0").is_none());
		let check = visual.debug_bounds("ordinary-control-check-0").unwrap();
		visual.simulate_click(check.center(), gpui::Modifiers::default());
		reply_archive_check(&conversations, &server, &original);
		shell.update(visual, |s, cx| s.synchronize_conversations(cx));
		visual.run_until_parked();
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		let button = visual.debug_bounds("ordinary-control-acknowledge-0").unwrap();
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
			s.conversations = recorded_archive_fixture().0;
			s.reset_cards.profile = Some(profile.clone());
			s.chief.update(cx, |chief, cx| {
				chief.draft_profiles.storage = Storage::open(Ok(reopened));
				chief.bind_profile(Some(profile), cx);
			});
			s.reset_ordinary_draft_binding(cx);
			assert_eq!(s.composer.read(cx).content(), expected);
			assert!(s.conversations.ordinary_control_states().is_empty());
		});
	}
}

#[gpui::test]
fn acknowledged_routing_control_preserves_other_records_after_restart(
	cx: &mut gpui::TestAppContext,
) {
	use crate::conversations::tests::{
		prepare_control_check, recorded_routing_fixture, reply_routing_check,
	};
	let text = "Later unsent text";
	let expected = text;
	for kind in 0..3 {
		let (_service, profile, other) = super::super::tests::profiles();
		let scope = profile.draft_scope_key();
		let directory = tempfile::tempdir().unwrap();
		let root = directory.path().canonicalize().unwrap().join("desktop");
		let store = ClientDraftStore::open_at(&root).unwrap();
		let (conversations, server, original) = recorded_routing_fixture(kind);
		let mut draft = conversations.ordinary_draft(text).unwrap();
		let unrelated = recorded_routing_fixture((kind + 1) % 3).2;
		draft.unconfirmed.push(unrelated.clone());
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
		shell.update(visual, |s, cx| s.synchronize_conversations(cx));
		visual.run_until_parked();
		visual.update(|window, cx| {
			window.resize(gpui::size(gpui::px(1440.), gpui::px(1000.)));
			window.draw(cx).clear();
		});
		assert!(visual.debug_bounds("ordinary-control-acknowledge-0").is_none());
		let check = visual.debug_bounds("ordinary-control-check-0").unwrap();
		prepare_control_check(&conversations);
		visual.simulate_click(check.center(), gpui::Modifiers::default());
		reply_routing_check(&conversations, &server, &original);
		shell.update(visual, |s, cx| s.synchronize_conversations(cx));
		visual.run_until_parked();
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		let button = visual.debug_bounds("ordinary-control-acknowledge-0").unwrap();
		visual.simulate_click(button.center(), gpui::Modifiers::default());
		visual.run_until_parked();
		assert!(take_ready_command(&conversations, &server).is_none());
		let reopened = ClientDraftStore::open_at(&root).unwrap();
		let saved = DesktopDraftDocument::decode(&reopened.load().unwrap().payload).unwrap();
		let active = &saved.profiles[&scope].ordinary["/tmp"];
		assert_eq!(active.unconfirmed, vec![unrelated.clone()]);
		assert_eq!(active.composer.text, expected);
		assert_eq!(active.composer.conversation_id, draft.composer.conversation_id);
		assert_eq!(active.parked, draft.parked);
		assert!(
			saved
				.recovered
				.iter()
				.filter(|copy| copy.scope.as_ref() == Some(&scope))
				.all(|copy| copy.draft.ordinary["/tmp"].unconfirmed == vec![unrelated.clone()])
		);
		assert!(saved.recovered.contains(&foreign_copy));
		let (cold, cold_visual) =
			cx.add_window_view(|window, cx| Shell::new(window, cx, ConnectionView::Stopped));
		cold.update(cold_visual, |s, cx| {
			s.conversations = recorded_routing_fixture(kind).0;
			s.reset_cards.profile = Some(profile.clone());
			s.chief.update(cx, |chief, cx| {
				chief.draft_profiles.storage = Storage::open(Ok(reopened));
				chief.bind_profile(Some(profile), cx);
			});
			s.reset_ordinary_draft_binding(cx);
			assert_eq!(s.composer.read(cx).content(), expected);
			assert_eq!(s.conversations.ordinary_control_states(), vec![(unrelated.clone(), None)]);
			assert!(!s.conversations.snapshot().can_submit);
		});
	}
}
