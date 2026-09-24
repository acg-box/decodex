//! Cold setup survives disk, profile changes and uncertain creation without replay.
use super::*;
use decodex_protocol::{
	ChiefSandboxDto, ConversationReasoningEffort, DesktopCreationSetup, ServiceTier,
};

fn choose(
	surface: &mut ChiefSurface,
	cx: &mut Context<ChiefSurface>,
	model: &str,
) -> DesktopCreationSetup {
	surface.model.update(cx, |input, cx| input.set_content(model, cx));
	surface.cwd.update(cx, |input, cx| input.set_content("unfinished/relative path", cx));
	surface.account.update(cx, |input, cx| input.set_content("account edit", cx));
	surface.effort = ConversationReasoningEffort::new("provider-effort").unwrap();
	surface.fast = false;
	surface.service_tier = Some(ServiceTier::new("flex").unwrap());
	surface.sandbox = ChiefSandboxDto::WorkspaceWrite;
	surface.creation_setup(cx).unwrap()
}

#[gpui::test]
fn creation_setup_survives_cold_reopen_without_a_message_or_auto_send(
	cx: &mut gpui::TestAppContext,
) {
	let (_service, profile, other) = super::super::tests::profiles();
	let directory = tempfile::tempdir().unwrap();
	let store =
		ClientDraftStore::open_at(&directory.path().canonicalize().unwrap().join("desktop"))
			.unwrap();
	let surface = cx.new(ChiefSurface::new);
	let expected = surface.update(cx, |s, cx| {
		s.draft_profiles.storage = Storage::open(Ok(store.clone()));
		s.bind_profile(Some(profile.clone()), cx);
		let expected = choose(s, cx, "first-model");
		s.bind_profile(Some(other.clone()), cx);
		assert!(s.creation_setup(cx).is_none(), "fresh service must not inherit another setup");
		choose(s, cx, "second-model");
		s.bind_profile(Some(profile.clone()), cx);
		assert_eq!(s.creation_setup(cx), Some(expected.clone()));
		s.remember_draft_document(cx);
		publish_document(&store, 0, &s.draft_profiles.storage.document)
			.unwrap_or_else(|_| panic!("save"));
		expected
	});
	let reopened = cx.new(ChiefSurface::new);
	reopened.update(cx, |s, cx| {
		s.draft_profiles.storage = Storage::open(Ok(store.clone()));
		s.bind_profile(Some(profile), cx);
		assert_eq!(s.creation_setup(cx), Some(expected));
		assert!(s.composer.read(cx).content().is_empty());
		assert!(s.submission.command.is_none() && s.submission.waiting.is_none() && !s.sending);
		assert!(s.capabilities.is_none());
		s.bind_profile(Some(other), cx);
		assert_eq!(s.model.read(cx).content(), "second-model");
	});
}

#[gpui::test]
fn unbound_creation_edits_survive_reopen(cx: &mut gpui::TestAppContext) {
	let directory = tempfile::tempdir().unwrap();
	let store =
		ClientDraftStore::open_at(&directory.path().canonicalize().unwrap().join("desktop"))
			.unwrap();
	let surface = cx.new(ChiefSurface::new);
	let expected = surface.update(cx, |s, cx| {
		s.draft_profiles.storage = Storage::open(Ok(store.clone()));
		let expected = choose(s, cx, "incomplete model ");
		s.remember_draft_document(cx);
		publish_document(&store, 0, &s.draft_profiles.storage.document)
			.unwrap_or_else(|_| panic!("save"));
		expected
	});
	let reopened = cx.new(ChiefSurface::new);
	reopened.update(cx, |s, cx| {
		s.draft_profiles.storage = Storage::open(Ok(store));
		s.restore_unbound_draft(cx);
		assert_eq!(s.creation_setup(cx), Some(expected));
		assert!(s.submission.command.is_none());
	});
}

#[gpui::test]
fn uncertain_creation_keeps_original_setup_and_later_edits_separate(cx: &mut gpui::TestAppContext) {
	let (_service, profile, _) = super::super::tests::profiles();
	let directory = tempfile::tempdir().unwrap();
	let store =
		ClientDraftStore::open_at(&directory.path().canonicalize().unwrap().join("desktop"))
			.unwrap();
	let surface = cx.new(ChiefSurface::new);
	let original = surface.update(cx, |s, cx| {
		s.draft_profiles.storage = Storage::open(Ok(store.clone()));
		s.bind_profile(Some(profile.clone()), cx);
		let original = choose(s, cx, "original-model");
		s.composer.update(cx, |input, cx| input.set_content("Original message", cx));
		let key = IdempotencyKey::new("creation-unknown").unwrap();
		let mut pending = PendingCommand {
			recovery: Some(s.command_draft_copy(cx).unwrap()),
			key: Some(key.clone()),
			steer: None,
			execution_intent: None,
			epoch: s.command_epoch,
			draft: Some("Original message".into()),
			owner: None,
			attachments: Some(vec![]),
			references: Some(vec![]),
		};
		s.fence_command_draft(&mut pending);
		s.submission.unconfirmed.push(key);
		s.sending = true;
		choose(s, cx, "later-model");
		s.remember_draft_document(cx);
		publish_document(&store, 0, &s.draft_profiles.storage.document)
			.unwrap_or_else(|_| panic!("save"));
		original
	});
	let reopened = cx.new(ChiefSurface::new);
	reopened.update(cx, |s, cx| {
		s.draft_profiles.storage = Storage::open(Ok(store));
		s.bind_profile(Some(profile), cx);
		assert_eq!(s.model.read(cx).content(), "later-model");
		assert!(s.uncertain && !s.sending && s.submission.command.is_none());
		let copies = s.recovered_drafts();
		assert_eq!(copies.len(), 1);
		assert_eq!(copies[0].draft.composer.creation, Some(original));
		assert!(copies[0].draft.uncertain);
	});
}

#[gpui::test]
fn restored_empty_directory_is_not_replaced_by_shell_prefill(cx: &mut gpui::TestAppContext) {
	let directory = tempfile::tempdir().unwrap();
	let store =
		ClientDraftStore::open_at(&directory.path().canonicalize().unwrap().join("desktop"))
			.unwrap();
	let surface = cx.new(ChiefSurface::new);
	surface.update(cx, |s, cx| {
		s.draft_profiles.storage = Storage::open(Ok(store.clone()));
		s.seed_context(
			Some(decodex_protocol::ConversationWorkingDirectory::new("/tmp").unwrap()),
			vec![],
			cx,
		);
		s.cwd.update(cx, |input, cx| input.set_content("", cx));
		assert!(s.creation_setup(cx).is_some(), "empty edit still belongs to the saved setup");
		s.remember_draft_document(cx);
		publish_document(&store, 0, &s.draft_profiles.storage.document)
			.unwrap_or_else(|_| panic!("save"));
	});
	let reopened = cx.new(ChiefSurface::new);
	reopened.update(cx, |s, cx| {
		s.draft_profiles.storage = Storage::open(Ok(store));
		s.restore_unbound_draft(cx);
		s.seed_context(
			Some(decodex_protocol::ConversationWorkingDirectory::new("/replacement").unwrap()),
			vec![],
			cx,
		);
		assert!(s.cwd.read(cx).content().is_empty());
		assert!(s.submission.command.is_none());
	});
}
