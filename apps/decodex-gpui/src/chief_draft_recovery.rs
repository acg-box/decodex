//! Explicit conflict reconciliation; copies retain exact service ownership.
use super::{ChiefSurface, Context, DesktopDraftDocument, Drafts, SaveFailure, publish_document};
use decodex_protocol::{ClientProfile, DesktopRecoveredDraft};

#[path = "chief_draft_export.rs"] mod export;

impl ChiefSurface {
	pub(in super::super::super) fn show_recovered_drafts(&self) -> bool {
		self.draft_profiles.storage.show_recovered
	}

	pub(in super::super::super) fn toggle_recovered_drafts(&mut self, cx: &mut Context<Self>) {
		self.draft_profiles.storage.show_recovered = !self.draft_profiles.storage.show_recovered;
		cx.notify();
	}

	pub(in super::super::super) fn can_keep_both_drafts(&self) -> bool {
		let state = &self.draft_profiles.storage;
		state.reconcilable && state.task.is_none() && !self.sending
	}

	pub(in super::super::super) fn keep_both_drafts(&mut self, cx: &mut Context<Self>) {
		if !self.can_keep_both_drafts() {
			return;
		}
		self.reconcile_draft_action(None, None, cx);
	}

	pub(in super::super::super) fn recovered_drafts(&self) -> Vec<DesktopRecoveredDraft> {
		let scope = self.draft_profiles.active.as_ref().map(ClientProfile::draft_scope_key);
		self.draft_profiles
			.storage
			.document
			.recovered
			.iter()
			.filter(|copy| copy.scope == scope || copy.scope.is_none())
			.cloned()
			.collect()
	}

	pub(in super::super::super) fn recovered_draft_count(&self) -> usize {
		let scope = self.draft_profiles.active.as_ref().map(ClientProfile::draft_scope_key);
		self.draft_profiles
			.storage
			.document
			.recovered
			.iter()
			.filter(|copy| copy.scope == scope || copy.scope.is_none())
			.count()
	}

	pub(in super::super::super) fn restore_draft_copy(
		&mut self,
		copy: DesktopRecoveredDraft,
		cx: &mut Context<Self>,
	) {
		if self.sending
			|| self.draft_profiles.storage.task.is_some()
			|| !self.recovered_drafts().contains(&copy)
			|| copy.scope != self.draft_profiles.active.as_ref().map(ClientProfile::draft_scope_key)
		{
			return;
		}
		self.reconcile_draft_action(Some(copy), None, cx);
	}

	pub(in super::super::super) fn draft_copy_matches_service(
		&self,
		copy: &DesktopRecoveredDraft,
	) -> bool {
		copy.scope == self.draft_profiles.active.as_ref().map(ClientProfile::draft_scope_key)
	}

	pub(in super::super::super) fn request_draft_copy_removal(
		&mut self,
		copy: DesktopRecoveredDraft,
		cx: &mut Context<Self>,
	) {
		if self.recovered_drafts().contains(&copy) && !copy.draft.has_unconfirmed_delivery() {
			self.draft_profiles.storage.remove_candidate = Some(copy);
			cx.notify();
		}
	}

	pub(in super::super::super) fn removing_draft_copy(
		&self,
		copy: &DesktopRecoveredDraft,
	) -> bool {
		self.draft_profiles.storage.remove_candidate.as_ref() == Some(copy)
	}

	pub(in super::super::super) fn cancel_draft_copy_removal(&mut self, cx: &mut Context<Self>) {
		self.draft_profiles.storage.remove_candidate = None;
		cx.notify();
	}

	pub(in super::super::super) fn confirm_draft_copy_removal(
		&mut self,
		copy: DesktopRecoveredDraft,
		cx: &mut Context<Self>,
	) {
		if self.sending
			|| self.draft_profiles.storage.task.is_some()
			|| !self.removing_draft_copy(&copy)
			|| !self.recovered_drafts().contains(&copy)
			|| copy.draft.has_unconfirmed_delivery()
		{
			return;
		}
		self.draft_profiles.storage.remove_candidate = None;
		self.reconcile_draft_action(None, Some(copy), cx);
	}

