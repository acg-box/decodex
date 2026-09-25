//! Shared-store handoff for ordinary editors and original delivery identities.
use super::{
	CommandEnvelope, CommandOutcome, CommandResultEnvelope, ConversationCommandState,
	ConversationQueryPurpose, ConversationRouteOutcome, Conversations, EntityId, InFlightCommand,
	State, accepted_archive_result, accepted_result_task,
};
use decodex_protocol::{
	ConversationCreationReceiptRequest, ConversationCreationReceiptResult,
	DesktopOrdinaryComposerDraft, DesktopOrdinaryDraft, QueryPayload, QueryResultPayload,
};
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct DeliveryDrafts {
	pub(super) required: bool,
	pub(super) saved: Vec<CommandEnvelope>,
	pub(super) unconfirmed: Vec<CommandEnvelope>,
	pub(super) confirmed: Vec<CommandEnvelope>,
	pub(super) turn_readbacks:
		Vec<(CommandEnvelope, decodex_protocol::ConversationTurnOutcomeResult)>,
	pub(super) readbacks: Vec<(CommandEnvelope, ConversationCreationReceiptResult)>,
	parked: BTreeMap<String, DesktopOrdinaryComposerDraft>,
	new_conversation: Option<DesktopOrdinaryComposerDraft>,
}

impl Conversations {
	pub(crate) fn ordinary_creation_receipts(
		&self,
	) -> Vec<(CommandEnvelope, Option<ConversationCreationReceiptResult>)> {
		let state = self.lock();
		state
			.delivery
			.unconfirmed
			.iter()
			.filter(|command| ConversationCreationReceiptRequest::from_command(command).is_some())
			.map(|command| {
				(
					command.clone(),
					state
						.delivery
						.readbacks
						.iter()
						.find(|(original, _)| original == command)
						.map(|(_, result)| result.clone()),
				)
			})
			.collect()
	}

	pub(crate) fn open_recorded_ordinary_creation(&self, command: &CommandEnvelope) -> bool {
		let Some(request) = ConversationCreationReceiptRequest::from_command(command) else {
			return false;
		};
		let mut state = self.lock();
		if state.pending_command.is_some() || state.in_flight_command.is_some()
			|| !state.delivery.unconfirmed.contains(command)
			|| !state.delivery.readbacks.iter().any(|(original, result)| original == command
				&& matches!(result, ConversationCreationReceiptResult::Recorded { conversation_id, creation_revision }
					if conversation_id == &request.conversation_id && creation_revision.0 > 0)) {
			return false;
		}
		if state.selected.is_none() && state.requested_selection.is_none() {
			state.delivery.new_conversation = None;
		}
		state.clear_catalog();
		state.execution = request.execution;
		state.selected = state
			.tasks
			.iter()
			.any(|task| task.conversation_id == request.conversation_id)
			.then(|| request.conversation_id.clone());
		state.requested_selection = state.selected.is_none().then_some(request.conversation_id);
		state.selection_suppressed = true;
		state.confirm_delivery(command);
		state.delivery.readbacks.retain(|(original, _)| original != command);
		state.command = ConversationCommandState::Idle;
		state.queue_list();
		drop(state);
		self.inner.notify.notify_one();
		true
	}

	pub(crate) fn check_ordinary_creation(&self, command: &CommandEnvelope) -> bool {
		let Some(request) = ConversationCreationReceiptRequest::from_command(command) else {
			return false;
		};
		let mut state = self.lock();
		if !state.delivery.unconfirmed.contains(command) {
			return false;
		}
		let queued = state.queue_query(
			QueryPayload::GetConversationCreationReceipt { request },
			ConversationQueryPurpose::CreationReceipt { command: Box::new(command.clone()) },
		);
		if queued {
			state.delivery.readbacks.retain(|(original, _)| original != command);
		}
		drop(state);
		if queued {
			self.inner.notify.notify_one();
		}
		queued
	}

	pub(crate) fn can_cancel_unsent_ordinary(&self) -> bool {
		let state = self.lock();
		state.delivery.required
			&& state.pending_command.is_some()
			&& state.in_flight_command.is_none()
	}

	pub(crate) fn cancel_unsent_ordinary(&self) -> bool {
		let mut state = self.lock();
		if !state.delivery.required || state.in_flight_command.is_some() {
			return false;
		}
		let Some(pending) = state.pending_command.take() else { return false };
		// Taking the queue entry under the dispatch lock proves that no transport
		// has taken this command. A dispatched or restored command cannot enter here.
		state.confirm_delivery(&pending.envelope);
		state.delivery.saved.retain(|saved| saved != &pending.envelope);
		state.cancel_refresh_batch();
		state.command = ConversationCommandState::Idle;
		state.last_submission_accepted = false;
		state.submission_result_generation = state.submission_result_generation.saturating_add(1);
		drop(state);
		self.inner.notify.notify_one();
		true
	}

	pub(crate) fn ordinary_editor_owner(&self) -> Option<EntityId> {
		let state = self.lock();
		state.selected.clone().or_else(|| state.requested_selection.clone())
	}

	pub(crate) fn require_saved_dispatch(&self) {
		self.lock().delivery.required = true;
	}

	pub(crate) fn release_saved_commands(&self, commands: &[CommandEnvelope]) {
		let mut state = self.lock();
		if state.delivery.saved == commands {
			return;
		}
		state.delivery.saved = commands.to_vec();
		drop(state);
		self.inner.notify.notify_one();
	}

