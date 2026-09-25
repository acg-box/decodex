//! Ordinary UI uses the existing shared writer and never restores a send queue.
use super::{Context, Shell};
use decodex_protocol::DesktopOrdinaryDraft;

impl Shell {
	pub(super) fn open_recorded_creation(
		&mut self,
		command: &decodex_protocol::CommandEnvelope,
		cx: &mut Context<Self>,
	) {
		let Some(request) =
			decodex_protocol::ConversationCreationReceiptRequest::from_command(command)
		else {
			return;
		};
		let Some(current) = self.conversations.ordinary_draft(self.composer.read(cx).content())
		else {
			return;
		};
		if !self.conversations.open_recorded_ordinary_creation(command) {
			return;
		}
		let carry = current
			.composer
			.conversation_id
			.as_ref()
			.is_none_or(|id| id == &request.conversation_id);
		let text = if carry {
			if current.composer.text == request.message.as_str() {
				String::new()
			} else {
				current.composer.text
			}
		} else {
			self.conversations.park_ordinary_editor(current.composer);
			self.conversations
				.select_ordinary_editor(Some(&request.conversation_id))
				.map(|editor| editor.text)
				.unwrap_or_default()
		};
		self.ordinary_syncing = true;
		self.ordinary_owner = Some(request.conversation_id.clone());
		if self
			.pending_submission
			.as_ref()
			.is_some_and(|pending| pending.conversation_id == request.conversation_id)
		{
			self.pending_submission = None;
		}
		self.composer.update(cx, |input, cx| input.set_content(&text, cx));
		self.ordinary_syncing = false;
		self.sync_ordinary_drafts(cx);
		self.synchronize_conversations(cx);
	}

	pub(super) fn sync_ordinary_drafts(&mut self, cx: &mut Context<Self>) {
		if self.ordinary_syncing || self.reset_cards.profile.is_none() {
			return;
		}
		let Some(directory) = self.conversations.working_directory() else { return };
		self.conversations.require_saved_dispatch();
		self.ordinary_syncing = true;
		let stored = self.chief.read(cx).ordinary_storage_record(directory.as_str(), false);
		if stored != self.ordinary_last
			&& let Some(stored) = stored.as_ref()
		{
			if self.conversations.restore_ordinary_draft(stored) {
				self.composer.update(cx, |input, cx| input.set_content(&stored.composer.text, cx));
				self.ordinary_owner = stored.composer.conversation_id.clone();
				self.ordinary_last = Some(stored.clone());
				self.pending_submission = None;
			} else {
				// A live command retains its editor. Keep the requested replacement
				// as a recoverable copy, then save subsequent local edits normally.
				self.chief.update(cx, |surface, _| surface.defer_ordinary_restore());
			}
		}
		self.quick = self.conversations.snapshot();
		let owner = self.conversations.ordinary_editor_owner();
		if owner != self.ordinary_owner {
			let rebinding_created = self.pending_submission.as_ref().is_some_and(|pending| {
				Some(&pending.conversation_id) == owner.as_ref()
					&& self.conversations.snapshot().last_submission_accepted
			});
			if !rebinding_created {
				if let Some(mut old) =
					self.ordinary_last.as_ref().map(|draft| draft.composer.clone())
				{
					old.text = self.composer.read(cx).content().into();
					self.conversations.park_ordinary_editor(old);
				}
				let text = self
					.conversations
					.select_ordinary_editor(owner.as_ref())
					.map(|draft| draft.text)
					.unwrap_or_default();
				self.composer.update(cx, |input, cx| input.set_content(&text, cx));
			}
			self.ordinary_owner = owner;
		}
		self.reconcile_pending_submission(cx);
		if let Some(draft) = self.conversations.ordinary_draft(self.composer.read(cx).content()) {
			let confirmed = self.conversations.confirmed_ordinary_commands();
			let saved = self.chief.read(cx).ordinary_storage_record(directory.as_str(), true);
			let needs_write = stored.as_ref() != Some(&draft)
				|| self.ordinary_last.as_ref() != Some(&draft)
				|| saved.as_ref() != Some(&draft)
				|| !confirmed.is_empty();
			self.ordinary_last = Some(draft.clone());
			if needs_write {
				self.chief
					.update(cx, |surface, cx| surface.save_ordinary_storage(draft, &confirmed, cx));
			}
			if let Some(saved) =
				self.chief.read(cx).ordinary_storage_record(directory.as_str(), true)
			{
				self.conversations.release_saved_commands(&saved.unconfirmed);
			}
		}
		self.ordinary_syncing = false;
	}

	pub(super) fn reset_ordinary_draft_binding(&mut self, cx: &mut Context<Self>) {
		self.ordinary_last = None::<DesktopOrdinaryDraft>;
		self.ordinary_owner = None;
		self.sync_ordinary_drafts(cx);
	}
}

/// Inspect original creation requests without replaying input or clearing delivery records.
pub(super) fn creation_receipt_controls(
	shell: &Shell,
	cx: &mut Context<Shell>,
) -> gpui::AnyElement {
	use decodex_protocol::{CommandPayload, ConversationCreationReceiptResult as Receipt};
	use gpui::{
		InteractiveElement as _, IntoElement as _, ParentElement as _,
		StatefulInteractiveElement as _, Styled as _, div,
	};
	let mut rows = div().flex().flex_col();
	for (index, (command, result)) in
		shell.conversations.ordinary_creation_receipts().into_iter().enumerate()
	{
		let CommandPayload::CreateConversation { message, .. } = &command.payload else { continue };
		let original: String = message.as_str().chars().take(160).collect();
		let recorded = matches!(&result, Some(Receipt::Recorded { .. }));
		let status = match result {
			Some(Receipt::Recorded { .. }) =>
				"Conversation created locally; model outcome not confirmed.",
			Some(Receipt::NotRecorded) => "No creation record found. Original input is retained.",
			Some(Receipt::Conflict) => "Saved request does not match the service record.",
			Some(Receipt::Unavailable) => "Creation record is unavailable. Try again.",
			None => "Creation outcome has not been checked.",
		};
		let check_command = command.clone();
		rows = rows.child(
			div().child(original).child(div().child(status)).child(
				div()
					.id(format!("ordinary-creation-check-{index}"))
					.debug_selector(move || format!("ordinary-creation-check-{index}"))
					.cursor_pointer()
					.child("Check saved creation")
					.on_click(cx.listener(move |shell, _, _, cx| {
						if !shell.conversations.check_ordinary_creation(&check_command) {
							shell.input_status = Some(
								"Wait for the current query or reconnect, then check again.".into(),
							);
						}
						shell.synchronize_conversations(cx);
						cx.notify();
					})),
			),
		);
		if recorded {
			rows = rows.child(
				div()
					.id(format!("ordinary-creation-open-{index}"))
					.debug_selector(move || format!("ordinary-creation-open-{index}"))
					.cursor_pointer()
					.child("Open created conversation")
					.on_click(cx.listener(move |shell, _, _, cx| {
						shell.open_recorded_creation(&command, cx);
						cx.notify();
					})),
			);
		}
	}
	rows.into_any_element()
}