	fn reconcile_draft_action(
		&mut self,
		copy: Option<DesktopRecoveredDraft>,
		remove: Option<DesktopRecoveredDraft>,
		cx: &mut Context<Self>,
	) {
		self.cancel_queued_command(cx);
		self.remember_draft_document(cx);
		let state = &mut self.draft_profiles.storage;
		let Some(store) = state.store.clone() else { return };
		if state.seeded {
			state.document.unbound = Default::default();
		}
		let captured = state.document.clone();
		let mut local = captured.clone();

		let mut baseline = state.saved.clone();
		let write = cx.background_executor().spawn(async move {
			let snapshot = store.load().map_err(|_| SaveFailure::Failed)?;
			let mut latest = if snapshot.revision == 0 {
				DesktopDraftDocument::default()
			} else {
				DesktopDraftDocument::decode(&snapshot.payload).map_err(SaveFailure::Invalid)?
			};
			if let Some(remove) = &remove {
				latest = latest.remove_recovered_copy(remove).map_err(SaveFailure::Invalid)?;
				local.recovered.retain(|saved| saved != remove);
				baseline.recovered.retain(|saved| saved != remove);
			}
			if let Some(copy) = &copy {
				// With no concurrent change to this target, current input legitimately
				// replaces the old baseline. Restore then exchanges it with the selected
				// copy, instead of requiring a temporary 33rd recovery slot.
				if let Some(scope) = &copy.scope {
					if latest.profiles.get(scope) == baseline.profiles.get(scope)
						&& let Some(current) = local.profiles.get(scope)
					{
						latest.profiles.insert(scope.clone(), current.clone());
					}
				} else {
					if latest.unbound == baseline.unbound {
						latest.unbound = local.unbound.clone();
					}
					if latest.unbound_ordinary == baseline.unbound_ordinary {
						latest.unbound_ordinary = local.unbound_ordinary.clone();
					}
				}
			}
			let mut merged =
				local.reconcile_keep_both(&baseline, &latest).map_err(SaveFailure::Invalid)?;
			if let Some(copy) = &copy {
				merged = merged.restore_recovered_copy(copy).map_err(SaveFailure::Invalid)?;
			}
			let revision = publish_document(&store, snapshot.revision, &merged)?;
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
							SaveFailure::Busy =>
								"Draft storage is busy. Keep both copies again after the other save finishes.",
							SaveFailure::Conflict =>
								"Drafts changed again. Keep both copies again to include the new changes.",
							SaveFailure::Failed =>
								"Draft copies could not be saved. Current edits remain in this window.",
						};
						surface.draft_profiles.storage.error = Some(message.into());
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
		let rebased = match current.reconcile_keep_both(&captured, &merged) {
			Ok(document) => document,
			Err(reason) => {
				state.error = Some(reason.into());
				return;
			},
		};
		state.document = rebased.clone();
		state.error = None;
		state.reconcilable = false;
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
		self.feedback = "Draft changes saved. Nothing was sent.".into();
		self.save_draft_document(cx);
	}
}

#[cfg(test)]
mod tests {
	use super::{
		super::{
			ClientDraftStore, DesktopComposerDraft, DesktopProfileDraft, IdempotencyKey, Storage,
		},
		*,
	};

	#[gpui::test]
	fn full_copy_capacity_requires_confirmed_removal_and_retains_current_input(
		cx: &mut gpui::TestAppContext,
	) {
		let (_service, profile, _) = super::super::super::tests::profiles();
		let directory = tempfile::tempdir().unwrap();
		let store =
			ClientDraftStore::open_at(&directory.path().canonicalize().unwrap().join("desktop"))
				.unwrap();
		let mut doc = DesktopDraftDocument::default();
		for index in 0..32 {
			doc.recovered.push(DesktopRecoveredDraft {
				scope: Some(profile.draft_scope_key()),
				draft: DesktopProfileDraft {
					composer: DesktopComposerDraft {
						text: format!("copy-{index}"),
						..Default::default()
					},
					..Default::default()
				},
			});
		}
		store.save(0, &doc.encode().unwrap()).unwrap();
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual, |s, cx| {
			s.draft_profiles.storage = Storage::open(Ok(store.clone()));
			s.bind_profile(Some(profile.clone()), cx);
			s.composer.update(cx, |input, cx| input.set_content("unsaved current input", cx));
			let mut remote = doc.clone();
			remote.profiles.insert(
				profile.draft_scope_key(),
				DesktopProfileDraft {
					composer: DesktopComposerDraft {
						text: "other window input".into(),
						..Default::default()
					},
					..Default::default()
				},
			);
			store.save(1, &remote.encode().unwrap()).unwrap();
			s.restore_draft_copy(doc.recovered[0].clone(), cx);
			s.toggle_recovered_drafts(cx);
		});
		visual.run_until_parked();
		surface.read_with(visual, |s, cx| {
			assert_eq!(s.composer.read(cx).content(), "unsaved current input");
			assert!(s.draft_storage_notice().unwrap().contains("Too many recovered"));
		});
		assert_eq!(store.load().unwrap().revision, 2);
		visual.simulate_resize(gpui::size(gpui::px(1200.0), gpui::px(1000.0)));
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		let remove = visual.debug_bounds("draft-copy-remove-0").unwrap();
		visual.simulate_click(remove.center(), gpui::Modifiers::default());
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		assert_eq!(store.load().unwrap().revision, 2);
		let confirm = visual.debug_bounds("draft-copy-remove-confirm-0").unwrap();
		visual.simulate_click(confirm.center(), gpui::Modifiers::default());
		visual.run_until_parked();
		let saved = DesktopDraftDocument::decode(&store.load().unwrap().payload).unwrap();
		assert_eq!(saved.recovered.len(), 32);
		assert_eq!(
			saved.profiles[&profile.draft_scope_key()].composer.text,
			"unsaved current input"
		);
		surface.update(visual, |s, cx| s.restore_draft_copy(doc.recovered[1].clone(), cx));
		visual.run_until_parked();
		surface.read_with(visual, |s, cx| {
			assert_eq!(s.composer.read(cx).content(), "copy-1");
			assert!(s.submission.command.is_none());
		});
		let saved = DesktopDraftDocument::decode(&store.load().unwrap().payload).unwrap();
		assert!(
			saved.recovered.iter().any(|copy| copy.draft.composer.text == "unsaved current input")
		);
	}

