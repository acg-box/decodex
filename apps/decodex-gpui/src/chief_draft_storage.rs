//! Local cold recovery and background publication of unsent desktop inputs.
use super::*;
use decodex_protocol::{
	ClientDraftStore, DesktopComposerDraft, DesktopDraftDocument, DesktopPendingDraft,
	DesktopProfileDraft,
};
use std::time::Duration;

#[path = "chief_ordinary_storage.rs"] mod ordinary;
#[path = "chief_draft_recovery.rs"] mod recovery;

enum SaveFailure {
	Conflict,
	Invalid(&'static str),
	Busy,
	Failed,
}

pub(super) struct Storage {
	store: Option<ClientDraftStore>,
	document: DesktopDraftDocument,
	saved: DesktopDraftDocument,
	revision: u64,
	task: Option<Task<()>>,
	error: Option<String>,
	busy: bool,
	quitting: bool,
	reconcilable: bool,
	seeded: bool,
	show_recovered: bool,
	remove_candidate: Option<decodex_protocol::DesktopRecoveredDraft>,
}
impl Default for Storage {
	fn default() -> Self {
		#[cfg(not(test))]
		{
			Self::open(ClientDraftStore::open_default())
		}
		#[cfg(test)]
		{
			Self::empty()
		}
	}
}
impl Storage {
	pub(super) fn clear_unbound(&mut self) {
		self.document.unbound = Default::default();
	}

	pub(super) fn restore(&self, scope: &str, epoch: u64) -> Option<Drafts> {
		self.document.profiles.get(scope).cloned().map(|saved| Drafts::from_document(saved, epoch))
	}

	pub(super) fn seed_conflict(&mut self) {
		self.reconcilable = true;
		self.seeded = true;
		self.error = Some("A saved draft and new input both exist. Current input remains in memory; the saved draft is unchanged.".into());
	}

	fn empty() -> Self {
		Self {
			store: None,
			document: Default::default(),
			saved: Default::default(),
			revision: 0,
			task: None,
			error: None,
			busy: false,
			quitting: false,
			reconcilable: false,
			seeded: false,
			show_recovered: false,
			remove_candidate: None,
		}
	}

	fn open(store: Result<ClientDraftStore, decodex_protocol::ClientDraftError>) -> Self {
		let mut state = Self::empty();
		let result = (|| {
			let store = store.map_err(|error| error.to_string())?;
			let snapshot = store.load().map_err(|error| error.to_string())?;
			let document = if snapshot.revision == 0 {
				DesktopDraftDocument::default()
			} else {
				DesktopDraftDocument::decode(&snapshot.payload).map_err(str::to_owned)?
			};
			state.revision = snapshot.revision;
			state.saved = document.clone();
			state.document = document;
			state.store = Some(store);
			Ok::<(), String>(())
		})();
		if result.is_err() {
			state.error =
				Some("Saved drafts could not be read. Current edits remain in memory.".into());
		}
		state
	}
}

impl ChiefSurface {
	pub(in super::super) fn renew_prompt_editor(
		&mut self,
		previous: &decodex_protocol::DesktopPromptEditDraft,
		fresh: &decodex_protocol::DesktopPromptEditDraft,
		cx: &mut Context<Self>,
	) -> Result<decodex_protocol::DesktopPromptEditDraft, &'static str> {
		if self.selected.as_deref() != Some(previous.work_id.as_str())
			|| !self.saved_prompt_editors(previous.work_id.as_str()).contains(previous)
		{
			return Err("Draft changed while history was being reviewed");
		}
		let renewed = previous.refresh_review(fresh)?;
		let scope =
			self.profile.as_ref().ok_or("Service profile is unavailable")?.draft_scope_key();
		self.remember_draft_document(cx);
		let mut next = self.draft_profiles.storage.document.clone();
		let editors =
			&mut next.profiles.get_mut(&scope).ok_or("Draft profile is unavailable")?.prompt_edits;
		if editors
			.get(renewed.review_token.as_str())
			.is_some_and(|other| other != previous && other != &renewed)
		{
			return Err("Another draft already uses this review");
		}
		editors.remove(previous.review_token.as_str());
		editors.insert(renewed.review_token.as_str().into(), renewed.clone());
		next.encode()?;
		self.draft_profiles.storage.document = next;
		self.save_draft_document(cx);
		Ok(renewed)
	}

	pub(in super::super) fn saved_prompt_editors(
		&self,
		work: &str,
	) -> Vec<decodex_protocol::DesktopPromptEditDraft> {
		let Some(profile) = &self.profile else {
			return Vec::new();
		};
		let Some(thread) = self
			.snapshot
			.as_ref()
			.and_then(|snapshot| snapshot.work_items.iter().find(|item| item.id == work))
			.and_then(|item| item.codex_thread_id.as_deref())
		else {
			return Vec::new();
		};
		self.draft_profiles
			.storage
			.document
			.profiles
			.get(&profile.draft_scope_key())
			.into_iter()
			.flat_map(|profile| profile.prompt_edits.values())
			.filter(|draft| draft.work_id.as_str() == work && draft.thread_id.as_str() == thread)
			.cloned()
			.collect()
	}

