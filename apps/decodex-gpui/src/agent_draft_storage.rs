//! Local cold recovery and background publication of unsent desktop inputs.
#[path = "agent_draft_merge.rs"] mod merge;
#[path = "agent_ordinary_storage.rs"] mod ordinary;

use std::{
	collections::BTreeSet,
	iter,
	time::{Duration, Instant},
};

#[cfg(test)]
use crate::shell::agent_surface::drafts::{
	ConversationReasoningEffort, ConversationWorkingDirectory, Entity, IntoElement, LoadState,
	Render, Window, px,
};
use crate::{
	shell::agent_surface::drafts::{
		AgentSurface, BTreeMap, ClientProfile, Context, Drafts, EntityId, PendingCommand, Task,
		WireText, execution_intent::Intents,
	},
	ui_feedback::Feedback,
};
use decodex_protocol::{
	AgentCommandResponse, ClientDraftStore, DesktopComposerDraft, DesktopDraftDocument,
	DesktopMessage, DesktopMessageState, DesktopPendingDraft, DesktopProfileDraft,
	DesktopPromptEditDraft, IdempotencyKey,
};

// Preserve absence separately from a failed bounded conversion.
trait TransposeOption<T> {
	fn transpose_option(self) -> Option<Option<T>>;
}

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
	conflicted: bool,
	seeded: bool,
}
impl Storage {
	pub(super) fn clear_unbound(&mut self) {
		self.document.unbound = Default::default();
	}

	pub(super) fn restore(&self, scope: &str, epoch: u64) -> Option<Drafts> {
		self.document.profiles.get(scope).cloned().map(|saved| Drafts::from_document(saved, epoch))
	}

	pub(super) fn seed_conflict(&mut self) {
		self.conflicted = true;
		self.seeded = true;
		self.error = None;
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
			conflicted: false,
			seeded: false,
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
			state.error = Some(
				"Saved drafts unavailable. Keep this window open to retain current edits.".into(),
			);
		}

		state
	}
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

impl AgentSurface {
	pub(in super::super) fn dismiss_failed_message(&mut self, key: &IdempotencyKey, cx: &mut Context<Self>) {
		let state = &mut self.draft_profiles.storage;
		let Some(profile) = self.profile.as_ref().and_then(|profile| {
			state.document.profiles.get_mut(&profile.draft_scope_key())
		}) else { return };
		if profile.messages.get(key).is_some_and(|message| message.state == DesktopMessageState::Failed) {
			profile.messages.remove(key);
			self.save_draft_document(cx);
			cx.notify();
		}
	}

	pub(in super::super) fn clear_prepared_prompt_record(
		&mut self,
		profile: &ClientProfile,
		pending: &DesktopPromptEditDraft,
	) -> bool {
		let Some(saved) = self
			.draft_profiles
			.storage
			.document
			.profiles
			.get_mut(&profile.draft_scope_key())
			.and_then(|profile| profile.prompt_edits.get_mut(pending.review_token.as_str()))
		else {
			return false;
		};

		if saved != pending {
			return false;
		}

		saved.pending_send = None;

		true
	}

	pub(in super::super) fn settle_prompt_send(
		&mut self,
		expected: &DesktopPromptEditDraft,
		accepted: bool,
		cx: &mut Context<Self>,
	) -> Result<Option<DesktopPromptEditDraft>, &'static str> {
		expected.send_identity()?;

		let scope =
			self.profile.as_ref().ok_or("Service profile is unavailable")?.draft_scope_key();

		self.remember_draft_document(cx);

		let mut next = self.draft_profiles.storage.document.clone();
		let profile = next.profiles.get_mut(&scope).ok_or("Draft profile is unavailable")?;
		let key = expected.review_token.as_str();

		if profile.prompt_edits.get(key) != Some(expected) {
			return Err("The retained send draft changed");
		}

		let mut retained = expected.clone();

		retained.pending_send = None;

