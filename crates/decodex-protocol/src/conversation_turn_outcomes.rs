//! Read one ordinary provider attempt without replaying its input.
use crate::{CommandEnvelope, CommandPayload, EntityId, IdempotencyKey};
use serde::{Deserialize, Serialize};

/// Stable original coordinates of a later ordinary message.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationTurnOutcomeRequest {
	/// Original logical submission key.
	pub idempotency_key: IdempotencyKey,
	/// Original conversation.
	pub conversation_id: EntityId,
	/// Original client-generated user turn.
	pub turn_id: EntityId,
}
impl ConversationTurnOutcomeRequest {
	/// Extract lookup coordinates without admitting or replaying the submission.
	pub fn from_command(command: &CommandEnvelope) -> Option<Self> {
		let CommandPayload::SubmitConversationTurn { conversation_id, turn_id, .. } =
			&command.payload
		else {
			return None;
		};
		Some(Self {
			idempotency_key: command.idempotency_key.clone(),
			conversation_id: conversation_id.clone(),
			turn_id: turn_id.clone(),
		})
	}
}

/// Durable provider outcome, separate from the local turn's display status.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationTurnOutcomeState {
	/// Preparation or authorized dispatch is not terminal.
	Pending,
	/// Provider effects or completion remain unproved.
	Unknown,
	/// Positive provider evidence confirms success.
	Completed,
	/// Positive provider evidence confirms a definitive failure.
	Failed,
	/// Dispatch was cancelled beforehand or positive evidence proves non-submission.
	NotSubmitted,
}

/// Result of a read-only provider-attempt observation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum ConversationTurnOutcomeResult {
	/// State of the exact original conversation turn.
	Observed {
		/// Matched conversation identity.
		conversation_id: EntityId,
		/// Matched original user turn identity.
		turn_id: EntityId,
		/// Provider evidence, not local history-item status.
		outcome: ConversationTurnOutcomeState,
	},
	/// No provider attempt exists under these coordinates; absence does not prove no effect.
	NotRecorded,
	/// The stored attempt belongs to a different consumer.
	Conflict,
	/// Store or evidence is unavailable or inconsistent.
	Unavailable,
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{
		CURRENT_VERSION, ClientMessage, QueryEnvelope, QueryId, QueryPayload, decode_client_message,
	};
	#[test]
	fn turn_outcome_queries_require_original_canonical_coordinates() {
		let request = ConversationTurnOutcomeRequest {
			idempotency_key: IdempotencyKey::new("original-submission").unwrap(),
			conversation_id: EntityId::new("30000000-0000-4000-8000-000000000001").unwrap(),
			turn_id: EntityId::new("50000000-0000-4000-8000-000000000001").unwrap(),
		};
		let query = ClientMessage::Query(QueryEnvelope {
			version: CURRENT_VERSION,
			query_id: QueryId::new("outcome").unwrap(),
			payload: QueryPayload::GetConversationTurnOutcome { request: request.clone() },
		});
		assert_eq!(decode_client_message(&serde_json::to_string(&query).unwrap()).unwrap(), query);
		for field in ["conversation_id", "turn_id"] {
			let mut invalid = serde_json::to_value(&query).unwrap();
			invalid["body"]["payload"]["arguments"]["request"][field] = "invalid".into();
			assert!(decode_client_message(&invalid.to_string()).is_err());
		}
		let result = ConversationTurnOutcomeResult::Observed {
			conversation_id: request.conversation_id,
			turn_id: request.turn_id,
			outcome: ConversationTurnOutcomeState::Unknown,
		};
		assert_eq!(
			serde_json::from_str::<ConversationTurnOutcomeResult>(
				&serde_json::to_string(&result).unwrap()
			)
			.unwrap(),
			result
		);
	}
}