	pub(in super::super) fn discard_prompt_editor(
		&mut self,
		draft: &decodex_protocol::DesktopPromptEditDraft,
		cx: &mut Context<Self>,
	) -> Result<(), &'static str> {
		if self.selected.as_deref() != Some(draft.work_id.as_str())
			|| !self.saved_prompt_editors(draft.work_id.as_str()).contains(draft)
		{
			return Err("Draft source changed");
		}
		if draft.handback_pending || draft.receipt_id.is_some() {
			return Err("Recover the history edit before discarding its draft");
		}
		let scope =
			self.profile.as_ref().ok_or("Service profile is unavailable")?.draft_scope_key();
		let saved = self
			.draft_profiles
			.storage
			.document
			.profiles
			.get_mut(&scope)
			.ok_or("Draft profile is unavailable")?;
		if saved.prompt_edits.get(draft.review_token.as_str()) != Some(draft) {
			return Err("Saved draft changed");
		}
		saved.prompt_edits.remove(draft.review_token.as_str());
		self.save_draft_document(cx);
		Ok(())
	}

	pub(in super::super) fn stage_prompt_editor(
		&mut self,
		draft: decodex_protocol::DesktopPromptEditDraft,
		cx: &mut Context<Self>,
	) -> Result<(), &'static str> {
		let scope =
			self.profile.as_ref().ok_or("Service profile is unavailable")?.draft_scope_key();
		if self.draft_profiles.active.as_ref().map(ClientProfile::draft_scope_key)
			!= Some(scope.clone())
		{
			return Err("Draft profile changed");
		}
		self.remember_draft_document(cx);
		let mut next = self.draft_profiles.storage.document.clone();
		next.profiles
			.entry(scope)
			.or_default()
			.prompt_edits
			.insert(draft.review_token.as_str().into(), draft);
		next.encode()?;
		self.draft_profiles.storage.document = next;
		self.save_draft_document(cx);
		Ok(())
	}

	pub(in super::super) fn restore_unbound_draft(&mut self, cx: &mut Context<Self>) {
		let saved = self.draft_profiles.storage.document.unbound.clone();
		self.restore_creation_setup(saved.creation.as_ref(), cx);
		self.composer.update(cx, |input, cx| input.set_content(&saved.text, cx));
		self.attachments = saved.attachments.clone();
		self.task_references = saved.references.clone();
	}

	pub(crate) fn drafts_ready_for_quit(&mut self, cx: &mut Context<Self>) -> bool {
		self.save_draft_document(cx);
		let state = &self.draft_profiles.storage;
		state.error.is_none() && state.task.is_none() && state.document == state.saved
	}

	pub(in super::super) fn draft_quit_in_progress(&self) -> bool {
		self.draft_profiles.storage.quitting
	}

	pub(crate) fn flush_drafts_for_quit(&mut self, cx: &mut Context<Self>) -> Task<bool> {
		self.cancel_queued_command(cx);
		self.draft_profiles.storage.quitting = true;
		self.save_draft_document(cx);
		let retained = cx.entity();
		cx.spawn(async move |_, cx| {
			let deadline = std::time::Instant::now() + Duration::from_secs(5);
			loop {
				let result = retained.update(cx, |surface, cx| {
					surface.save_draft_document(cx);
					let storage = &mut surface.draft_profiles.storage;
					let result = if storage.error.is_some() || std::time::Instant::now() >= deadline
					{
						Some(false)
					} else if storage.task.is_none() && storage.document == storage.saved {
						Some(true)
					} else {
						None
					};
					if result.is_some() {
						storage.quitting = false;
						if result == Some(false) {
							surface.feedback =
								"Quit canceled: drafts are not saved. Keep this window open."
									.into();
						}
						cx.notify();
					}
					result
				});
				if let Some(saved) = result {
					return saved;
				}
				cx.background_executor().timer(Duration::from_millis(50)).await;
			}
		})
	}

	pub(in super::super) fn draft_storage_notice(&self) -> Option<&str> {
		self.draft_profiles.storage.error.as_deref().or_else(|| {
			self.draft_profiles
				.storage
				.busy
				.then_some("Waiting to save drafts. Keep this window open.")
		})
	}

	pub(super) fn remember_draft_document(&mut self, cx: &Context<Self>) {
		let Some(profile) = self.draft_profiles.active.as_ref() else {
			if self.composer_manager.is_some() {
				self.draft_profiles.storage.error = Some(
					"Draft service identity is unavailable. Current edits remain in memory.".into(),
				);
				return;
			}
			self.draft_profiles.storage.document.unbound = DesktopComposerDraft {
				creation: self.creation_setup(cx),
				text: self.composer.read(cx).content().into(),
				attachments: self.attachments.clone(),
				references: self.task_references.clone(),
				..Default::default()
			};
			return;
		};
		let scope = profile.draft_scope_key();
		match self.capture_draft_document(cx) {
			Some(draft) => {
				for composer in std::iter::once(&draft.composer).chain(draft.parked.values()) {
					if let (Some(work), Some(thread)) = (&composer.work_id, &composer.thread_id) {
						self.draft_profiles
							.threads
							.entry(work.as_str().into())
							.or_insert_with(|| thread.as_str().into());
					}
				}
				self.draft_profiles.storage.document.profiles.insert(scope, draft);
			},
			None =>
				self.draft_profiles.storage.error =
					Some("Draft identity is unavailable. Current edits remain in memory.".into()),
		}
	}

	pub(in super::super) fn command_draft_copy(
		&self,
		cx: &Context<Self>,
	) -> Result<decodex_protocol::DesktopRecoveredDraft, &'static str> {
		let scope = self
			.draft_profiles
			.active
			.as_ref()
			.ok_or("Draft service identity is unavailable.")?
			.draft_scope_key();
		let mut draft = self.capture_draft_document(cx).ok_or("Draft identity is unavailable.")?;
		// Capture the original source and choices before asynchronous dispatch.
		// Other editors remain in the current document, not in this alternative.
		draft.parked.clear();
		draft.questions.clear();
		let copy = decodex_protocol::DesktopRecoveredDraft { scope: Some(scope), draft };
		let mut prospective = self.draft_profiles.storage.document.clone();
		if !prospective.recovered.contains(&copy) {
			prospective.recovered.push(copy.clone());
		}
		prospective.encode().map_err(
			|_| "Free space in saved draft copies before sending. Current input is retained.",
		)?;
		Ok(copy)
	}

	pub(in super::super) fn fence_command_draft(&mut self, pending: &mut PendingCommand) {
		let Some(copy) = pending.recovery.as_mut() else { return };
		copy.draft.uncertain = true;
		if let Some(key) = &pending.key {
			copy.draft.unconfirmed_commands.push(key.clone());
		}
		self.draft_profiles.storage.document.recovered.push(copy.clone());
	}

	pub(in super::super) fn resolve_steer_draft_copies(
		&mut self,
		identity: &decodex_protocol::ChiefSteerIdentity,
	) {
		let scope = self.draft_profiles.active.as_ref().map(ClientProfile::draft_scope_key);
		for copy in &mut self.draft_profiles.storage.document.recovered {
			if copy.scope != scope
				|| copy.draft.composer.work_id.as_ref() != Some(&identity.work_id)
				|| copy.draft.composer.thread_id.as_ref() != Some(&identity.thread_id)
				|| !copy.draft.unconfirmed_commands.contains(&identity.submission_id)
			{
				continue;
			}
			copy.draft.unconfirmed_commands.retain(|key| key != &identity.submission_id);
			if copy.draft.pending.as_ref().and_then(|pending| pending.steer.as_ref())
				== Some(identity)
			{
				copy.draft.pending = None;
			}
			copy.draft.uncertain =
				!copy.draft.unconfirmed_commands.is_empty() || copy.draft.pending.is_some();
		}
	}

	pub(in super::super) fn remove_command_draft_fence(&mut self, pending: &PendingCommand) {
		if let Some(copy) = &pending.recovery {
			self.draft_profiles.storage.document.recovered.retain(|saved| saved != copy);
		}
	}

	pub(in super::super) fn retain_failed_command_draft(
		&mut self,
		pending: &PendingCommand,
		uncertain: bool,
		cx: &Context<Self>,
	) {
		let Some(mut copy) = pending.recovery.clone() else { return };
		let current = self.capture_draft_document(cx);
		let same_editor = current.as_ref().and_then(|draft| {
			if draft.composer.work_id == copy.draft.composer.work_id {
				Some(&draft.composer)
			} else {
				copy.draft.composer.work_id.as_ref().and_then(|id| draft.parked.get(id.as_str()))
			}
		}) == Some(&copy.draft.composer);
		if same_editor
			&& current.as_ref().is_some_and(|draft| draft.execution == copy.draft.execution)
		{
			return;
		}
		copy.draft.uncertain = uncertain;
		copy.draft.unconfirmed_commands.retain(|key| Some(key) != pending.key.as_ref());
		if uncertain && let Some(key) = &pending.key {
			copy.draft.unconfirmed_commands.push(key.clone());
		}
		let storage = &mut self.draft_profiles.storage;
		if !storage.document.recovered.contains(&copy) {
			storage.document.recovered.push(copy);
		}
		storage.show_recovered = true;
		// The ordinary save reports capacity/conflict errors and keeps both copies
		// in memory. It must not discard the original to make a write fit.
	}

	fn capture_draft_document(&self, cx: &Context<Self>) -> Option<DesktopProfileDraft> {
		let composer = |owner: Option<&str>, text: String, attachments, references| {
			let work_id = owner.map(EntityId::new).transpose().ok()?;
			let thread_id = owner
				.and_then(|owner| {
					self.draft_profiles.threads.get(owner).cloned().or_else(|| {
						self.snapshot
							.as_ref()?
							.work_items
							.iter()
							.find(|work| work.id == owner)?
							.codex_thread_id
							.clone()
					})
				})
				.map(WireText::new)
				.transpose()
				.ok()?;
			Some(DesktopComposerDraft {
				creation: if owner.is_none() { self.creation_setup(cx) } else { None },
				work_id,
				thread_id,
				text,
				attachments,
				references,
			})
		};
		let mut parked = BTreeMap::new();
		let owners: std::collections::BTreeSet<_> = self
			.draft_profiles
			.texts
			.keys()
			.chain(self.draft_profiles.files.keys())
			.chain(self.draft_profiles.tasks.keys())
			.collect();
		for owner in owners {
			parked.insert(
				owner.clone(),
				composer(
					Some(owner),
					self.draft_profiles.texts.get(owner).cloned().unwrap_or_default(),
					self.draft_profiles.files.get(owner).cloned().unwrap_or_default(),
					self.draft_profiles.tasks.get(owner).cloned().unwrap_or_default(),
				)?,
			);
		}
		let (execution_revision, execution) = self.draft_profiles.execution.saved_choices();
		let pending = self
			.submission
			.pending
			.as_ref()
			.map(|pending| {
				Some(DesktopPendingDraft {
					steer: pending.steer.clone(),
					owner: pending.owner.as_deref().map(EntityId::new).transpose().ok()?,
					text: pending.draft.clone(),
					attachments: pending.attachments.clone(),
					references: pending.references.clone(),
					execution: pending
						.execution_intent
						.as_ref()
						.map(|(owner, revision)| Some((EntityId::new(owner).ok()?, *revision)))
						.transpose_option()?,
				})
			})
			.transpose_option()?;
		Some(DesktopProfileDraft {
			prompt_edits: self
				.draft_profiles
				.active
				.as_ref()
				.and_then(|profile| {
					self.draft_profiles.storage.document.profiles.get(&profile.draft_scope_key())
				})
				.map(|draft| draft.prompt_edits.clone())
				.unwrap_or_default(),
			ordinary: self
				.draft_profiles
				.active
				.as_ref()
				.and_then(|profile| {
					self.draft_profiles.storage.document.profiles.get(&profile.draft_scope_key())
				})
				.map(|draft| draft.ordinary.clone())
				.unwrap_or_default(),
			unconfirmed_commands: self.submission.unconfirmed.clone(),
			composer: composer(
				self.composer_manager.as_deref(),
				self.composer.read(cx).content().into(),
				self.attachments.clone(),
				self.task_references.clone(),
			)?,
			parked,
			execution,
			execution_revision,
			questions: self.capture_async_drafts(cx)?,
			uncertain: self.uncertain || self.sending || pending.is_some(),
			pending,
		})
	}

	pub(in super::super) fn save_draft_document(&mut self, cx: &mut Context<Self>) {
		self.remember_draft_document(cx);
		if self.draft_profiles.storage.store.is_some()
			&& self
				.submission
				.waiting
				.as_ref()
				.is_some_and(|queued| self.draft_profiles.active.as_ref() != Some(&queued.profile))
		{
			self.draft_profiles.storage.error =
				Some("Draft service identity is unavailable. Nothing was sent.".into());
		}
		let state = &mut self.draft_profiles.storage;
		if state.task.is_some() {
			return;
		}
		if state.error.is_some() {
			if let Some(queued) = self.submission.waiting.take() {
				self.finish_command(
					queued.pending,
					Err("Draft could not be saved. Nothing was sent.".into()),
					cx,
				);
			}
			return;
		}
		if state.document == state.saved {
			if let Some(queued) = self.submission.waiting.take() {
				self.dispatch_saved_command(queued, cx);
			}
			return;
		}
		let Some(store) = state.store.clone() else {
			#[cfg(test)]
			if let Some(queued) = self.submission.waiting.take() {
				self.dispatch_saved_command(queued, cx);
			}
			return;
		};
		let document = state.document.clone();
		let saved = document.clone();
		let revision = state.revision;
		let write = cx
			.background_executor()
			.spawn(async move { publish_document(&store, revision, &document) });
		state.task = Some(cx.spawn(async move |surface, cx| {
			let result = write.await;
			let _ = surface.update(cx, |surface, cx| {
				let state = &mut surface.draft_profiles.storage;
				state.task = None;
				match result {
					Ok(revision) => {
						state.busy = false;
						state.revision = revision;
						state.saved = saved;
					},
					Err(SaveFailure::Busy) => {
						state.busy = true;
						cx.notify();
						return;
					},
					Err(SaveFailure::Conflict) => {
						state.reconcilable = true;
						state.error = Some(
							"Another window saved different drafts. Keep both copies to continue."
								.into(),
						);
					},
					Err(SaveFailure::Invalid(reason)) => state.error = Some(reason.into()),
					Err(SaveFailure::Failed) => state.error = Some(
						"Drafts could not be saved. Keep this window open to retain current edits."
							.into(),
					),
				}
				surface.save_draft_document(cx);
				cx.notify();
			});
		}));
	}
}

