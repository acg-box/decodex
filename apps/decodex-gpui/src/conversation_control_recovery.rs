//! Observe current control targets without replaying their original commands.
use super::{
	CommandEnvelope, CommandPayload, ConversationCommandState, ConversationQueryPurpose,
	ConversationResult, ConversationRouteOutcome, ConversationSummary, Conversations, EntityId,
	QueryPayload, QueryResultPayload, State,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ControlObservation {
	Archived,
	Current,
	RoutingAdvanced,
	TurnInactive,
	StillActive,
	Missing,
	Conflict,
	Unavailable,
}
impl ControlObservation {
	pub(crate) fn can_acknowledge(self) -> bool {
		matches!(self, Self::Archived | Self::Current | Self::TurnInactive | Self::RoutingAdvanced)
	}

	pub(crate) fn message(self) -> &'static str {
		match self {
			Self::Archived => "Conversation is archived.",
			Self::RoutingAdvanced =>
				"Routing state has advanced; original command delivery is unconfirmed.",
			Self::Current =>
				"Current conversation state is available; original refresh delivery is unconfirmed.",
			Self::TurnInactive =>
				"The original turn is no longer active; cancellation is not confirmed.",
			Self::StillActive => "The original turn is still active.",
			Self::Missing => "Conversation not found; original outcome remains unknown.",
			Self::Conflict => "Observed state does not match the saved request.",
			Self::Unavailable => "Current state is unavailable. Try again.",
		}
	}
}

pub(super) fn control_target(command: &CommandEnvelope) -> Option<&EntityId> {
	match &command.payload {
		CommandPayload::ArchiveConversation { conversation_id }
		| CommandPayload::RefreshConversation { conversation_id }
		| CommandPayload::InterruptConversation { conversation_id, .. }
		| CommandPayload::ResumeConversationRouting { conversation_id }
		| CommandPayload::ResumeConversationEstablishment { conversation_id }
		| CommandPayload::CreateConversationRoutingSuccessor { conversation_id } => Some(conversation_id),
		_ => None,
	}
}

impl Conversations {
	pub(crate) fn ordinary_control_states(
		&self,
	) -> Vec<(CommandEnvelope, Option<ControlObservation>)> {
		let state = self.lock();
		state
			.delivery
			.unconfirmed
			.iter()
			.filter(|command| control_target(command).is_some())
			.map(|command| {
				(
					command.clone(),
					state
						.delivery
						.control_readbacks
						.iter()
						.find(|(original, _)| original == command)
						.map(|(_, value)| *value),
				)
			})
			.collect()
	}

	pub(crate) fn check_ordinary_control(&self, command: &CommandEnvelope) -> bool {
		let Some(target) = control_target(command) else { return false };
		let mut state = self.lock();
		if !state.delivery.unconfirmed.contains(command) {
			return false;
		}
		let source =
			state.tasks.iter().find(|task| &task.conversation_id == target).cloned().map(Box::new);
		let queued = state.queue_query(
			QueryPayload::GetConversation { conversation_id: target.clone() },
			ConversationQueryPurpose::ControlState { command: Box::new(command.clone()), source },
		);
		if queued {
			state.delivery.control_readbacks.retain(|(original, _)| original != command);
		}
		drop(state);
		if queued {
			self.inner.notify.notify_one();
		}
		queued
	}

	pub(crate) fn acknowledge_ordinary_control(&self, command: &CommandEnvelope) -> bool {
		let mut state = self.lock();
		if state.pending_command.is_some()
			|| state.in_flight_command.is_some()
			|| !state.delivery.unconfirmed.contains(command)
			|| !state
				.delivery
				.control_readbacks
				.iter()
				.any(|(original, value)| original == command && value.can_acknowledge())
		{
			return false;
		}
		state.confirm_delivery(command);
		if super::RoutingSuccessorReconciliation::from_command(command)
			.as_ref()
			.is_some_and(|saved| state.routing_successor_reconciliation.as_ref() == Some(saved))
		{
			state.routing_successor_reconciliation = None;
			state.outcome_unknown_readback_generation = None;
		}

		state.delivery.control_readbacks.retain(|(original, _)| original != command);
		if state.delivery.unconfirmed.is_empty() {
			state.command = ConversationCommandState::Idle;
		}
		state.queue_list();
		drop(state);
		self.inner.notify.notify_one();
		true
	}
}

