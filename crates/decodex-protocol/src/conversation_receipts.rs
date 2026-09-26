//! Exact local creation receipt readback, without command replay.
use crate::{
	CommandEnvelope, CommandPayload, ConversationExecutionSettings, ConversationWorkingDirectory,
	EntityId, EntityRevision, HistoryText, IdempotencyKey,
};
use serde::{Deserialize, Serialize};

/// Original creation coordinates used by the durable request fingerprint.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationCreationReceiptRequest {
	/// Original logical command key.
	pub idempotency_key: IdempotencyKey,
	/// Original local conversation identity.
	pub conversation_id: EntityId,
	/// Original input, not the current composer text.
	pub message: HistoryText,
	/// Original working directory.
	pub working_directory: ConversationWorkingDirectory,
	/// Original execution choices, including inherited reasoning effort.
	pub execution: ConversationExecutionSettings,
	/// Original account observation, when creation used native model discovery.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub initial_model_source: Option<Box<crate::InitialModelSource>>,
}

impl ConversationCreationReceiptRequest {
	/// Extract a creation request without admitting or replaying its command.
	pub fn from_command(command: &CommandEnvelope) -> Option<Self> {
		if command.expected_revision.is_some() {
			return None;
		}
		let CommandPayload::CreateConversation {
			conversation_id,
			message,
			working_directory,
			execution,
			initial_model_source,
		} = &command.payload
		else {
			return None;
		};
		Some(Self {
			idempotency_key: command.idempotency_key.clone(),
			conversation_id: conversation_id.clone(),
			message: message.clone(),
			working_directory: working_directory.clone(),
			execution: execution.clone(),
			initial_model_source: initial_model_source.clone(),
		})
	}
}

/// Local persistence evidence. None of these states asserts provider completion.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum ConversationCreationReceiptResult {
	/// The exact request created its local conversation.
	Recorded {
		/// Conversation recorded by the original creation transaction.
		conversation_id: EntityId,
		/// Revision recorded at creation, not the current revision.
		creation_revision: EntityRevision,
	},
	/// No exact receipt was found; this does not prove that no effect occurred.
	NotRecorded,
	/// The key belongs to another request or conversation.
	Conflict,
	/// The receipt could not be read or decoded.
	Unavailable,
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{
		CURRENT_VERSION, ClientMessage, QueryEnvelope, QueryId, QueryPayload, decode_client_message,
	};

	#[test]
	fn creation_receipt_is_a_read_query_and_preserves_original_choices() {
		let request = ConversationCreationReceiptRequest {
			initial_model_source: None,
			idempotency_key: IdempotencyKey::new("original-command").unwrap(),
			conversation_id: EntityId::new("30000000-0000-4000-8000-000000000001").unwrap(),
			message: HistoryText::new("Original message").unwrap(),
			working_directory: ConversationWorkingDirectory::new("/tmp").unwrap(),
			execution: ConversationExecutionSettings {
				model: crate::ConversationModel::new("native-model").unwrap(),
				reasoning_effort: None,
				fast: false,
				service_tier: None,
			},
		};
		let query = ClientMessage::Query(QueryEnvelope {
			version: CURRENT_VERSION,
			query_id: QueryId::new("read-original").unwrap(),
			payload: QueryPayload::GetConversationCreationReceipt { request: request.clone() },
		});
		assert_eq!(decode_client_message(&serde_json::to_string(&query).unwrap()).unwrap(), query);
		let mut invalid = serde_json::to_value(&query).unwrap();
		invalid["body"]["payload"]["arguments"]["request"]["conversation_id"] = "wrong".into();
		assert!(decode_client_message(&invalid.to_string()).is_err());
		let mut invalid = serde_json::to_value(&query).unwrap();
		invalid["body"]["payload"]["arguments"]["request"]["message"] = "  ".into();
		assert!(decode_client_message(&invalid.to_string()).is_err());
		for result in [
			ConversationCreationReceiptResult::NotRecorded,
			ConversationCreationReceiptResult::Conflict,
			ConversationCreationReceiptResult::Unavailable,
			ConversationCreationReceiptResult::Recorded {
				conversation_id: request.conversation_id,
				creation_revision: EntityRevision(1),
			},
		] {
			let encoded = serde_json::to_string(&result).unwrap();
			assert_eq!(
				serde_json::from_str::<ConversationCreationReceiptResult>(&encoded).unwrap(),
				result
			);
		}
	}
}