	#[gpui::test]
	fn restore_copy_click_preserves_unsaved_editor_and_isolates_profile(
		cx: &mut gpui::TestAppContext,
	) {
		let (_service, profile, other) = super::super::super::tests::profiles();
		let directory = tempfile::tempdir().unwrap();
		let store =
			ClientDraftStore::open_at(&directory.path().canonicalize().unwrap().join("desktop"))
				.unwrap();
		let copy = DesktopRecoveredDraft {
			scope: Some(profile.draft_scope_key()),
			draft: DesktopProfileDraft {
				composer: DesktopComposerDraft {
					text: "Recovered earlier text".into(),
					work_id: Some(decodex_protocol::EntityId::new("original-work").unwrap()),
					thread_id: Some(decodex_protocol::WireText::new("original-thread").unwrap()),
					..Default::default()
				},
				..Default::default()
			},
		};
		let mut doc = DesktopDraftDocument::default();
		doc.recovered.push(copy.clone());
		store.save(0, &doc.encode().unwrap()).unwrap();
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual, |s, cx| {
			s.draft_profiles.storage = Storage::open(Ok(store.clone()));
			s.bind_profile(Some(other), cx);
			assert!(s.recovered_drafts().is_empty());
			s.restore_draft_copy(copy, cx);
			assert!(s.draft_profiles.storage.task.is_none());
			s.bind_profile(Some(profile.clone()), cx);
			s.composer.update(cx, |input, cx| input.set_content("Unsaved current input", cx));
		});
		visual.simulate_resize(gpui::size(gpui::px(1100.0), gpui::px(800.0)));
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		let toggle = visual.debug_bounds("draft-copies-toggle").unwrap();
		visual.simulate_click(toggle.center(), gpui::Modifiers::default());
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		let restore = visual.debug_bounds("draft-copy-restore-0").unwrap();
		visual.simulate_click(restore.center(), gpui::Modifiers::default());
		visual.run_until_parked();
		surface.read_with(visual, |s, cx| {
			assert_eq!(s.composer.read(cx).content(), "Recovered earlier text");
			assert!(s.submission.command.is_none() && !s.sending);
			assert_eq!(s.selected.as_deref(), Some("original-work"));
			assert!(!s.draft_owner_available());
		});
		let saved = DesktopDraftDocument::decode(&store.load().unwrap().payload).unwrap();
		assert_eq!(
			saved.profiles[&profile.draft_scope_key()].composer.text,
			"Recovered earlier text"
		);
		assert!(
			saved
				.recovered
				.iter()
				.any(|saved| saved.draft.composer.text == "Unsaved current input")
		);
		surface.update(visual, |s, cx| {
			let backup = s
				.recovered_drafts()
				.into_iter()
				.find(|copy| copy.draft.composer.text == "Unsaved current input")
				.unwrap();
			s.restore_draft_copy(backup, cx);
			s.composer.update(cx, |input, cx| input.set_content("Typed during restore", cx));
		});
		visual.run_until_parked();
		surface.read_with(visual, |s, cx| {
			assert_eq!(s.composer.read(cx).content(), "Typed during restore");
			assert!(s.submission.command.is_none());
		});
		let saved = DesktopDraftDocument::decode(&store.load().unwrap().payload).unwrap();
		assert_eq!(
			saved.profiles[&profile.draft_scope_key()].composer.text,
			"Typed during restore"
		);
		assert!(
			saved.recovered.iter().any(|copy| copy.draft.composer.text == "Unsaved current input")
		);
	}

	#[gpui::test]
	fn keep_both_preserves_later_edits_and_refreshes_inactive_profiles(
		cx: &mut gpui::TestAppContext,
	) {
		let (_service, first, second) = super::super::super::tests::profiles();
		let directory = tempfile::tempdir().unwrap();
		let store =
			ClientDraftStore::open_at(&directory.path().canonicalize().unwrap().join("desktop"))
				.unwrap();
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual, |s, cx| {
			s.draft_profiles.storage = Storage::open(Ok(store.clone()));
			s.bind_profile(Some(second.clone()), cx);
			s.composer.update(cx, |input, cx| input.set_content("old inactive", cx));
			s.bind_profile(Some(first.clone()), cx);
			s.composer.update(cx, |input, cx| input.set_content("base active", cx));
			s.save_draft_document(cx);
		});
		visual.run_until_parked();
		let original = store.load().unwrap();
		let mut disk = DesktopDraftDocument::decode(&original.payload).unwrap();
		let remote = disk.profiles.get_mut(&first.draft_scope_key()).unwrap();
		remote.composer.text = "other window".into();
		remote.uncertain = true;
		remote.unconfirmed_commands.push(IdempotencyKey::new("remote-command").unwrap());
		disk.profiles.get_mut(&second.draft_scope_key()).unwrap().composer.text =
			"new inactive".into();
		store.save(original.revision, &disk.encode().unwrap()).unwrap();
		surface.update(visual, |s, cx| {
			s.composer.update(cx, |input, cx| input.set_content("my current draft", cx));
			s.save_draft_document(cx);
		});
		visual.run_until_parked();
		surface.update(visual, |s, cx| {
			assert!(s.can_keep_both_drafts());
			s.keep_both_drafts(cx);
			s.composer.update(cx, |input, cx| input.set_content("edited during recovery", cx));
		});
		visual.run_until_parked();
		surface.update(visual, |s, cx| {
			assert_eq!(s.composer.read(cx).content(), "edited during recovery");
			assert!(s.uncertain);
			assert_eq!(s.submission.unconfirmed[0].as_str(), "remote-command");
			assert!(s.draft_storage_notice().is_none());
			assert!(s.submission.command.is_none());
			s.bind_profile(Some(second), cx);
			assert_eq!(s.composer.read(cx).content(), "new inactive");
		});
		let saved = DesktopDraftDocument::decode(&store.load().unwrap().payload).unwrap();
		assert_eq!(
			saved.profiles[&first.draft_scope_key()].composer.text,
			"edited during recovery"
		);
		assert!(saved.recovered.iter().any(|copy| copy.draft.composer.text == "other window"));
	}

	#[gpui::test]
	fn keep_both_resolves_seed_conflict_without_repeating_it_after_restart(
		cx: &mut gpui::TestAppContext,
	) {
		let (_service, profile, _) = super::super::super::tests::profiles();
		let directory = tempfile::tempdir().unwrap();
		let store =
			ClientDraftStore::open_at(&directory.path().canonicalize().unwrap().join("desktop"))
				.unwrap();
		let mut doc = DesktopDraftDocument::default();
		doc.profiles.insert(
			profile.draft_scope_key(),
			DesktopProfileDraft {
				composer: DesktopComposerDraft {
					text: "existing saved draft".into(),
					..Default::default()
				},
				..Default::default()
			},
		);
		store.save(0, &doc.encode().unwrap()).unwrap();
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual, |s, cx| {
			s.draft_profiles.storage = Storage::open(Ok(store.clone()));
			s.composer.update(cx, |input, cx| input.set_content("new seed", cx));
			s.bind_profile(Some(profile.clone()), cx);
			assert!(s.can_keep_both_drafts());
		});
		visual.simulate_resize(gpui::size(gpui::px(1000.0), gpui::px(700.0)));
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		let button = visual
			.debug_bounds("draft-keep-both")
			.expect("visible recovery action before a task exists");
		visual.simulate_click(button.center(), gpui::Modifiers::default());
		visual.run_until_parked();
		let saved = DesktopDraftDocument::decode(&store.load().unwrap().payload).unwrap();
		assert!(saved.unbound.text.is_empty());
		assert_eq!(saved.profiles[&profile.draft_scope_key()].composer.text, "new seed");
		assert!(
			saved.recovered.iter().any(|copy| copy.draft.composer.text == "existing saved draft")
		);
		let (reopened, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		reopened.update(visual, |s, cx| {
			s.draft_profiles.storage = Storage::open(Ok(store));
			s.restore_unbound_draft(cx);
			s.bind_profile(Some(profile), cx);
			assert_eq!(s.composer.read(cx).content(), "new seed");
			assert!(s.draft_storage_notice().is_none());
		});
	}
}
