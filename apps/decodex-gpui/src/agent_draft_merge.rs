//! Reconcile current editor changes through the existing atomic writer.
use crate::shell::agent_surface::drafts::storage::{
	self, AgentSurface, Context, DesktopDraftDocument, Drafts, SaveFailure,
};
use decodex_protocol::{DesktopComposerDraft, DesktopProfileDraft};

// The active slot is a cold-start hint. A live window can show the same saved
// editors with another one active, without changing their contents or owners.
pub(super) fn show_editor(
	mut saved: DesktopProfileDraft,
	window: &DesktopProfileDraft,
) -> DesktopProfileDraft {
	if saved.composer.work_id != window.composer.work_id {
		let editor = match &window.composer.work_id {
			Some(id) => saved.parked.remove(id.as_str()).unwrap_or_else(|| {
				if window.new_conversation.as_ref() == Some(id) {
					return window.composer.clone();
				}
				// A deleted draft must not regain its previous text from this window.
				DesktopComposerDraft {
					work_id: Some(id.clone()),
					thread_id: window.composer.thread_id.clone(),
					..Default::default()
				}
			}),
			None => window.composer.clone(),
		};
		let previous = std::mem::replace(&mut saved.composer, editor);
		if let Some(id) = previous.work_id.as_ref() {
			saved.parked.insert(id.as_str().into(), previous);
		}
	}
	if let Some(id) = window.new_conversation.as_ref()
		&& window.composer.work_id.as_ref() == Some(id)
		&& saved.new_conversation.as_ref() != Some(id)
	{
		saved.new_conversation.clone_from(&window.new_conversation);
		saved.new_conversation_workspace.clone_from(&window.new_conversation_workspace);
		saved.new_conversation_permissions = window.new_conversation_permissions;
		if let Some(folder) = window.chat_folders.get(id.as_str()) {
			saved.chat_folders.insert(id.as_str().into(), folder.clone());
		}
		if let Some(choice) = window.execution.get(id.as_str()) {
			saved.execution.insert(id.as_str().into(), choice.clone());
			saved.execution_revision = saved.execution_revision.max(choice.0);
		}
	}
	saved
}

impl AgentSurface {
	pub(super) fn merge_current_edits(&mut self, cx: &mut Context<Self>) {
		self.cancel_queued_command(cx);
		self.remember_draft_document(cx);

		let state = &mut self.draft_profiles.storage;
		let Some(store) = state.store.clone() else { return };

		if state.seeded {
			state.document.unbound = Default::default();
		}

		let captured = state.document.clone();
		let local = captured.clone();
		let baseline = state.saved.clone();
		let write = cx.background_executor().spawn(async move {
			let snapshot = store.load().map_err(|_| SaveFailure::Failed)?;
			let latest = if snapshot.revision == 0 {
				DesktopDraftDocument::default()
			} else {
				DesktopDraftDocument::decode(&snapshot.payload).map_err(SaveFailure::Invalid)?
			};
			let merged = local.merge_edits(&baseline, &latest).map_err(SaveFailure::Invalid)?;
			let revision = storage::publish_document(&store, snapshot.revision, &merged)?;

			Ok::<_, SaveFailure>((revision, merged))
		});

		state.task = Some(cx.spawn(async move |surface, cx| {
			let result = write.await;
			let _ = surface.update(cx, |surface, cx| {
				surface.draft_profiles.storage.task = None;

				match result {
					Ok((revision, merged)) =>
						surface.finish_draft_reconciliation(captured, merged, revision, cx),
					Err(failure) => {
						let message = match failure {
							SaveFailure::Invalid(reason) => reason,
							SaveFailure::Busy | SaveFailure::Conflict =>
								"Input changed while saving. Retry when the other write completes.",
							SaveFailure::Failed => "Input not saved. Keep this window open.",
						};

						surface.draft_profiles.storage.error = Some(message.into());
						surface.draft_profiles.storage.conflicted = false;
					},
				}

				cx.notify();
			});
		}));

		cx.notify();
	}

	fn finish_draft_reconciliation(
		&mut self,
		captured: DesktopDraftDocument,
		merged: DesktopDraftDocument,
		revision: u64,
		cx: &mut Context<Self>,
	) {
		self.remember_draft_document(cx);

		let state = &mut self.draft_profiles.storage;
		let current = state.document.clone();

		state.saved = merged.clone();
		state.revision = revision;

		let rebased = match current.merge_edits(&captured, &merged) {
			Ok(document) => document,
			Err(reason) => {
				state.error = Some(reason.into());

				return;
			},
		};

		state.document = rebased.clone();
		state.error = None;
		state.conflicted = false;
		state.seeded = false;
		state.busy = false;

		// Every parked profile is already captured in the document. Discard stale
		// editor caches so an unchanged profile adopts the newer disk version.
		self.draft_profiles.saved.clear();

		if let Some(profile) = &self.draft_profiles.active {
			let scope = profile.draft_scope_key();

			if rebased.profiles.get(&scope) != current.profiles.get(&scope)
				&& let (Some(saved), Some(window)) =
					(rebased.profiles.get(&scope), self.capture_draft_document(cx))
			{
				let view = show_editor(saved.clone(), &window);
				self.apply_drafts(Drafts::from_document(view, self.command_epoch), cx);
			}
		} else if rebased.unbound != current.unbound {
			self.restore_unbound_draft(cx);
		}

		self.save_draft_document(cx);
	}
}