	pub(crate) fn ordinary_draft(&self, text: &str) -> Option<DesktopOrdinaryDraft> {
		let state = self.lock();
		let owner = state.selected.clone().or_else(|| state.requested_selection.clone());
		let mut unconfirmed = state.delivery.unconfirmed.clone();
		for command in state
			.pending_command
			.as_ref()
			.map(|pending| &pending.envelope)
			.into_iter()
			.chain(state.in_flight_command.as_ref().map(|pending| &pending.envelope))
		{
			if !unconfirmed.contains(command) {
				unconfirmed.push(command.clone());
			}
		}
		let mut parked = state.delivery.parked.clone();
		if let Some(owner) = &owner {
			parked.remove(owner.as_str());
		}
		Some(DesktopOrdinaryDraft {
			working_directory: self.inner.working_directory.clone()?,
			composer: DesktopOrdinaryComposerDraft {
				conversation_id: owner.clone(),
				text: text.into(),
				execution: state.execution.clone(),
				creation_intent: state.creation_intent.clone(),
			},
			new_conversation: owner
				.is_some()
				.then(|| state.delivery.new_conversation.clone())
				.flatten(),
			parked,
			unconfirmed,
		})
	}

	pub(crate) fn restore_ordinary_draft(&self, draft: &DesktopOrdinaryDraft) -> bool {
		if self.inner.working_directory.as_ref() != Some(&draft.working_directory) {
			return false;
		}
		let mut state = self.lock();
		if state.pending_command.is_some() || state.in_flight_command.is_some() {
			return false;
		}
		state.clear_catalog();
		state.execution = draft.composer.execution.clone();
		state.execution_source = None;
		state.creation_intent = draft.composer.creation_intent.clone();
		state.execution_choice_owner = draft.composer.conversation_id.clone();
		state.delivery.parked = draft.parked.clone();
		state.delivery.new_conversation = draft.new_conversation.clone();
		state.delivery.unconfirmed = draft.unconfirmed.clone();
		state.delivery.saved.clear();
		state.delivery.readbacks.clear();
		state.delivery.turn_readbacks.clear();
		state.requested_selection = draft.composer.conversation_id.clone();
		state.selected = state
			.requested_selection
			.as_ref()
			.filter(|id| state.tasks.iter().any(|task| &task.conversation_id == *id))
			.cloned();
		if state.selected.is_some() {
			state.requested_selection = None;
		}
		state.selection_suppressed = true;
		if !state.delivery.unconfirmed.is_empty() {
			state.command = ConversationCommandState::OutcomeUnknown;
		}
		true
	}

	pub(crate) fn park_ordinary_editor(&self, editor: DesktopOrdinaryComposerDraft) {
		let mut state = self.lock();
		if let Some(owner) = &editor.conversation_id {
			state.delivery.parked.insert(owner.as_str().into(), editor);
		} else {
			state.delivery.new_conversation = Some(editor);
		}
	}

	pub(crate) fn select_ordinary_editor(
		&self,
		owner: Option<&EntityId>,
	) -> Option<DesktopOrdinaryComposerDraft> {
		let mut state = self.lock();
		let editor = match owner {
			Some(owner) => state.delivery.parked.get(owner.as_str()).cloned(),
			None => state.delivery.new_conversation.clone(),
		}?;
		state.execution = editor.execution.clone();
		state.execution_source = None;
		state.creation_intent = editor.creation_intent.clone();
		state.execution_choice_owner = editor.conversation_id.clone();
		Some(editor)
	}

	pub(crate) fn confirmed_ordinary_commands(&self) -> Vec<CommandEnvelope> {
		std::mem::take(&mut self.lock().delivery.confirmed)
	}
}

impl State {
	pub(super) fn route_creation_receipt(
		&mut self,
		command: &CommandEnvelope,
		payload: &QueryResultPayload,
	) -> (ConversationRouteOutcome, bool) {
		let Some(request) = ConversationCreationReceiptRequest::from_command(command) else {
			return (ConversationRouteOutcome::Refused, false);
		};
		if !self.delivery.unconfirmed.contains(command) {
			return (ConversationRouteOutcome::Unmatched, false);
		}
		let result = match payload {
			QueryResultPayload::ConversationCreationReceipt(result) => result.clone(),
			_ => ConversationCreationReceiptResult::Unavailable,
		};
		if let ConversationCreationReceiptResult::Recorded { conversation_id, creation_revision } =
			&result && (conversation_id != &request.conversation_id || creation_revision.0 == 0)
		{
			return (ConversationRouteOutcome::Refused, false);
		}
		self.delivery.readbacks.retain(|(original, _)| original != command);
		self.delivery.readbacks.push((command.clone(), result));
		// Local creation does not prove provider completion or clear the saved original.
		(ConversationRouteOutcome::Fresh, false)
	}

	pub(super) fn confirm_terminal_delivery(
		&mut self,
		in_flight: &InFlightCommand,
		result: &CommandResultEnvelope,
	) {
		let terminal = match result.outcome {
			CommandOutcome::Succeeded =>
				accepted_archive_result(in_flight, result).is_some()
					|| accepted_result_task(in_flight, result).is_some(),
			CommandOutcome::Rejected => result.error.is_some() && result.payload.is_none(),
			CommandOutcome::AcceptanceUnknown => false,
		};
		if terminal {
			self.confirm_delivery(&in_flight.envelope);
		}
	}

	pub(super) fn confirm_delivery(&mut self, command: &CommandEnvelope) {
		self.delivery.unconfirmed.retain(|entry| entry != command);
		if !self.delivery.confirmed.contains(command) {
			self.delivery.confirmed.push(command.clone());
		}
	}
}
