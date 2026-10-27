//! Reconcile current editor changes through the existing atomic writer.
use crate::shell::agent_surface::drafts::storage::{
	self, AgentSurface, Context, DesktopDraftDocument, Drafts, SaveFailure,
};

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
				&& let Some(saved) = rebased.profiles.get(&scope)
			{
				let previous_owner = self.composer_manager.clone();

				self.apply_drafts(Drafts::from_document(saved.clone(), self.command_epoch), cx);

				if self.composer_manager != previous_owner {
					self.selected = self.composer_manager.clone();
					self.history = None;
					self.history_task = None;
					self.history_requested_for = None;

					self.load_history(cx);
				}
			}
		} else if rebased.unbound != current.unbound {
			self.restore_unbound_draft(cx);
		}

		self.save_draft_document(cx);
	}
}