fn publish_document(
	store: &ClientDraftStore,
	revision: u64,
	document: &DesktopDraftDocument,
) -> Result<u64, SaveFailure> {
	let bytes = document.encode().map_err(SaveFailure::Invalid)?;
	match store.save(revision, &bytes) {
		Ok(revision) => Ok(revision),
		Err(decodex_protocol::ClientDraftError::WriteUnconfirmed(_)) => {
			let actual = store.load().map_err(|_| SaveFailure::Failed)?;
			if actual.payload == bytes { Ok(actual.revision) } else { Err(SaveFailure::Failed) }
		},
		Err(decodex_protocol::ClientDraftError::Busy) => Err(SaveFailure::Busy),
		Err(decodex_protocol::ClientDraftError::Conflict) => Err(SaveFailure::Conflict),
		Err(_) => Err(SaveFailure::Failed),
	}
}

impl Drafts {
	fn from_document(saved: DesktopProfileDraft, epoch: u64) -> Self {
		let mut result = Self {
			creation: saved.composer.creation,
			unconfirmed: saved.unconfirmed_commands,
			text: saved.composer.text,
			manager: saved.composer.work_id.map(|id| id.as_str().into()),
			attachments: saved.composer.attachments,
			references: saved.composer.references,
			uncertain: saved.uncertain,
			restored_questions: saved.questions,
			execution: execution_intent::Intents::from_saved(
				saved.execution_revision,
				saved.execution,
			),
			..Default::default()
		};
		if let (Some(owner), Some(thread)) = (&result.manager, saved.composer.thread_id) {
			result.threads.insert(owner.clone(), thread.as_str().into());
		}
		for (owner, draft) in saved.parked {
			result.texts.insert(owner.clone(), draft.text);
			result.files.insert(owner.clone(), draft.attachments);
			result.tasks.insert(owner.clone(), draft.references);
			if let Some(thread) = draft.thread_id {
				result.threads.insert(owner, thread.as_str().into());
			}
		}
		result.steer_pending = saved.pending.map(|pending| PendingCommand {
			recovery: None,
			key: pending.steer.as_ref().map(|steer| steer.submission_id.clone()),
			steer: pending.steer,
			epoch,
			execution_intent: pending
				.execution
				.map(|(owner, revision)| (owner.as_str().into(), revision)),
			draft: pending.text,
			owner: pending.owner.map(|id| id.as_str().into()),
			attachments: pending.attachments,
			references: pending.references,
		});
		if result.uncertain {
			result.feedback = "Previous submission acceptance is unknown. Inspect the conversation before sending again.".into();
		}
		result
	}
}