impl State {
	pub(super) fn invalidate_control_state(&mut self, target: &EntityId) {
		self.delivery
			.control_readbacks
			.retain(|(command, _)| control_target(command) != Some(target));
	}

	pub(super) fn route_control_state(
		&mut self,
		command: &CommandEnvelope,
		source: Option<&ConversationSummary>,
		payload: &QueryResultPayload,
	) -> (ConversationRouteOutcome, bool) {
		let Some(target) = control_target(command) else {
			return (ConversationRouteOutcome::Refused, false);
		};
		if !self.delivery.unconfirmed.contains(command) {
			return (ConversationRouteOutcome::Unmatched, false);
		}
		if self.tasks.iter().find(|task| &task.conversation_id == target) != source {
			return (ConversationRouteOutcome::Refused, false);
		}
		let observation = match payload {
			QueryResultPayload::Conversation(result) => observe(command, target, result),
			_ => ControlObservation::Unavailable,
		};
		self.delivery.control_readbacks.retain(|(original, _)| original != command);
		self.delivery.control_readbacks.push((command.clone(), observation));
		(ConversationRouteOutcome::Fresh, false)
	}
}

fn observe(
	command: &CommandEnvelope,
	target: &EntityId,
	result: &ConversationResult,
) -> ControlObservation {
	use ControlObservation as O;
	let Some(expected) = command.expected_revision.filter(|revision| revision.0 > 0) else {
		return O::Conflict;
	};
	match result {
		ConversationResult::Archived { conversation_id, conversation_revision }
			if conversation_id == target
				&& conversation_revision.0 > expected.0
				&& !matches!(
					command.payload,
					CommandPayload::CreateConversationRoutingSuccessor { .. }
				) =>
			O::Archived,
		ConversationResult::RoutingSuccessorRedirect {
			source_conversation_id,
			source_conversation_revision,
			successor_conversation_id,
			successor_conversation_revision,
		} if source_conversation_id == target
			&& source_conversation_revision.0 > expected.0
			&& successor_conversation_id != target
			&& successor_conversation_revision.0 > 0 =>
			if matches!(command.payload, CommandPayload::CreateConversationRoutingSuccessor { .. })
			{
				if expected.0.checked_add(1) == Some(source_conversation_revision.0) {
					O::RoutingAdvanced
				} else {
					O::Conflict
				}
			} else {
				O::Archived
			},
		ConversationResult::Available(current)
			if &current.conversation_id == target
				&& current.conversation_revision.0 >= expected.0 =>
			match &command.payload {
				CommandPayload::RefreshConversation { .. } => O::Current,
				CommandPayload::ResumeConversationRouting { .. }
					if current.conversation_revision.0 > expected.0
						&& matches!(
							current.state,
							decodex_protocol::ConversationState::EstablishmentPending
								| decodex_protocol::ConversationState::QuotaExhausted
								| decodex_protocol::ConversationState::NoRoute
								| decodex_protocol::ConversationState::Establishing
								| decodex_protocol::ConversationState::Ready
								| decodex_protocol::ConversationState::Running
								| decodex_protocol::ConversationState::ManualRecovery
						) =>
					O::RoutingAdvanced,
				CommandPayload::ResumeConversationEstablishment { .. }
					if current.conversation_revision.0 > expected.0
						&& matches!(
							current.state,
							decodex_protocol::ConversationState::Ready
								| decodex_protocol::ConversationState::Running
								| decodex_protocol::ConversationState::ManualRecovery
						) =>
					O::RoutingAdvanced,
				CommandPayload::InterruptConversation { turn_id, .. }
					if current.conversation_revision.0 > expected.0
						&& matches!(
							current.state,
							decodex_protocol::ConversationState::Ready
								| decodex_protocol::ConversationState::Running
						) && current.active_turn_id.as_ref() != Some(turn_id) =>
					O::TurnInactive,
				CommandPayload::InterruptConversation { turn_id, .. }
					if current.active_turn_id.as_ref() == Some(turn_id) =>
					O::StillActive,
				CommandPayload::InterruptConversation { .. } => O::Unavailable,
				_ => O::Conflict,
			},
		ConversationResult::NotFound => O::Missing,
		ConversationResult::Unavailable { .. } => O::Unavailable,
		_ => O::Conflict,
	}
}