		if accepted {
			profile.prompt_edits.remove(key);
			next.encode()?;

			self.draft_profiles.storage.document = next;

			self.save_draft_document(cx);

			Ok(None)
		} else {
			profile.prompt_edits.insert(key.into(), retained.clone());
			next.encode()?;

			self.draft_profiles.storage.document = next;

			self.save_draft_document(cx);

			Ok(Some(retained))
		}
	}

	pub(in super::super) fn prompt_editor_saved(&self, expected: &DesktopPromptEditDraft) -> bool {
		let Some(profile) = &self.profile else {
			return false;
		};

		if self.draft_profiles.active.as_ref() != Some(profile) {
			return false;
		}

		let state = &self.draft_profiles.storage;
		let scope = profile.draft_scope_key();
		let get = |document: &DesktopDraftDocument| {
			document
				.profiles
				.get(&scope)
				.and_then(|profile| profile.prompt_edits.get(expected.review_token.as_str()))
				== Some(expected)
		};

		state.store.is_some() && get(&state.document) && get(&state.saved)
	}

	pub(in super::super) fn renew_prompt_editor(
		&mut self,
		previous: &DesktopPromptEditDraft,
		fresh: &DesktopPromptEditDraft,
		cx: &mut Context<Self>,
	) -> Result<DesktopPromptEditDraft, &'static str> {
		if self.selected.as_deref() != Some(previous.work_id.as_str())
			|| !self.saved_prompt_editors(previous.work_id.as_str()).contains(previous)
		{
			return Err("Draft changed during review");
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
			return Err("Another draft uses this review");
		}

		editors.remove(previous.review_token.as_str());
		editors.insert(renewed.review_token.as_str().into(), renewed.clone());
		next.encode()?;

		self.draft_profiles.storage.document = next;

		self.save_draft_document(cx);

		Ok(renewed)
	}

	pub(in super::super) fn saved_prompt_editors(&self, work: &str) -> Vec<DesktopPromptEditDraft> {
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
		draft: &DesktopPromptEditDraft,
		cx: &mut Context<Self>,
	) -> Result<(), &'static str> {
		if self.selected.as_deref() != Some(draft.work_id.as_str())
			|| !self.saved_prompt_editors(draft.work_id.as_str()).contains(draft)
		{
			return Err("Draft source changed");
		}
		if draft.handback_pending
			|| draft.receipt_id.is_some()
			|| draft.confirmation_key.is_some()
			|| draft.fork.is_some()
			|| draft.pending_send.is_some()
		{
			return Err("Recover the history edit before discarding this draft");
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
		draft: DesktopPromptEditDraft,
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
		self.cancel_prepared_prompt_send();

		self.draft_profiles.storage.quitting = true;

		self.save_draft_document(cx);

		let retained = cx.entity();

		cx.spawn(async move |_, cx| {
			let deadline = Instant::now() + Duration::from_secs(5);

			loop {
				let result = retained.update(cx, |surface, cx| {
					surface.save_draft_document(cx);

					let storage = &mut surface.draft_profiles.storage;
					let result = if storage.error.is_some() || Instant::now() >= deadline {
						Some(false)
					} else if storage.task.is_none() && storage.document == storage.saved {
						Some(true)
					} else {
						None
					};

					if result.is_some() {
						storage.quitting = false;

						if result == Some(false) {
							surface.feedback = Feedback::error(
								"Quit canceled: drafts are not saved. Keep this window open.",
							);
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

	pub(in super::super) fn can_retry_input_save(&self) -> bool {
		self.draft_profiles.storage.store.is_some()
			&& self.draft_profiles.storage.error.is_some()
			&& self.draft_profiles.storage.task.is_none()
			&& !self.sending
	}

	pub(in super::super) fn retry_input_save(&mut self, cx: &mut Context<Self>) {
		if !self.can_retry_input_save() {
			return;
		}
		self.draft_profiles.storage.error = None;
		self.save_draft_document(cx);
		cx.notify();
	}

	pub(in super::super) fn draft_storage_notice(&self) -> Option<&str> {
		self.draft_profiles.storage.error.as_deref().or_else(|| {
			self.draft_profiles
				.storage
				.busy
				.then_some("Waiting to save drafts. Keep this window open.")
		})
	}

	pub(in super::super) fn remember_draft_document(&mut self, cx: &Context<Self>) {
		let Some(profile) = self.draft_profiles.active.as_ref() else {
			if self.composer_manager.is_some() {
				self.draft_profiles.storage.error = Some(
					"Draft storage unavailable. Keep this window open to retain current edits."
						.into(),
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
				for composer in iter::once(&draft.composer).chain(draft.parked.values()) {
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
				self.draft_profiles.storage.error = Some(
					"Draft storage unavailable. Keep this window open to retain current edits."
						.into(),
				),
		}
	}

	pub(in super::super) fn retained_messages(&self) -> Vec<(IdempotencyKey, DesktopMessage)> {
		self.draft_profiles
			.active
			.as_ref()
			.and_then(|profile| {
				self.draft_profiles.storage.document.profiles.get(&profile.draft_scope_key())
			})
			.map(|profile| {
				profile
					.messages
					.iter()
					.map(|(key, message)| (key.clone(), message.clone()))
					.collect()
			})
			.unwrap_or_default()
	}

	pub(in super::super) fn stage_message(
		&mut self,
		key: &IdempotencyKey,
		message: DesktopMessage,
		replaces: Option<&IdempotencyKey>,
		cx: &Context<Self>,
	) -> Result<(), &'static str> {
		self.remember_draft_document(cx);
		let scope = self
			.draft_profiles
			.active
			.as_ref()
			.ok_or("Input storage is unavailable.")?
			.draft_scope_key();
		let mut document = self.draft_profiles.storage.document.clone();
		let messages = &mut document.profiles.entry(scope).or_default().messages;
		if let Some(previous) = replaces {
			messages.remove(previous);
		}
		messages.insert(key.clone(), message);
		document.encode().map_err(|_| "Input could not be saved. Nothing sent.")?;
		self.draft_profiles.storage.document = document;
		Ok(())
	}

	pub(in super::super) fn settle_message(
		&mut self,
		key: Option<&IdempotencyKey>,
		result: &Result<AgentCommandResponse, String>,
	) {
		let Some(key) = key else { return };
		let Some(profile) = self.draft_profiles.active.as_ref().and_then(|profile| {
			self.draft_profiles.storage.document.profiles.get_mut(&profile.draft_scope_key())
		}) else {
			return;
		};
		if matches!(result, Ok(AgentCommandResponse::Accepted { .. })) {
			profile.messages.remove(key);
		} else if let Some(message) = profile.messages.get_mut(key) {
			message.state =
				if matches!(result, Ok(AgentCommandResponse::PotentiallyDispatched { .. })) {
					DesktopMessageState::Unconfirmed
				} else {
					DesktopMessageState::Failed
				};
			message.error = match result {
				Err(error) => Some(error.clone()),
				Ok(AgentCommandResponse::Rejected { error }) =>
					Some(super::super::command_rejection_message(error)),
				_ => None,
			};
		}
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
		let owners: BTreeSet<_> = self
			.draft_profiles
			.texts
			.keys()
			.chain(self.draft_profiles.files.keys())
			.chain(self.draft_profiles.tasks.keys())
			.collect();
		let mut parked = BTreeMap::new();

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
			messages: self
				.draft_profiles
				.active
				.as_ref()
				.and_then(|profile| {
					self.draft_profiles.storage.document.profiles.get(&profile.draft_scope_key())
				})
				.map(|profile| profile.messages.clone())
				.unwrap_or_default(),
			new_conversation_workspace: self
				.workspace
				.new_conversation_workspace
				.as_deref()
				.map(EntityId::new)
				.transpose()
				.ok()?,
			new_conversation: self
				.workspace
				.new_conversation
				.as_deref()
				.map(EntityId::new)
				.transpose()
				.ok()?,
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
			questions: self.restored_question_drafts.clone(),
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
				Some("Draft storage unavailable. Nothing sent.".into());
		}

		if self.draft_profiles.storage.conflicted && self.draft_profiles.storage.task.is_none() {
			self.merge_current_edits(cx);
			return;
		}
		let state = &mut self.draft_profiles.storage;

		if state.task.is_some() {
			return;
		}
		if state.error.is_some() {
			if let Some(queued) = self.submission.waiting.take() {
				self.finish_command(
					queued.pending,
					Err("Draft not saved. Nothing sent.".into()),
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
						let changed = !state.busy;
						state.busy = true;
						if changed {
							cx.notify();
						}

						return;
					},
					Err(SaveFailure::Conflict) => {
						state.conflicted = true;
						state.error = None;
					},
					Err(SaveFailure::Invalid(reason)) => state.error = Some(reason.into()),
					Err(SaveFailure::Failed) =>
						state.error = Some(
							"Drafts not saved. Keep this window open to retain current edits."
								.into(),
						),
				}

				surface.save_draft_document(cx);
				cx.notify();
			});
		}));
	}
}

impl Drafts {
	fn from_document(saved: DesktopProfileDraft, epoch: u64) -> Self {
		let mut result = Self {
			new_conversation: saved.new_conversation.map(|id| id.as_str().into()),
			new_conversation_workspace: saved
				.new_conversation_workspace
				.map(|id| id.as_str().into()),
			creation: saved.composer.creation,
			unconfirmed: saved.unconfirmed_commands,
			text: saved.composer.text,
			manager: saved.composer.work_id.map(|id| id.as_str().into()),
			attachments: saved.composer.attachments,
			references: saved.composer.references,
			uncertain: saved.uncertain,
			restored_questions: saved.questions,
			execution: Intents::from_saved(saved.execution_revision, saved.execution),
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
			result.feedback =
				Feedback::warning("Delivery unconfirmed. Check the conversation before resending.");
		}

		result
	}
}

impl<T> TransposeOption<T> for Option<Option<T>> {
	fn transpose_option(self) -> Self {
		match self {
			None => Some(None),
			Some(value) => value.map(Some),
		}
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
		Err(decodex_protocol::ClientDraftError::WriteUnconfirmed(_)) =>
			confirm_unconfirmed_publication(store, &bytes),
		Err(decodex_protocol::ClientDraftError::Busy) => Err(SaveFailure::Busy),
		Err(decodex_protocol::ClientDraftError::Conflict) => Err(SaveFailure::Conflict),
		Err(_) => Err(SaveFailure::Failed),
	}
}

fn confirm_unconfirmed_publication(
	store: &ClientDraftStore,
	bytes: &[u8],
) -> Result<u64, SaveFailure> {
	let actual = store.load().map_err(|_| SaveFailure::Failed)?;

	if actual.payload != bytes {
		return Err(SaveFailure::Failed);
	}

	// A visible rename can precede a failed directory sync. Require one successful
	// atomic publication, using the observed revision so a competing writer wins.
	// Do not recurse if this publication is also unconfirmed.
	match store.save(actual.revision, bytes) {
		Ok(revision) => Ok(revision),
		Err(decodex_protocol::ClientDraftError::Busy) => Err(SaveFailure::Busy),
		Err(decodex_protocol::ClientDraftError::Conflict) => Err(SaveFailure::Conflict),
		Err(_) => Err(SaveFailure::Failed),
	}
}

#[cfg(test)]
#[path = "agent_prompt_confirm_wire_tests.rs"]
mod prompt_confirm_tests;
#[cfg(test)]
#[path = "agent_prompt_handback_wire_tests.rs"]
mod prompt_handback_tests;
#[cfg(test)]
#[path = "agent_prompt_send_wire_tests.rs"]
mod prompt_send_tests;
#[cfg(test)]
mod tests {
	use decodex_protocol::AgentActionDto;
	use std::os::unix::fs::OpenOptionsExt as _;

	#[cfg(test)] use gpui::AppContext as _;

	#[cfg(test)]
	use crate::shell::agent_surface::drafts::storage::{
		ConversationReasoningEffort, ConversationWorkingDirectory, IdempotencyKey, LoadState,
	};
	use crate::shell::agent_surface::drafts::{
		storage::{
			self, AgentSurface, ClientDraftStore, DesktopDraftDocument, Drafts, Duration, EntityId,
			SaveFailure, Storage, WireText,
		},
		tests,
	};

	#[gpui::test]
	fn new_conversation_draft_restores_without_creating_work(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.new_work_conversation(cx);

			s.workspace.new_conversation_workspace = Some("folder".into());

			s.composer.update(cx, |input, cx| input.set_content("Unsent idea", cx));

			let saved = s.capture_draft_document(cx).unwrap();
			let restored = Drafts::from_document(saved, s.command_epoch);

			s.workspace.new_conversation = None;
			s.workspace.new_conversation_workspace = None;
			s.selected = s.root_id();

			s.apply_drafts(restored, cx);

			assert!(s.is_new_conversation());
			assert_eq!(s.workspace.new_conversation_workspace.as_deref(), Some("folder"));
			assert_eq!(s.composer.read(cx).content(), "Unsent idea");
			assert!(s.conversation_work().is_none());
			assert!(s.submission.waiting.is_none());
		});
	}

	#[test]
	fn unconfirmed_publication_requires_a_successful_save_before_acknowledgement() {
		let directory = tempfile::tempdir().unwrap();
		let store =
			ClientDraftStore::open_at(&directory.path().canonicalize().unwrap().join("desktop"))
				.unwrap();
		let bytes = DesktopDraftDocument::default().encode().unwrap();
		let observed_revision = store.save(0, &bytes).unwrap();
		// Model a completed rename followed by a failed parent-directory sync.
		let acknowledged = storage::confirm_unconfirmed_publication(&store, &bytes)
			.unwrap_or_else(|_| panic!("confirm through a fresh durable publication"));

		assert!(
			acknowledged > observed_revision,
			"matching readback alone does not confirm a failed sync"
		);
		assert_eq!(store.load().unwrap().revision, acknowledged);
		assert_eq!(store.load().unwrap().payload, bytes);
	}

	#[test]
	fn unconfirmed_publication_retains_busy_and_different_snapshots() {
		let directory = tempfile::tempdir().unwrap();
		let root = directory.path().canonicalize().unwrap().join("desktop");
		let store = ClientDraftStore::open_at(&root).unwrap();
		let bytes = DesktopDraftDocument::default().encode().unwrap();
		let revision = store.save(0, &bytes).unwrap();
		let lock = std::fs::OpenOptions::new()
			.create(true)
			.truncate(false)
			.read(true)
			.write(true)
			.mode(0o600)
			.open(root.join("client-drafts/writer.lock"))
			.unwrap();

		lock.try_lock().unwrap();

		assert!(
			matches!(
				storage::confirm_unconfirmed_publication(&store, &bytes),
				Err(SaveFailure::Busy)
			),
			"matching bytes do not bypass a competing writer"
		);
		assert_eq!(store.load().unwrap().revision, revision);

		drop(lock);

		let other = b"another window's newer draft";
		let newer = store.save(revision, other).unwrap();

		assert!(matches!(
			storage::confirm_unconfirmed_publication(&store, &bytes),
			Err(SaveFailure::Failed)
		));
		assert_eq!(store.load().unwrap().revision, newer);
		assert_eq!(store.load().unwrap().payload, other);
	}

	#[gpui::test]
	fn prompt_send_acceptance_clears_exact_editor_without_archiving_input(
		cx: &mut gpui::TestAppContext,
	) {
		let (_service, profile, _) = tests::profiles();
		let directory = tempfile::tempdir().unwrap();
		let store =
			ClientDraftStore::open_at(&directory.path().canonicalize().unwrap().join("desktop"))
				.unwrap();
		let input = decodex_protocol::PromptDraft::new(vec![
			serde_json::json!({"type":"text","text":"Edited"}),
			serde_json::json!({"type":"image","fileId":"retained-file","detail":"original"}),
		])
		.unwrap();
		let draft = decodex_protocol::DesktopPromptEditDraft {
			work_id: EntityId::new("work").unwrap(),
			thread_id: WireText::new("thread").unwrap(),
			before_turn_id: WireText::new("turn").unwrap(),
			item_id: WireText::new("item").unwrap(),
			original_hash: input.fingerprint().unwrap(),
			review_token: WireText::new("a".repeat(64)).unwrap(),
			receipt_id: Some(42),
			confirmation_key: None,
			fork: None,
			pending_send: None,
			handback_pending: false,
			input,
		};
		let pending = draft
			.begin_send(7, IdempotencyKey::new("send-once").unwrap(), Default::default())
			.unwrap();
		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, cx| {
			s.draft_profiles.storage = Storage::open(Ok(store.clone()));

			s.bind_profile(Some(profile.clone()), cx);
			s.composer.update(cx, |input, cx| input.set_content("Unrelated main input", cx));
			s.stage_prompt_editor(pending.clone(), cx).unwrap();

			let mut other = draft.clone();

			other.review_token = WireText::new("b".repeat(64)).unwrap();

			s.stage_prompt_editor(other, cx).unwrap();
		});

		cx.run_until_parked();

		surface.update(cx, |s, cx| {
			let mut crossed = pending.clone();

			crossed.pending_send.as_mut().unwrap().command_key =
				IdempotencyKey::new("different").unwrap();

			assert!(s.settle_prompt_send(&crossed, true, cx).is_err());
			assert!(s.settle_prompt_send(&pending, true, cx).unwrap().is_none());
			assert_eq!(s.composer.read(cx).content(), "Unrelated main input");
		});

		cx.run_until_parked();

		let document = DesktopDraftDocument::decode(&store.load().unwrap().payload).unwrap();
		let saved = &document.profiles[&profile.draft_scope_key()];

		assert!(!saved.prompt_edits.contains_key(pending.review_token.as_str()));
		assert!(saved.prompt_edits.contains_key(&"b".repeat(64)));
		assert_eq!(saved.composer.text, "Unrelated main input");
	}

	#[gpui::test]
	fn prompt_handback_requires_the_exact_saved_draft_and_profile(cx: &mut gpui::TestAppContext) {
		let (_service, profile, other) = tests::profiles();
		let directory = tempfile::tempdir().unwrap();
		let store =
			ClientDraftStore::open_at(&directory.path().canonicalize().unwrap().join("desktop"))
				.unwrap();
		let surface = cx.new(AgentSurface::new);
		let input = decodex_protocol::PromptDraft::new(vec![
			serde_json::json!({"type":"text","text":"Retained edit"}),
		])
		.unwrap();
		let mut draft = decodex_protocol::DesktopPromptEditDraft {
			work_id: EntityId::new("work").unwrap(),
			thread_id: WireText::new("thread").unwrap(),
			before_turn_id: WireText::new("turn").unwrap(),
			item_id: WireText::new("item").unwrap(),
			original_hash: input.fingerprint().unwrap(),
			review_token: WireText::new("a".repeat(64)).unwrap(),
			receipt_id: Some(42),
			handback_pending: true,
			confirmation_key: None,
			fork: None,
			pending_send: None,
			input,
		};

		surface.update(cx, |s, cx| {
			s.draft_profiles.storage = Storage::open(Ok(store.clone()));

			s.bind_profile(Some(profile.clone()), cx);
			s.stage_prompt_editor(draft.clone(), cx).unwrap();

			assert!(!s.prompt_editor_saved(&draft));
		});

		cx.run_until_parked();
		surface.update(cx, |s, _| assert!(s.prompt_editor_saved(&draft)));

		let doc = DesktopDraftDocument::decode(&store.load().unwrap().payload).unwrap();

		assert_eq!(
			doc.profiles[&profile.draft_scope_key()].prompt_edits[draft.review_token.as_str()],
			draft
		);

		draft.input.replace_text(0, 0..0, "Later ").unwrap();
		surface.update(cx, |s, cx| {
			assert!(!s.prompt_editor_saved(&draft));

			s.stage_prompt_editor(draft.clone(), cx).unwrap();

			assert!(!s.prompt_editor_saved(&draft));
		});
		cx.run_until_parked();
		surface.update(cx, |s, cx| {
			assert!(s.prompt_editor_saved(&draft));

			s.bind_profile(Some(other), cx);

			assert!(!s.prompt_editor_saved(&draft));
		});
	}

	#[gpui::test]
	fn cold_unbound_draft_reopens_and_moves_only_to_first_profile(cx: &mut gpui::TestAppContext) {
		let (_service, profile, other) = tests::profiles();
		let directory = tempfile::tempdir().unwrap();
		let root = directory.path().canonicalize().unwrap().join("desktop");
		let store = ClientDraftStore::open_at(&root).unwrap();
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |s, cx| {
			s.draft_profiles.storage = Storage::open(Ok(store.clone()));

			s.composer.update(cx, |input, cx| input.set_content("Before choosing a service", cx));
			s.attachments.push(decodex_protocol::AgentAttachmentDto {
				path: ConversationWorkingDirectory::new("/tmp/local.png").unwrap(),
				image: true,
				skill_name: None,
			});

			assert!(!s.drafts_ready_for_quit(cx));
		});

		visual.run_until_parked();

		let saved = DesktopDraftDocument::decode(&store.load().unwrap().payload).unwrap();

		assert_eq!(saved.unbound.text, "Before choosing a service");
		assert!(saved.profiles.is_empty());

		let (restored, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

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
		let (_service, profile, _) = tests::profiles();
		let directory = tempfile::tempdir().unwrap();
		let store =
			ClientDraftStore::open_at(&directory.path().canonicalize().unwrap().join("desktop"))
				.unwrap();
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
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
	fn cold_quit_merges_current_input_before_close(cx: &mut gpui::TestAppContext) {
		let (_service, profile, _) = tests::profiles();
		let directory = tempfile::tempdir().unwrap();
		let store =
			ClientDraftStore::open_at(&directory.path().canonicalize().unwrap().join("desktop"))
				.unwrap();
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		let flush = surface.update(visual, |s, cx| {
			s.draft_profiles.storage = Storage::open(Ok(store.clone()));

			s.bind_profile(Some(profile.clone()), cx);
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

		assert_eq!(outcome.get(), Some(true));

		surface.update(visual, |s, cx| {
			assert!(s.drafts_ready_for_quit(cx));
			assert_eq!(s.composer.read(cx).content(), "Unmerged edit");
		});

		let saved = DesktopDraftDocument::decode(&store.load().unwrap().payload).unwrap();
		assert_eq!(saved.profiles[&profile.draft_scope_key()].composer.text, "Unmerged edit");
	}

	#[gpui::test]
	fn cold_dispatch_busy_writer_retries_without_dispatch_or_losing_edits(
		cx: &mut gpui::TestAppContext,
	) {
		let (_service, profile, _) = tests::profiles();
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

		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |s, cx| {
			s.draft_profiles.storage = Storage::open(Ok(store.clone()));

			s.bind_profile(Some(profile.clone()), cx);

			s.state = LoadState::Ready;

			s.execute(
				AgentActionDto::RefreshIntegrations { work_id: EntityId::new("work").unwrap() },
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
		let (_service, profile, _) = tests::profiles();
		let directory = tempfile::tempdir().unwrap();
		let store =
			ClientDraftStore::open_at(&directory.path().canonicalize().unwrap().join("desktop"))
				.unwrap();
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |s, cx| {
			s.draft_profiles.storage = Storage::open(Ok(store.clone()));

			s.bind_profile(Some(profile.clone()), cx);
			s.composer.update(cx, |input, cx| input.set_content("Original input", cx));
			s.save_draft_document(cx);

			assert!(s.draft_profiles.storage.task.is_some());

			s.state = LoadState::Ready;

			s.execute(
				AgentActionDto::RefreshIntegrations { work_id: EntityId::new("work").unwrap() },
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
		let (_service, profile, other) = tests::profiles();
		let directory = tempfile::tempdir().unwrap();
		let store =
			ClientDraftStore::open_at(&directory.path().canonicalize().unwrap().join("desktop"))
				.unwrap();
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |s, cx| {
			s.draft_profiles.storage = Storage::open(Ok(store.clone()));

			s.bind_profile(Some(profile.clone()), cx);

			s.state = LoadState::Ready;

			s.execute(
				AgentActionDto::RefreshIntegrations { work_id: EntityId::new("work").unwrap() },
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
		let (_service, profile, _) = tests::profiles();
		let directory = tempfile::tempdir().unwrap();
		let store =
			ClientDraftStore::open_at(&directory.path().canonicalize().unwrap().join("desktop"))
				.unwrap();
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |s, cx| {
			s.draft_profiles.storage = Storage::open(Ok(store.clone()));

			s.bind_profile(Some(profile), cx);

			s.state = LoadState::Ready;

			store.save(0, &DesktopDraftDocument::default().encode().unwrap()).unwrap();
			s.execute(
				AgentActionDto::RefreshIntegrations { work_id: EntityId::new("work").unwrap() },
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
			assert!(s.feedback.contains("Nothing sent"));
		});

		let saved = DesktopDraftDocument::decode(&store.load().unwrap().payload).unwrap();
		assert!(saved.profiles.values().all(|profile| profile.unconfirmed_commands.is_empty()));
	}

	#[gpui::test]
	fn cold_drafts_restore_primary_input_and_keep_questions_pending(cx: &mut gpui::TestAppContext) {
		let (_service, profile, _) = tests::profiles();
		let directory = tempfile::tempdir().unwrap();
		let root = directory.path().canonicalize().unwrap().join("desktop");
		let store = ClientDraftStore::open_at(&root).unwrap();
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |s, cx| {
			s.draft_profiles.storage = Storage::open(Ok(store.clone()));

			s.bind_profile(Some(profile.clone()), cx);

			s.composer_manager = Some("work".into());

			s.draft_profiles.threads.insert("work".into(), "native-thread".into());
			s.composer.update(cx, |input, cx| input.set_content("Unsent cold draft", cx));

			s.effort = ConversationReasoningEffort::new("provider-defined-effort").unwrap();

			s.mark_effort_intent(cx);
			s.attachments.push(decodex_protocol::AgentAttachmentDto {
				path: ConversationWorkingDirectory::new("/tmp/selected.png").unwrap(),
				image: true,
				skill_name: None,
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

		let (restored, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

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
			assert_eq!(s.restored_question_drafts[0].custom.as_deref(), Some("Alternative"));
			assert!(s.uncertain && !s.sending && s.submission.command.is_none());
			assert_eq!(s.submission.unconfirmed[0].as_str(), "original-command");
			assert!(!s.draft_owner_available());
		});
	}

	#[gpui::test]
	fn cold_draft_conflict_saves_current_input_without_alternative_snapshots(
		cx: &mut gpui::TestAppContext,
	) {
		let (_service, profile, _) = tests::profiles();
		let directory = tempfile::tempdir().unwrap();
		let store =
			ClientDraftStore::open_at(&directory.path().canonicalize().unwrap().join("desktop"))
				.unwrap();
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |s, cx| {
			s.draft_profiles.storage = Storage::open(Ok(store.clone()));

			s.bind_profile(Some(profile.clone()), cx);
			s.composer.update(cx, |input, cx| input.set_content("Keep current edit", cx));
		});

		let other = DesktopDraftDocument::default().encode().unwrap();

		store.save(0, &other).unwrap();
		surface.update(visual, |s, cx| s.save_draft_document(cx));
		visual.run_until_parked();
		surface.read_with(visual, |s, cx| {
			assert_eq!(s.composer.read(cx).content(), "Keep current edit");
			assert!(s.draft_storage_notice().is_none());
		});

		let saved = DesktopDraftDocument::decode(&store.load().unwrap().payload).unwrap();
		assert_eq!(saved.profiles[&profile.draft_scope_key()].composer.text, "Keep current edit");
	}
}
#[cfg(test)]
#[path = "agent_creation_setup_tests.rs"]
mod creation_tests;
#[cfg(test)]
mod ordinary_owner_tests {
	#[cfg(test)] use gpui::AppContext as _;

	use crate::{
		client_lifecycle::ConnectionView,
		conversations::{creation_defaults_tests, tests},
		shell::{
			Destination, Shell,
			agent_surface::drafts::{
				storage,
				storage::{
					AgentSurface, ClientDraftStore, DesktopDraftDocument, EntityId, Storage,
					WireText,
				},
			},
		},
	};

	#[gpui::test]
	fn ordinary_creation_acceptance_saves_the_new_owner_and_later_editor(
		cx: &mut gpui::TestAppContext,
	) {
		for later in ["Original input", "Later unsent input"] {
			let (_service, profile, _) = super::super::tests::profiles();
			let directory = tempfile::tempdir().unwrap();
			let store = ClientDraftStore::open_at(
				&directory.path().canonicalize().unwrap().join("desktop"),
			)
			.unwrap();
			let (conversations, server, mut task) = tests::catalog_conversations();

			conversations.begin_new();

			let (shell, visual) =
				cx.add_window_view(|window, cx| Shell::new(window, cx, ConnectionView::Stopped));

			shell.update(visual, |s, cx| {
				s.conversations = conversations.clone();
				s.reset_cards.profile = Some(profile.clone());
				s.selected = Destination::Conversations;
				s.agent.update(cx, |agent, cx| {
					agent.draft_profiles.storage = Storage::open(Ok(store.clone()));

					agent.bind_profile(Some(profile.clone()), cx);
				});

				s.reset_ordinary_draft_binding(cx);
				s.composer.update(cx, |input, cx| input.set_content("Original input", cx));
				s.synchronize_conversations(cx);
			});

			creation_defaults_tests::reply_defaults(
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
				window.resize(gpui::size(gpui::px(1_440.), gpui::px(1_000.)));
				window.draw(cx).clear(cx);
			});

			let send = visual.debug_bounds("conversation-send").unwrap();

			visual.simulate_click(send.center(), gpui::Modifiers::default());
			visual.run_until_parked();

			let original =
				tests::take_ready_command(&conversations, &server).expect("saved creation command");
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
			assert!(tests::take_ready_command(&conversations, &server).is_none());
		}
	}

	#[gpui::test]
	fn ordinary_competing_writer_saves_current_edits_before_dispatch(
		cx: &mut gpui::TestAppContext,
	) {
		exercise_ordinary_competing_writer(cx, false);
	}

	#[gpui::test]
	fn ordinary_competing_writer_cancel_button_keeps_input_without_sending(
		cx: &mut gpui::TestAppContext,
	) {
		exercise_ordinary_competing_writer(cx, true);
	}

	fn exercise_ordinary_competing_writer(cx: &mut gpui::TestAppContext, cancel: bool) {
		let (_service, profile, _) = super::super::tests::profiles();
		let directory = tempfile::tempdir().unwrap();
		let store =
			ClientDraftStore::open_at(&directory.path().canonicalize().unwrap().join("desktop"))
				.unwrap();
		let (conversations, server, _) = tests::catalog_conversations();
		let (shell, visual) =
			cx.add_window_view(|window, cx| Shell::new(window, cx, ConnectionView::Stopped));

		shell.update(visual, |s, cx| {
			s.conversations = conversations.clone();
			s.reset_cards.profile = Some(profile.clone());
			s.agent.update(cx, |agent, cx| {
				agent.draft_profiles.storage = Storage::open(Ok(store.clone()));

				agent.bind_profile(Some(profile.clone()), cx);
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
		let writer = std::fs::OpenOptions::new()
			.read(true)
			.write(true)
			.open(directory.path().join("desktop/client-drafts/writer.lock"))
			.unwrap();
		writer.try_lock().unwrap();
		conversations.submit("Original input").unwrap();
		shell.update(visual, |s, cx| s.sync_ordinary_drafts(cx));
		visual.run_until_parked();

		assert!(tests::take_ready_command(&conversations, &server).is_none());

		shell.update(visual, |s, cx| {
			assert!(s.agent.read(cx).ordinary_draft_notice().is_some());
			assert_eq!(s.composer.read(cx).content(), "Original input");

			s.composer.update(cx, |input, cx| input.set_content("Later local input", cx));
			s.sync_ordinary_drafts(cx);

			s.selected = crate::shell::Destination::Conversations;

			s.synchronize_conversations(cx);
		});

		if cancel {
			visual.update(|window, cx| {
				window.resize(gpui::size(gpui::px(1_440.), gpui::px(1_000.)));
				window.draw(cx).clear(cx);
			});

			let button = visual.debug_bounds("ordinary-cancel-unsent").expect("cancel button");

			visual.simulate_click(button.center(), gpui::Modifiers::default());

			assert!(!conversations.can_cancel_unsent_ordinary());
		}

		drop(writer);
		shell.update(visual, |s, cx| s.agent.update(cx, |agent, cx| agent.save_draft_document(cx)));
		visual.run_until_parked();
		shell.update(visual, |s, cx| s.synchronize_conversations(cx));
		visual.run_until_parked();

		shell.update(visual, |s, cx| {
			assert_eq!(s.composer.read(cx).content(), "Later local input");
		});
		visual.run_until_parked();

		let saved = DesktopDraftDocument::decode(&store.load().unwrap().payload).unwrap();
		let scope = profile.draft_scope_key();
		let records: Vec<_> = saved
			.profiles
			.get(&scope)
			.into_iter()
			.filter_map(|profile| profile.ordinary.get("/tmp"))
			.collect();

		let local =
			records.iter().find(|record| record.composer.text == "Later local input").unwrap();

		if cancel {
			assert!(records.iter().all(|record| record.unconfirmed.is_empty()));
			assert!(tests::take_ready_command(&conversations, &server).is_none());
		} else {
			assert_eq!(local.unconfirmed.len(), 1);
			assert_eq!(
				tests::take_ready_command(&conversations, &server),
				Some(local.unconfirmed[0].clone())
			);
		}
	}

	#[gpui::test]
	fn agent_capture_and_profile_switch_preserve_ordinary_edits(cx: &mut gpui::TestAppContext) {
		let (_root, first, second) = super::super::tests::profiles();
		let directory = tempfile::tempdir().unwrap();
		let store =
			ClientDraftStore::open_at(&directory.path().canonicalize().unwrap().join("desktop"))
				.unwrap();
		let surface = cx.new(AgentSurface::new);
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
						confirmation_key: None,
						fork: None,
						pending_send: None,
						input: decodex_protocol::PromptDraft::new(vec![
							serde_json::json!({"type":"image","fileId":"retained-native-file"}),
						])
						.unwrap(),
					},
				);
			s.composer.update(cx, |input, cx| input.set_content("Agent input", cx));
			s.bind_profile(Some(second.clone()), cx);
			s.composer.update(cx, |input, cx| input.set_content("Other service input", cx));
			s.bind_profile(Some(first.clone()), cx);
			s.remember_draft_document(cx);

			assert_eq!(s.composer.read(cx).content(), "Agent input");
			assert!(
				s.draft_profiles.storage.document.profiles[&second.draft_scope_key()]
					.ordinary
					.is_empty()
			);

			storage::publish_document(&store, 0, &s.draft_profiles.storage.document)
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
#[path = "agent_ordinary_outcome_tests.rs"]
mod ordinary_outcome_tests;

#[cfg(test)]
mod message_tests {
	use super::{AgentSurface, ClientDraftStore, DesktopDraftDocument, Storage, publish_document};
	use decodex_protocol::{
		AgentActionDto, AgentAttachmentDto, ConversationWorkingDirectory, DesktopMessage,
		DesktopMessageState, EntityId, HistoryText, IdempotencyKey,
	};
	use gpui::AppContext as _;

	#[gpui::test]
	fn submitted_message_and_later_editor_survive_reopen_with_separate_delivery_state(
		cx: &mut gpui::TestAppContext,
	) {
		let (_service, profile, _) = crate::shell::agent_surface::drafts::tests::profiles();
		for state in [
			DesktopMessageState::Sending,
			DesktopMessageState::Unconfirmed,
			DesktopMessageState::Failed,
		] {
			let directory = tempfile::tempdir().unwrap();
			let store = ClientDraftStore::open_at(
				&directory.path().canonicalize().unwrap().join("desktop"),
			)
			.unwrap();
			let surface = cx.new(AgentSurface::new);
			let key = IdempotencyKey::new("original-message").unwrap();
			let action = AgentActionDto::SendConfigured {
				root_id: EntityId::new("agent").unwrap(),
				text: HistoryText::new("Original message").unwrap(),
				execution: Default::default(),
				attachments: vec![AgentAttachmentDto {
					path: ConversationWorkingDirectory::new("/tmp/original.png").unwrap(),
					image: true,
					skill_name: None,
				}],
				task_references: vec![],
			};
			let message = DesktopMessage {
				action,
				owner: Some(EntityId::new("agent").unwrap()),
				thread_id: None,
				text: "Original message".into(),
				state,
				error: None,
			};
			surface.update(cx, |s, cx| {
				s.draft_profiles.storage = Storage::open(Ok(store.clone()));
				s.bind_profile(Some(profile.clone()), cx);
				s.stage_message(&key, message.clone(), None, cx).unwrap();
				s.composer.update(cx, |input, cx| input.set_content("Later input stays here", cx));
				s.uncertain = state != DesktopMessageState::Failed;
				if s.uncertain {
					s.submission.unconfirmed.push(key.clone());
				}
				s.remember_draft_document(cx);
				assert!(publish_document(&store, 0, &s.draft_profiles.storage.document).is_ok());
			});
			let saved = DesktopDraftDocument::decode(&store.load().unwrap().payload).unwrap();
			assert!(saved.profiles[&profile.draft_scope_key()].messages[&key] == message);
			let cold = cx.new(AgentSurface::new);
			cold.update(cx, |s, cx| {
				s.draft_profiles.storage = Storage::open(Ok(store));
				s.bind_profile(Some(profile.clone()), cx);
				assert_eq!(s.composer.read(cx).content(), "Later input stays here");
				assert_eq!(s.uncertain, state != DesktopMessageState::Failed);
				assert!(s.submission.command.is_none() && s.submission.waiting.is_none());
				assert_eq!(s.retained_messages(), vec![(key.clone(), message.clone())]);
			});
		}
	}
}