// Preserve absence separately from a failed bounded conversion.
trait TransposeOption<T> {
	fn transpose_option(self) -> Option<Option<T>>;
}
impl<T> TransposeOption<T> for Option<Option<T>> {
	fn transpose_option(self) -> Self {
		match self {
			None => Some(None),
			Some(value) => value.map(Some),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[gpui::test]
	fn exact_receipt_settles_only_matching_saved_copies(cx: &mut gpui::TestAppContext) {
		let (_service, profile, other) = super::super::tests::profiles();
		let surface = cx.new(ChiefSurface::new);
		surface.update(cx, |s, cx| {
			s.bind_profile(Some(profile.clone()), cx);
			let identity = decodex_protocol::ChiefSteerIdentity {
				work_id: EntityId::new("work").unwrap(),
				thread_id: WireText::new("thread").unwrap(),
				turn_id: WireText::new("turn").unwrap(),
				submission_id: IdempotencyKey::new("confirmed").unwrap(),
			};
			let mut draft = DesktopProfileDraft::default();
			draft.composer.work_id = Some(identity.work_id.clone());
			draft.composer.thread_id = Some(identity.thread_id.clone());
			draft.composer.text = "Retain this copy".into();
			draft.uncertain = true;
			draft.unconfirmed_commands = vec![identity.submission_id.clone()];
			let copy = decodex_protocol::DesktopRecoveredDraft {
				scope: Some(profile.draft_scope_key()),
				draft,
			};
			let mut foreign = copy.clone();
			foreign.scope = Some(other.draft_scope_key());
			let mut additional = copy.clone();
			additional.draft.unconfirmed_commands.push(IdempotencyKey::new("unknown").unwrap());
			s.draft_profiles.storage.document.recovered =
				vec![copy.clone(), foreign.clone(), additional];
			s.resolve_steer_draft_copies(&identity);
			let copies = &s.draft_profiles.storage.document.recovered;
			assert_eq!(copies[0].draft.composer.text, "Retain this copy");
			assert!(!copies[0].draft.uncertain && copies[0].draft.unconfirmed_commands.is_empty());
			assert!(copies[1] == foreign, "another service retains its uncertainty");
			assert!(copies[2].draft.uncertain, "another submission remains unconfirmed");
			assert_eq!(copies[2].draft.unconfirmed_commands[0].as_str(), "unknown");
		});
	}

	#[gpui::test]
	fn cold_reopen_keeps_inflight_original_after_later_edit(cx: &mut gpui::TestAppContext) {
		let (_service, profile, _) = super::super::tests::profiles();
		let directory = tempfile::tempdir().unwrap();
		let store =
			ClientDraftStore::open_at(&directory.path().canonicalize().unwrap().join("desktop"))
				.unwrap();
		let surface = cx.new(ChiefSurface::new);
		let key = IdempotencyKey::new("inflight-original").unwrap();
		surface.update(cx, |s, cx| {
			s.draft_profiles.storage = Storage::open(Ok(store.clone()));
			s.bind_profile(Some(profile.clone()), cx);
			s.composer.update(cx, |input, cx| input.set_content("Original in flight", cx));
			let mut pending = PendingCommand {
				recovery: Some(s.command_draft_copy(cx).unwrap()),
				key: Some(key.clone()),
				steer: None,
				epoch: s.command_epoch,
				execution_intent: None,
				draft: Some("Original in flight".into()),
				owner: None,
				attachments: Some(vec![]),
				references: Some(vec![]),
			};
			s.fence_command_draft(&mut pending);
			s.submission.unconfirmed.push(key.clone());
			s.sending = true;
			s.composer.update(cx, |input, cx| input.set_content("Later unsent edit", cx));
			s.remember_draft_document(cx);
			publish_document(&store, 0, &s.draft_profiles.storage.document)
				.unwrap_or_else(|_| panic!("saved"));
		});
		let reopened = cx.new(ChiefSurface::new);
		reopened.update(cx, |s, cx| {
			s.draft_profiles.storage = Storage::open(Ok(store.clone()));
			s.bind_profile(Some(profile), cx);
			assert_eq!(s.composer.read(cx).content(), "Later unsent edit");
			assert!(s.uncertain && !s.sending && s.submission.command.is_none());
			let copies = s.recovered_drafts();
			assert_eq!(copies.len(), 1);
			assert_eq!(copies[0].draft.composer.text, "Original in flight");
			assert!(copies[0].draft.uncertain);
			assert_eq!(copies[0].draft.unconfirmed_commands, vec![key]);
		});
	}

	#[gpui::test]
	fn failed_send_keeps_original_copy_and_new_editor_after_reopen(cx: &mut gpui::TestAppContext) {
		let (_service, profile, _) = super::super::tests::profiles();
		for unknown in [false, true] {
			let directory = tempfile::tempdir().unwrap();
			let root = directory.path().canonicalize().unwrap().join("desktop");
			let store = ClientDraftStore::open_at(&root).unwrap();
			let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
			surface.update(visual, |s, cx| {
				s.draft_profiles.storage = Storage::open(Ok(store.clone()));
				s.bind_profile(Some(profile.clone()), cx);
				s.composer_manager = Some("original-work".into());
				s.draft_profiles.threads.insert("original-work".into(), "original-thread".into());
				s.composer.update(cx, |input, cx| input.set_content("Original failed message", cx));
				let pending = PendingCommand {
					recovery: Some(s.command_draft_copy(cx).unwrap()),
					key: Some(IdempotencyKey::new("failed-message").unwrap()),
					steer: None,
					epoch: s.command_epoch,
					execution_intent: None,
					draft: Some("Original failed message".into()),
					owner: Some("original-work".into()),
					attachments: Some(vec![]),
					references: Some(vec![]),
				};
				s.composer.update(cx, |input, cx| input.set_content("Newer editor", cx));
				// The late result must retain the thread captured before dispatch.
				s.draft_profiles
					.threads
					.insert("original-work".into(), "replacement-thread".into());
				s.finish_command(
					pending,
					if unknown {
						Ok(ChiefCommandResponse::PotentiallyDispatched {
							failure: decodex_protocol::ClientFailure::ProtocolTimeout,
						})
					} else {
						Err("Fixture dispatch failed".into())
					},
					cx,
				);
				assert_eq!(s.composer.read(cx).content(), "Newer editor");
				assert!(s.show_recovered_drafts());
			});
			visual.run_until_parked();
			let saved = DesktopDraftDocument::decode(&store.load().unwrap().payload).unwrap();
			assert_eq!(saved.profiles[&profile.draft_scope_key()].composer.text, "Newer editor");
			assert_eq!(saved.recovered.len(), 1);
			let copy = &saved.recovered[0];
			assert_eq!(copy.scope, Some(profile.draft_scope_key()));
			assert_eq!(copy.draft.composer.text, "Original failed message");
			assert_eq!(copy.draft.composer.thread_id.as_ref().unwrap().as_str(), "original-thread");
			assert_eq!(copy.draft.uncertain, unknown);
			assert_eq!(copy.draft.unconfirmed_commands.len(), usize::from(unknown));
			if unknown {
				assert!(saved.remove_recovered_copy(copy).is_err());
			}
		}
	}

	#[gpui::test]
	fn cold_unbound_draft_reopens_and_moves_only_to_first_profile(cx: &mut gpui::TestAppContext) {
		let (_service, profile, other) = super::super::tests::profiles();
		let directory = tempfile::tempdir().unwrap();
		let root = directory.path().canonicalize().unwrap().join("desktop");
		let store = ClientDraftStore::open_at(&root).unwrap();
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual, |s, cx| {
			s.draft_profiles.storage = Storage::open(Ok(store.clone()));
			s.composer.update(cx, |input, cx| input.set_content("Before choosing a service", cx));
			s.attachments.push(decodex_protocol::ChiefAttachmentDto {
				path: ConversationWorkingDirectory::new("/tmp/local.png").unwrap(),
				image: true,
			});
			assert!(!s.drafts_ready_for_quit(cx));
		});
		visual.run_until_parked();
		let saved = DesktopDraftDocument::decode(&store.load().unwrap().payload).unwrap();
		assert_eq!(saved.unbound.text, "Before choosing a service");
		assert!(saved.profiles.is_empty());
		let (restored, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		restored.update(visual, |s, cx| {
			s.draft_profiles.storage = Storage::open(ClientDraftStore::open_at(&root));
			s.restore_unbound_draft(cx);
			assert_eq!(s.composer.read(cx).content(), saved.unbound.text);
			assert_eq!(s.attachments, saved.unbound.attachments);
			assert!(s.profile.is_none() && s.composer_manager.is_none());
			s.bind_profile(Some(profile.clone()), cx);
			s.save_draft_document(cx);
		});
		visual.run_until_parked();
		let saved = DesktopDraftDocument::decode(&store.load().unwrap().payload).unwrap();
		assert!(saved.unbound.text.is_empty() && saved.unbound.attachments.is_empty());
		assert_eq!(
			saved.profiles[&profile.draft_scope_key()].composer.text,
			"Before choosing a service"
		);
		restored.update(visual, |s, cx| {
			s.bind_profile(Some(other), cx);
			assert!(s.composer.read(cx).content().is_empty());
			assert!(s.attachments.is_empty());
			s.bind_profile(Some(profile), cx);
			assert_eq!(s.composer.read(cx).content(), "Before choosing a service");
			assert!(s.submission.command.is_none());
		});
	}

	#[gpui::test]
	fn cold_quit_flush_saves_latest_edit_and_checks_again_before_exit(
		cx: &mut gpui::TestAppContext,
	) {
		let (_service, profile, _) = super::super::tests::profiles();
		let directory = tempfile::tempdir().unwrap();
		let store =
			ClientDraftStore::open_at(&directory.path().canonicalize().unwrap().join("desktop"))
				.unwrap();
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		let flush = surface.update(visual, |s, cx| {
			s.draft_profiles.storage = Storage::open(Ok(store.clone()));
			s.bind_profile(Some(profile.clone()), cx);
			s.composer.update(cx, |input, cx| input.set_content("First edit", cx));
			s.save_draft_document(cx);
			s.composer.update(cx, |input, cx| input.set_content("Last edit before quit", cx));
			s.flush_drafts_for_quit(cx)
		});
		let outcome = std::rc::Rc::new(std::cell::Cell::new(None));
		let result = outcome.clone();
		visual.spawn(async move |_| result.set(Some(flush.await))).detach();
		visual.run_until_parked();
		visual.executor().advance_clock(Duration::from_millis(100));
		visual.run_until_parked();
		assert_eq!(outcome.get(), Some(true));
		let doc = DesktopDraftDocument::decode(&store.load().unwrap().payload).unwrap();
		assert_eq!(doc.profiles[&profile.draft_scope_key()].composer.text, "Last edit before quit");
		surface.update(visual, |s, cx| {
			assert!(s.drafts_ready_for_quit(cx));
			s.composer.update(cx, |input, cx| input.set_content("Edited after preflight", cx));
			assert!(!s.drafts_ready_for_quit(cx));
		});
		visual.run_until_parked();
	}

	#[gpui::test]
	fn cold_quit_conflict_retains_window_input(cx: &mut gpui::TestAppContext) {
		let (_service, profile, _) = super::super::tests::profiles();
		let directory = tempfile::tempdir().unwrap();
		let store =
			ClientDraftStore::open_at(&directory.path().canonicalize().unwrap().join("desktop"))
				.unwrap();
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		let flush = surface.update(visual, |s, cx| {
			s.draft_profiles.storage = Storage::open(Ok(store.clone()));
			s.bind_profile(Some(profile), cx);
			s.composer.update(cx, |input, cx| input.set_content("Unmerged edit", cx));
			store.save(0, &DesktopDraftDocument::default().encode().unwrap()).unwrap();
			s.flush_drafts_for_quit(cx)
		});
		let outcome = std::rc::Rc::new(std::cell::Cell::new(None));
		let result = outcome.clone();
		visual.spawn(async move |_| result.set(Some(flush.await))).detach();
		visual.run_until_parked();
		visual.executor().advance_clock(Duration::from_millis(100));
		visual.run_until_parked();
		assert_eq!(outcome.get(), Some(false));
		surface.update(visual, |s, cx| {
			assert!(!s.drafts_ready_for_quit(cx));
			assert_eq!(s.composer.read(cx).content(), "Unmerged edit");
			assert!(s.feedback.contains("Quit canceled"));
		});
		assert_eq!(store.load().unwrap().revision, 1);
	}

	#[gpui::test]
	fn cold_accepted_command_publishes_cleanup_without_waiting_for_poll(
		cx: &mut gpui::TestAppContext,
	) {
		let (_service, profile, _) = super::super::tests::profiles();
		let directory = tempfile::tempdir().unwrap();
		let store =
			ClientDraftStore::open_at(&directory.path().canonicalize().unwrap().join("desktop"))
				.unwrap();
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		let key = IdempotencyKey::new("accepted-command").unwrap();
		let pending = surface.update(visual, |s, cx| {
			s.draft_profiles.storage = Storage::open(Ok(store.clone()));
			s.bind_profile(Some(profile.clone()), cx);
			s.composer_manager = Some("work".into());
			s.composer.update(cx, |input, cx| input.set_content("Accepted input", cx));
			let mut pending = PendingCommand {
				recovery: Some(s.command_draft_copy(cx).unwrap()),
				key: Some(key.clone()),
				steer: None,
				epoch: s.command_epoch,
				execution_intent: None,
				draft: Some("Accepted input".into()),
				owner: Some("work".into()),
				attachments: None,
				references: None,
			};
			s.fence_command_draft(&mut pending);
			s.sending = true;
			s.submission.unconfirmed.push(key.clone());
			s.save_draft_document(cx);
			pending
		});
		visual.run_until_parked();
		assert_eq!(store.load().unwrap().revision, 1);
		surface.update(visual, |s, cx| {
			s.finish_command(
				pending,
				Ok(ChiefCommandResponse::Accepted { work_id: EntityId::new("work").unwrap() }),
				cx,
			);
			assert!(s.draft_profiles.storage.task.is_some());
		});
		visual.run_until_parked();
		let snapshot = store.load().unwrap();
		assert_eq!(snapshot.revision, 2);
		let doc = DesktopDraftDocument::decode(&snapshot.payload).unwrap();
		assert!(doc.recovered.is_empty(), "accepted copy is removed with its fence");
		let saved = &doc.profiles[&profile.draft_scope_key()];
		assert!(saved.composer.text.is_empty());
		assert!(saved.unconfirmed_commands.is_empty());
		assert!(!saved.uncertain);
	}

	#[gpui::test]
	fn cold_dispatch_busy_writer_retries_without_dispatch_or_losing_edits(
		cx: &mut gpui::TestAppContext,
	) {
		use std::os::unix::fs::OpenOptionsExt;
		let (_service, profile, _) = super::super::tests::profiles();
		let directory = tempfile::tempdir().unwrap();
		let root = directory.path().canonicalize().unwrap().join("desktop");
		let store = ClientDraftStore::open_at(&root).unwrap();
		let lock = std::fs::OpenOptions::new()
			.create(true)
			.truncate(false)
			.read(true)
			.write(true)
			.mode(0o600)
			.open(root.join("client-drafts/writer.lock"))
			.unwrap();
		lock.try_lock().unwrap();
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual, |s, cx| {
			s.draft_profiles.storage = Storage::open(Ok(store.clone()));
			s.bind_profile(Some(profile.clone()), cx);
			s.state = LoadState::Ready;
			s.execute(
				ChiefActionDto::RefreshIntegrations { work_id: EntityId::new("work").unwrap() },
				None,
				cx,
			);
		});
		visual.run_until_parked();
		surface.update(visual, |s, cx| {
			assert!(s.draft_profiles.storage.busy);
			assert!(s.draft_profiles.storage.error.is_none());
			assert!(s.submission.waiting.is_some() && s.submission.command.is_none());
			s.composer.update(cx, |input, cx| input.set_content("Edit while writer is busy", cx));
		});
		assert_eq!(store.load().unwrap().revision, 0);
		drop(lock);
		surface.update(visual, |s, cx| s.save_draft_document(cx));
		visual.run_until_parked();
		surface.read_with(visual, |s, _| {
			assert!(!s.draft_profiles.storage.busy);
			assert!(s.draft_profiles.storage.error.is_none());
			assert!(s.submission.waiting.is_none());
		});
		let saved = DesktopDraftDocument::decode(&store.load().unwrap().payload).unwrap();
		assert_eq!(
			saved.profiles[&profile.draft_scope_key()].composer.text,
			"Edit while writer is busy"
		);
	}

	#[gpui::test]
	fn cold_dispatch_waits_for_existing_save_and_clears_definite_failure(
		cx: &mut gpui::TestAppContext,
	) {
		let (_service, profile, _) = super::super::tests::profiles();
		let directory = tempfile::tempdir().unwrap();
		let store =
			ClientDraftStore::open_at(&directory.path().canonicalize().unwrap().join("desktop"))
				.unwrap();
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual, |s, cx| {
			s.draft_profiles.storage = Storage::open(Ok(store.clone()));
			s.bind_profile(Some(profile.clone()), cx);
			s.composer.update(cx, |input, cx| input.set_content("Original input", cx));
			s.save_draft_document(cx);
			assert!(s.draft_profiles.storage.task.is_some());
			s.state = LoadState::Ready;
			s.execute(
				ChiefActionDto::RefreshIntegrations { work_id: EntityId::new("work").unwrap() },
				None,
				cx,
			);
			assert!(s.submission.command.is_none(), "network task cannot precede durable save");
			let key = s.submission.waiting.as_ref().unwrap().key.clone();
			assert_eq!(s.submission.unconfirmed, vec![key.clone()]);
		});
		visual.run_until_parked();
		let snapshot = store.load().unwrap();
		assert!(
			snapshot.revision >= 3,
			"save draft, publish dispatch fence, then settle known failure"
		);
		let document = DesktopDraftDocument::decode(&snapshot.payload).unwrap();
		let saved = &document.profiles[&profile.draft_scope_key()];
		assert!(saved.unconfirmed_commands.is_empty());
		assert!(!saved.uncertain);
		assert_eq!(saved.composer.text, "Original input");
		let restored = Drafts::from_document(saved.clone(), 99);
		assert!(!restored.uncertain);
		assert!(restored.unconfirmed.is_empty());
	}