#[cfg(test)]
mod tests {
	use super::{CommandEnvelope, CommandPayload, ControlObservation, ConversationResult, observe};
	use crate::conversations::tests::{connected_conversations, dispatched_command};
	use decodex_protocol::{ConversationState, ConversationSummary, EntityRevision};

	fn fixture() -> (CommandEnvelope, ConversationSummary) {
		let (controller, server, task) = connected_conversations();
		controller.submit("Preserved input").expect("submit");
		let mut command = dispatched_command(&controller, &server);
		let CommandPayload::SubmitConversationTurn { conversation_id, turn_id, .. } =
			command.payload
		else {
			panic!("turn command")
		};
		command.payload = CommandPayload::InterruptConversation { conversation_id, turn_id };
		(command, task)
	}

	#[test]
	fn absent_active_turn_does_not_resolve_unknown_or_stale_interrupt() {
		let (command, mut task) = fixture();
		let target = task.conversation_id.clone();
		assert_eq!(
			observe(&command, &target, &ConversationResult::Available(task.clone())),
			ControlObservation::Unavailable
		);
		task.conversation_revision = EntityRevision(2);
		task.state = ConversationState::OutcomeUnknown;
		assert_eq!(
			observe(&command, &target, &ConversationResult::Available(task.clone())),
			ControlObservation::Unavailable
		);
		task.state = ConversationState::Ready;
		assert_eq!(
			observe(&command, &target, &ConversationResult::Available(task)),
			ControlObservation::TurnInactive
		);
	}

