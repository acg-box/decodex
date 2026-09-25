//! Observe the original provider attempt without replaying a saved message.
use super::{
	CommandEnvelope, ConversationCommandState, ConversationQueryPurpose, ConversationRouteOutcome,
	Conversations, State,
};
use decodex_protocol::{
	ConversationTurnOutcomeRequest as Request, ConversationTurnOutcomeResult as Result,
	ConversationTurnOutcomeState as Outcome, QueryPayload, QueryResultPayload,
};

impl Conversations {
	pub(crate) fn ordinary_turn_outcomes(&self) -> Vec<(CommandEnvelope, Option<Result>)> {
		let state = self.lock();
		state
			.delivery
			.unconfirmed
			.iter()
			.filter(|command| Request::from_command(command).is_some())
			.map(|command| {
				(
					command.clone(),
					state
						.delivery
						.turn_readbacks
						.iter()
						.find(|(original, _)| original == command)
						.map(|(_, result)| result.clone()),
				)
			})
			.collect()
	}

	pub(crate) fn check_ordinary_turn(&self, command: &CommandEnvelope) -> bool {
		let Some(request) = Request::from_command(command) else { return false };
		let mut state = self.lock();
		if !state.delivery.unconfirmed.contains(command) {
			return false;
		}
		let queued = state.queue_query(
			QueryPayload::GetConversationTurnOutcome { request },
			ConversationQueryPurpose::TurnOutcome { command: Box::new(command.clone()) },
		);
		if queued {
			state.delivery.turn_readbacks.retain(|(original, _)| original != command);
		}
		drop(state);
		if queued {
			self.inner.notify.notify_one();
		}
		queued
	}

	pub(crate) fn acknowledge_ordinary_turn(&self, command: &CommandEnvelope) -> Option<Outcome> {
		let mut state = self.lock();
		if state.pending_command.is_some()
			|| state.in_flight_command.is_some()
			|| !state.delivery.unconfirmed.contains(command)
		{
			return None;
		}
		let outcome = state.delivery.turn_readbacks.iter().find_map(|(original, result)| {
			if original != command {
				return None;
			}
			match result {
				Result::Observed {
					outcome:
						outcome @ (Outcome::Completed | Outcome::Failed | Outcome::NotSubmitted),
					..
				} => Some(*outcome),
				_ => None,
			}
		})?;
		state.confirm_delivery(command);
		state.delivery.turn_readbacks.retain(|(original, _)| original != command);
		if state.delivery.unconfirmed.is_empty() {
			state.command = ConversationCommandState::Idle;
		}
		Some(outcome)
	}
}

impl State {
	pub(super) fn route_turn_outcome(
		&mut self,
		command: &CommandEnvelope,
		payload: &QueryResultPayload,
	) -> (ConversationRouteOutcome, bool) {
		let Some(request) = Request::from_command(command) else {
			return (ConversationRouteOutcome::Refused, false);
		};
		if !self.delivery.unconfirmed.contains(command) {
			return (ConversationRouteOutcome::Unmatched, false);
		}
		let result = match payload {
			QueryResultPayload::ConversationTurnOutcome(result) => result.clone(),
			_ => Result::Unavailable,
		};
		if let Result::Observed { conversation_id, turn_id, .. } = &result
			&& (conversation_id != &request.conversation_id || turn_id != &request.turn_id)
		{
			return (ConversationRouteOutcome::Refused, false);
		}
		self.delivery.turn_readbacks.retain(|(original, _)| original != command);
		self.delivery.turn_readbacks.push((command.clone(), result));
		(ConversationRouteOutcome::Fresh, false)
	}
}