	#[gpui::test]
	fn cold_dispatch_profile_change_cancels_before_network(cx: &mut gpui::TestAppContext) {
		let (_service, profile, other) = super::super::tests::profiles();
		let directory = tempfile::tempdir().unwrap();
		let store =
			ClientDraftStore::open_at(&directory.path().canonicalize().unwrap().join("desktop"))
				.unwrap();
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual, |s, cx| {
			s.draft_profiles.storage = Storage::open(Ok(store.clone()));
			s.bind_profile(Some(profile.clone()), cx);
			s.state = LoadState::Ready;
			s.execute(
				ChiefActionDto::RefreshIntegrations { work_id: EntityId::new("work").unwrap() },
				None,
				cx,
			);
			assert!(s.submission.waiting.is_some());
			s.bind_profile(Some(other), cx);
			assert!(s.submission.waiting.is_none());
			assert!(s.submission.command.is_none());
			assert!(!s.sending && !s.uncertain);
		});
		visual.run_until_parked();
		let saved = DesktopDraftDocument::decode(&store.load().unwrap().payload).unwrap();
		let original = &saved.profiles[&profile.draft_scope_key()];
		assert!(original.unconfirmed_commands.is_empty());
		assert!(!original.uncertain);
		surface.read_with(visual, |s, _| assert!(s.submission.command.is_none()));
	}

	#[gpui::test]
	fn cold_dispatch_save_conflict_never_starts_network_task(cx: &mut gpui::TestAppContext) {
		let (_service, profile, _) = super::super::tests::profiles();
		let directory = tempfile::tempdir().unwrap();
		let store =
			ClientDraftStore::open_at(&directory.path().canonicalize().unwrap().join("desktop"))
				.unwrap();
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual, |s, cx| {
			s.draft_profiles.storage = Storage::open(Ok(store.clone()));
			s.bind_profile(Some(profile), cx);
			s.state = LoadState::Ready;
			store.save(0, &DesktopDraftDocument::default().encode().unwrap()).unwrap();
			s.execute(
				ChiefActionDto::RefreshIntegrations { work_id: EntityId::new("work").unwrap() },
				None,
				cx,
			);
			assert!(s.submission.command.is_none());
		});
		visual.run_until_parked();
		surface.read_with(visual, |s, _| {
			assert!(s.submission.command.is_none());
			assert!(s.submission.waiting.is_none());
			assert!(s.submission.unconfirmed.is_empty());
			assert!(!s.sending && !s.uncertain);
			assert!(s.feedback.contains("Nothing was sent"));
		});
		assert_eq!(store.load().unwrap().revision, 1);
	}

	#[gpui::test]
	fn cold_drafts_restore_primary_input_and_keep_questions_pending(cx: &mut gpui::TestAppContext) {
		let (_service, profile, _) = super::super::tests::profiles();
		let directory = tempfile::tempdir().unwrap();
		let root = directory.path().canonicalize().unwrap().join("desktop");
		let store = ClientDraftStore::open_at(&root).unwrap();
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual, |s, cx| {
			s.draft_profiles.storage = Storage::open(Ok(store.clone()));
			s.bind_profile(Some(profile.clone()), cx);
			s.composer_manager = Some("work".into());
			s.draft_profiles.threads.insert("work".into(), "native-thread".into());
			s.composer.update(cx, |input, cx| input.set_content("Unsent cold draft", cx));
			s.effort = ConversationReasoningEffort::new("provider-defined-effort").unwrap();
			s.mark_effort_intent(cx);
			s.attachments.push(decodex_protocol::ChiefAttachmentDto {
				path: ConversationWorkingDirectory::new("/tmp/selected.png").unwrap(),
				image: true,
			});
			s.restored_question_drafts.push(decodex_protocol::DesktopQuestionDraft {
				work_id: EntityId::new("work").unwrap(),
				thread_id: WireText::new("native-thread").unwrap(),
				question_id: WireText::new("q").unwrap(),
				text: "Question draft".into(),
				selected: None,
				custom: Some("Alternative".into()),
				collapsed: true,
			});
			s.uncertain = true;
			s.submission.unconfirmed.push(IdempotencyKey::new("original-command").unwrap());
			s.save_draft_document(cx);
		});
		visual.run_until_parked();
		assert_eq!(store.load().unwrap().revision, 1);
		let (restored, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		restored.update(visual, |s, cx| {
			s.draft_profiles.storage = Storage::open(ClientDraftStore::open_at(&root));
			s.bind_profile(Some(profile), cx);
			assert_eq!(s.composer.read(cx).content(), "Unsent cold draft");
			assert_eq!(s.composer_manager.as_deref(), Some("work"));
			assert_eq!(s.draft_profiles.threads["work"], "native-thread");
			assert_eq!(s.attachments.len(), 1);
			assert_eq!(
				s.draft_profiles.execution.choice("work").reasoning_effort,
				Some(ConversationReasoningEffort::new("provider-defined-effort").unwrap())
			);
			assert_eq!(s.restored_question_drafts.len(), 1);
			assert!(
				s.async_question_inputs.is_empty(),
				"native history must admit question restoration"
			);
			assert!(s.uncertain && !s.sending && s.submission.command.is_none());
			assert_eq!(s.submission.unconfirmed[0].as_str(), "original-command");
			assert!(!s.draft_owner_available());
		});
	}

	#[gpui::test]
	fn cold_draft_conflict_keeps_disk_and_current_input(cx: &mut gpui::TestAppContext) {
		let (_service, profile, _) = super::super::tests::profiles();
		let directory = tempfile::tempdir().unwrap();
		let store =
			ClientDraftStore::open_at(&directory.path().canonicalize().unwrap().join("desktop"))
				.unwrap();
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual, |s, cx| {
			s.draft_profiles.storage = Storage::open(Ok(store.clone()));
			s.bind_profile(Some(profile), cx);
			s.composer.update(cx, |input, cx| input.set_content("Keep current edit", cx));
		});
		let other = DesktopDraftDocument::default().encode().unwrap();
		store.save(0, &other).unwrap();
		surface.update(visual, |s, cx| s.save_draft_document(cx));
		visual.run_until_parked();
		surface.read_with(visual, |s, cx| {
			assert_eq!(s.composer.read(cx).content(), "Keep current edit");
			assert!(s.draft_storage_notice().is_some());
		});
		assert_eq!(store.load().unwrap().payload, other);
	}
}