	#[test]
	fn missing_or_stale_archive_is_not_archive_evidence() {
		let (mut command, task) = fixture();
		let target = task.conversation_id;
		command.payload = CommandPayload::ArchiveConversation { conversation_id: target.clone() };
		assert_eq!(
			observe(&command, &target, &ConversationResult::NotFound),
			ControlObservation::Missing
		);
		for (revision, expected) in
			[(1, ControlObservation::Conflict), (2, ControlObservation::Archived)]
		{
			let result = ConversationResult::Archived {
				conversation_id: target.clone(),
				conversation_revision: EntityRevision(revision),
			};
			assert_eq!(observe(&command, &target, &result), expected);
		}
	}
	#[test]
	fn control_acknowledgement_preserves_unrelated_input_and_never_replays() {
		let (controller, server, task) = connected_conversations();
		controller.submit("Preserved input").expect("submit");
		let input = dispatched_command(&controller, &server);
		controller.session_ended(1);
		let mut draft = controller.ordinary_draft("Later unsent text").expect("draft");
		let (control, _) = fixture();
		draft.unconfirmed.push(control.clone());
		let (restored, server, _) = connected_conversations();
		assert!(restored.restore_ordinary_draft(&draft));
		assert!(!restored.acknowledge_ordinary_control(&control));
		restored.lock().pending_query = None;
		assert!(restored.check_ordinary_control(&control));
		let dispatch = restored.try_take_dispatch(1, &server).expect("query");
		let query = dispatch.query().expect("no command replay");
		let mut ready = task;
		ready.conversation_revision = EntityRevision(2);
		assert_eq!(
			restored.route_query_result(
				1,
				&server,
				&decodex_protocol::QueryResultEnvelope {
					version: decodex_protocol::CURRENT_VERSION,
					query_id: query.query_id.clone(),
					server_id: server.clone(),
					payload: decodex_protocol::QueryResultPayload::Conversation(
						ConversationResult::Available(ready)
					),
				}
			),
			crate::conversations::ConversationRouteOutcome::Fresh
		);
		assert!(restored.acknowledge_ordinary_control(&control));
		let saved = restored.ordinary_draft("Later unsent text").expect("saved draft");
		assert_eq!(saved.unconfirmed, vec![input]);
		assert_eq!(saved.composer.text, "Later unsent text");
		assert!(!restored.acknowledge_ordinary_control(&control));
		assert!(restored.try_take_dispatch(1, &server).is_none_or(|next| next.command().is_none()));
	}
	#[test]
	fn changed_target_and_disconnection_invalidate_control_observations() {
		use crate::conversations::tests::{recorded_archive_fixture, reply_archive_check};
		let (controller, server, original) = recorded_archive_fixture();
		assert!(controller.check_ordinary_control(&original));
		let dispatch = controller.try_take_dispatch(1, &server).expect("query");
		let query = dispatch.query().expect("read only");
		let target = controller.lock().tasks[0].conversation_id.clone();
		controller.lock().tasks[0].conversation_revision = EntityRevision(2);
		let reply = decodex_protocol::QueryResultEnvelope {
			version: decodex_protocol::CURRENT_VERSION,
			query_id: query.query_id.clone(),
			server_id: server.clone(),
			payload: decodex_protocol::QueryResultPayload::Conversation(
				ConversationResult::Archived {
					conversation_id: target,
					conversation_revision: EntityRevision(2),
				},
			),
		};
		assert_eq!(
			controller.route_query_result(1, &server, &reply),
			crate::conversations::ConversationRouteOutcome::Refused
		);
		assert!(!controller.acknowledge_ordinary_control(&original));
		assert!(controller.check_ordinary_control(&original));
		reply_archive_check(&controller, &server, &original);
		assert!(controller.ordinary_control_states()[0].1.expect("observation").can_acknowledge());
		controller.session_ended(1);
		assert!(!controller.acknowledge_ordinary_control(&original));
		assert_eq!(controller.ordinary_control_states()[0].1, None);
	}
	#[test]
	fn routing_controls_require_newer_definite_progress() {
		let (mut command, mut current) = fixture();
		let target = current.conversation_id.clone();
		for establishment in [false, true] {
			command.payload = if establishment {
				CommandPayload::ResumeConversationEstablishment { conversation_id: target.clone() }
			} else {
				CommandPayload::ResumeConversationRouting { conversation_id: target.clone() }
			};
			for (revision, state, expected) in [
				(1, ConversationState::Ready, ControlObservation::Conflict),
				(2, ConversationState::OutcomeUnknown, ControlObservation::Conflict),
				(2, ConversationState::RoutingPending, ControlObservation::Conflict),
				(2, ConversationState::Ready, ControlObservation::RoutingAdvanced),
			] {
				current.conversation_revision = EntityRevision(revision);
				current.state = state;
				assert_eq!(
					observe(&command, &target, &ConversationResult::Available(current.clone())),
					expected
				);
			}
		}
		command.payload =
			CommandPayload::CreateConversationRoutingSuccessor { conversation_id: target.clone() };
		let archived = ConversationResult::Archived {
			conversation_id: target.clone(),
			conversation_revision: EntityRevision(2),
		};
		assert_eq!(observe(&command, &target, &archived), ControlObservation::Conflict);
		for (revision, expected) in
			[(2, ControlObservation::RoutingAdvanced), (3, ControlObservation::Conflict)]
		{
			let redirect = ConversationResult::RoutingSuccessorRedirect {
				source_conversation_id: target.clone(),
				source_conversation_revision: EntityRevision(revision),
				successor_conversation_id: decodex_protocol::EntityId::new(
					"00000000-0000-4000-8000-000000000099",
				)
				.expect("id"),
				successor_conversation_revision: EntityRevision(7),
			};
			assert_eq!(observe(&command, &target, &redirect), expected);
		}
	}
}