#[cfg(test)]
#[path = "chief_creation_setup_tests.rs"]
mod creation_tests;

#[cfg(test)]
mod ordinary_owner_tests {
	use super::*;

	#[gpui::test]
	fn ordinary_creation_acceptance_saves_the_new_owner_and_later_editor(
		cx: &mut gpui::TestAppContext,
	) {
		use crate::{
			client_lifecycle::ConnectionView,
			conversations::tests::{catalog_conversations, take_ready_command},
			shell::{Destination, Shell},
		};
		for later in ["Original input", "Later unsent input"] {
			let (_service, profile, _) = super::super::tests::profiles();
			let directory = tempfile::tempdir().unwrap();
			let store = ClientDraftStore::open_at(
				&directory.path().canonicalize().unwrap().join("desktop"),
			)
			.unwrap();
			let (conversations, server, mut task) = catalog_conversations();
			conversations.begin_new();
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
				s.composer.update(cx, |input, cx| input.set_content("Original input", cx));
				s.synchronize_conversations(cx);
			});
			crate::conversations::creation_defaults_tests::reply_defaults(
				&conversations,
				&server,
				decodex_protocol::InitialModelDefaults {
					configured: decodex_protocol::InitialExecutionDefaults {
						model: Some(
							decodex_protocol::ConversationModel::new("native-model").unwrap(),
						),
						reasoning_effort: None,
						service_tier: None,
					},
					managed: Default::default(),
					catalog_model: None,
				},
			);
			shell.update(visual, |s, cx| s.synchronize_conversations(cx));
			visual.run_until_parked();
			visual.update(|window, cx| {
				window.resize(gpui::size(gpui::px(1440.), gpui::px(1000.)));
				window.draw(cx).clear();
			});
			let send = visual.debug_bounds("conversation-send").unwrap();
			visual.simulate_click(send.center(), gpui::Modifiers::default());
			visual.run_until_parked();
			let original =
				take_ready_command(&conversations, &server).expect("saved creation command");
			let decodex_protocol::CommandPayload::CreateConversation { conversation_id, .. } =
				&original.payload
			else {
				panic!("creation")
			};
			shell.update(visual, |s, cx| {
				s.composer.update(cx, |input, cx| input.set_content(later, cx))
			});
			task.conversation_id = conversation_id.clone();
			let result = decodex_protocol::CommandResultEnvelope {
				version: decodex_protocol::CURRENT_VERSION,
				server_id: server.clone(),
				client_command_id: original.client_command_id,
				idempotency_key: original.idempotency_key,
				outcome: decodex_protocol::CommandOutcome::Succeeded,
				entity_revision: Some(task.conversation_revision),
				payload: Some(decodex_protocol::ResultPayload::ConversationAccepted {
					conversation: task,
				}),
				error: None,
			};
			assert_eq!(
				conversations.route_command_result(1, &server, &result),
				crate::conversations::ConversationRouteOutcome::Fresh
			);
			shell.update(visual, |s, cx| s.synchronize_conversations(cx));
			visual.run_until_parked();
			let saved = DesktopDraftDocument::decode(&store.load().unwrap().payload).unwrap();
			let draft = &saved.profiles[&profile.draft_scope_key()].ordinary["/tmp"];
			assert_eq!(draft.composer.conversation_id.as_ref(), Some(conversation_id));
			assert_eq!(draft.composer.text, if later == "Original input" { "" } else { later });
			assert!(draft.unconfirmed.is_empty());
			assert!(draft.new_conversation.is_none());
			assert!(take_ready_command(&conversations, &server).is_none());
		}
	}

	#[gpui::test]
	fn ordinary_competing_writer_blocks_dispatch_until_keep_both(cx: &mut gpui::TestAppContext) {
		exercise_ordinary_competing_writer(cx, false);
	}

	#[gpui::test]
	fn ordinary_competing_writer_cancel_button_keeps_input_without_sending(
		cx: &mut gpui::TestAppContext,
	) {
		exercise_ordinary_competing_writer(cx, true);
	}

	fn exercise_ordinary_competing_writer(cx: &mut gpui::TestAppContext, cancel: bool) {
		use crate::{
			client_lifecycle::ConnectionView,
			conversations::tests::{catalog_conversations, take_ready_command},
			shell::Shell,
		};
		let (_service, profile, _) = super::super::tests::profiles();
		let directory = tempfile::tempdir().unwrap();
		let store =
			ClientDraftStore::open_at(&directory.path().canonicalize().unwrap().join("desktop"))
				.unwrap();
		let (conversations, server, _) = catalog_conversations();
		let (shell, visual) =
			cx.add_window_view(|window, cx| Shell::new(window, cx, ConnectionView::Stopped));
		shell.update(visual, |s, cx| {
			s.conversations = conversations.clone();
			s.reset_cards.profile = Some(profile.clone());
			s.chief.update(cx, |chief, cx| {
				chief.draft_profiles.storage = Storage::open(Ok(store.clone()));
				chief.bind_profile(Some(profile.clone()), cx);
			});
			s.reset_ordinary_draft_binding(cx);
			s.composer.update(cx, |input, cx| input.set_content("Original input", cx));
			s.sync_ordinary_drafts(cx);
		});
		visual.run_until_parked();
		let snapshot = store.load().unwrap();
		let mut remote = DesktopDraftDocument::decode(&snapshot.payload).unwrap();
		remote
			.profiles
			.get_mut(&profile.draft_scope_key())
			.unwrap()
			.ordinary
			.get_mut("/tmp")
			.unwrap()
			.composer
			.text = "Other client input".into();
		let competing_store =
			ClientDraftStore::open_at(&directory.path().canonicalize().unwrap().join("desktop"))
				.unwrap();
		competing_store.save(snapshot.revision, &remote.encode().unwrap()).unwrap();
		conversations.submit("Original input").unwrap();
		shell.update(visual, |s, cx| s.sync_ordinary_drafts(cx));
		visual.run_until_parked();
		assert!(take_ready_command(&conversations, &server).is_none());
		shell.update(visual, |s, cx| {
			assert!(s.chief.read(cx).ordinary_draft_notice().is_some());
			assert_eq!(s.composer.read(cx).content(), "Original input");
			s.composer.update(cx, |input, cx| input.set_content("Later local input", cx));
			s.sync_ordinary_drafts(cx);
			s.selected = crate::shell::Destination::Conversations;
			s.synchronize_conversations(cx);
		});
		if cancel {
			visual.update(|window, cx| {
				window.resize(gpui::size(gpui::px(1440.), gpui::px(1000.)));
				window.draw(cx).clear();
			});
			let button = visual.debug_bounds("ordinary-cancel-unsent").expect("cancel button");
			visual.simulate_click(button.center(), gpui::Modifiers::default());
			assert!(!conversations.can_cancel_unsent_ordinary());
		}
		shell.update(visual, |s, cx| {
			assert_eq!(s.composer.read(cx).content(), "Later local input");
			s.chief.update(cx, |chief, cx| {
				assert!(chief.can_keep_both_drafts());
				chief.keep_both_drafts(cx);
			});
			assert!(take_ready_command(&conversations, &server).is_none());
		});
		visual.run_until_parked();
		let saved = DesktopDraftDocument::decode(&store.load().unwrap().payload).unwrap();
		let scope = profile.draft_scope_key();
		let records: Vec<_> = saved
			.profiles
			.get(&scope)
			.into_iter()
			.chain(
				saved
					.recovered
					.iter()
					.filter(|copy| copy.scope.as_ref() == Some(&scope))
					.map(|copy| &copy.draft),
			)
			.filter_map(|profile| profile.ordinary.get("/tmp"))
			.collect();
		assert!(records.iter().any(|record| record.composer.text == "Other client input"));
		let local =
			records.iter().find(|record| record.composer.text == "Later local input").unwrap();
		if cancel {
			assert!(records.iter().all(|record| record.unconfirmed.is_empty()));
			assert!(take_ready_command(&conversations, &server).is_none());
		} else {
			assert_eq!(local.unconfirmed.len(), 1);
			assert_eq!(
				take_ready_command(&conversations, &server),
				Some(local.unconfirmed[0].clone())
			);
		}
	}

	#[gpui::test]
	fn ordinary_live_restore_keeps_both_and_saves_later_input(cx: &mut gpui::TestAppContext) {
		use crate::{
			client_lifecycle::ConnectionView,
			conversations::tests::{catalog_conversations, take_ready_command},
			shell::Shell,
		};
		let (_service, profile, _) = super::super::tests::profiles();
		let directory = tempfile::tempdir().unwrap();
		let store =
			ClientDraftStore::open_at(&directory.path().canonicalize().unwrap().join("desktop"))
				.unwrap();
		let (conversations, server, _) = catalog_conversations();
		let (shell, visual) =
			cx.add_window_view(|window, cx| Shell::new(window, cx, ConnectionView::Stopped));
		shell.update(visual, |s, cx| {
			s.conversations = conversations.clone();
			s.reset_cards.profile = Some(profile.clone());
			s.chief.update(cx, |chief, cx| {
				chief.draft_profiles.storage = Storage::open(Ok(store.clone()));
				chief.bind_profile(Some(profile.clone()), cx);
			});
			s.reset_ordinary_draft_binding(cx);
			s.composer.update(cx, |input, cx| input.set_content("Original input", cx));
			s.sync_ordinary_drafts(cx);
		});
		visual.run_until_parked();
		conversations.submit("Original input").unwrap();
		shell.update(visual, |s, cx| {
			s.sync_ordinary_drafts(cx);
			assert!(take_ready_command(&conversations, &server).is_none());
			s.chief.update(cx, |chief, _| {
				let record = chief
					.draft_profiles
					.storage
					.document
					.profiles
					.get_mut(&profile.draft_scope_key())
					.unwrap()
					.ordinary
					.get_mut("/tmp")
					.unwrap();
				record.composer.text = "Requested replacement".into();
			});
			s.composer.update(cx, |input, cx| input.set_content("Later local input", cx));
			s.sync_ordinary_drafts(cx);
			assert_eq!(s.composer.read(cx).content(), "Later local input");
		});
		visual.run_until_parked();
		let saved = DesktopDraftDocument::decode(&store.load().unwrap().payload).unwrap();
		let scope = profile.draft_scope_key();
		let active = &saved.profiles[&scope].ordinary["/tmp"];
		assert_eq!(active.composer.text, "Later local input");
		assert_eq!(active.unconfirmed.len(), 1);
		assert!(saved.recovered.iter().any(|copy| {
			copy.scope.as_ref() == Some(&scope)
				&& copy.draft.ordinary.get("/tmp").is_some_and(|draft| {
					draft.composer.text == "Requested replacement"
						&& draft.unconfirmed == active.unconfirmed
				})
		}));
		assert_eq!(
			take_ready_command(&conversations, &server),
			Some(active.unconfirmed[0].clone())
		);
	}
	#[gpui::test]
	fn chief_capture_and_profile_switch_preserve_ordinary_edits(cx: &mut gpui::TestAppContext) {
		let (_root, first, second) = super::super::tests::profiles();
		let directory = tempfile::tempdir().unwrap();
		let store =
			ClientDraftStore::open_at(&directory.path().canonicalize().unwrap().join("desktop"))
				.unwrap();
		let surface = cx.new(ChiefSurface::new);
		let ordinary = decodex_protocol::DesktopOrdinaryDraft {
			working_directory: decodex_protocol::ConversationWorkingDirectory::new("/tmp").unwrap(),
			composer: decodex_protocol::DesktopOrdinaryComposerDraft {
				conversation_id: None,
				text: "Ordinary input".into(),
				execution: decodex_protocol::ConversationExecutionSettings {
					model: decodex_protocol::ConversationModel::new("native-model").unwrap(),
					reasoning_effort: None,
					fast: false,
					service_tier: None,
				},
				creation_intent: Default::default(),
			},
			new_conversation: None,
			parked: Default::default(),
			unconfirmed: vec![],
		};
		surface.update(cx, |s, cx| {
			s.draft_profiles.storage = Storage::open(Ok(store.clone()));
			s.bind_profile(Some(first.clone()), cx);
			s.draft_profiles
				.storage
				.document
				.profiles
				.entry(first.draft_scope_key())
				.or_default()
				.ordinary
				.insert("/tmp".into(), ordinary.clone());
			let review = "b".repeat(64);
			s.draft_profiles
				.storage
				.document
				.profiles
				.get_mut(&first.draft_scope_key())
				.unwrap()
				.prompt_edits
				.insert(
					review.clone(),
					decodex_protocol::DesktopPromptEditDraft {
						work_id: EntityId::new("edited-work").unwrap(),
						thread_id: WireText::new("native-thread").unwrap(),
						before_turn_id: WireText::new("turn").unwrap(),
						item_id: WireText::new("item").unwrap(),
						original_hash: decodex_protocol::Sha256Digest::new("a".repeat(64)).unwrap(),
						review_token: WireText::new(review).unwrap(),
						receipt_id: Some(42),
						handback_pending: true,
						input: decodex_protocol::PromptDraft::new(vec![
							serde_json::json!({"type":"image","fileId":"retained-native-file"}),
						])
						.unwrap(),
					},
				);
			s.composer.update(cx, |input, cx| input.set_content("Chief input", cx));
			s.bind_profile(Some(second.clone()), cx);
			s.composer.update(cx, |input, cx| input.set_content("Other service input", cx));
			s.bind_profile(Some(first.clone()), cx);
			s.remember_draft_document(cx);
			assert_eq!(s.composer.read(cx).content(), "Chief input");
			assert!(
				s.draft_profiles.storage.document.profiles[&second.draft_scope_key()]
					.ordinary
					.is_empty()
			);
			publish_document(&store, 0, &s.draft_profiles.storage.document)
				.unwrap_or_else(|_| panic!("publish"));
		});
		let decoded = DesktopDraftDocument::decode(&store.load().unwrap().payload).unwrap();
		assert_eq!(decoded.profiles[&first.draft_scope_key()].ordinary["/tmp"], ordinary);
		let restored = &decoded.profiles[&first.draft_scope_key()].prompt_edits[&"b".repeat(64)];
		assert_eq!(restored.input.parts()[0]["fileId"], "retained-native-file");
		assert!(restored.handback_pending);
		assert!(decoded.profiles[&second.draft_scope_key()].prompt_edits.is_empty());
	}
}

#[cfg(test)]
#[path = "chief_ordinary_outcome_tests.rs"]
mod ordinary_outcome_tests;
