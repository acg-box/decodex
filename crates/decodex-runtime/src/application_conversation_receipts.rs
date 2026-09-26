//! Read durable local creation evidence without starting a runtime or replaying input.
use super::{ProductStore, runtime_execution_settings};
use crate::conversation::CreateConversation;
use decodex_core::ConversationId;
use decodex_database::StoreError;
use decodex_protocol::{
	ConversationCreationReceiptRequest, ConversationCreationReceiptResult, EntityId, EntityRevision,
};

pub(super) async fn query_creation_receipt(
	store: &ProductStore,
	request: &ConversationCreationReceiptRequest,
) -> ConversationCreationReceiptResult {
	use ConversationCreationReceiptResult as Receipt;
	let ProductStore::Available(store) = store else {
		return Receipt::Unavailable;
	};
	let Ok(conversation_id) = ConversationId::new(request.conversation_id.as_str()) else {
		return Receipt::Conflict;
	};
	let command = CreateConversation {
		initial_model_source: match super::runtime_initial_model_source(
			request.initial_model_source.as_deref(),
		) {
			Ok(source) => source,
			Err(()) => return Receipt::Conflict,
		},
		operation_key: request.idempotency_key.as_str().into(),
		correlation_id: String::new(),
		causation_id: None,
		conversation_id: conversation_id.clone(),
		message: request.message.as_str().into(),
		working_directory: request.working_directory.as_str().into(),
		execution: runtime_execution_settings(&request.execution),
	};
	let Ok(identity) = command.creation_identity() else {
		return Receipt::Conflict;
	};
	match store.read_conversation_creation_receipt(&identity, &conversation_id).await {
		Ok(Some(record)) =>
			match (EntityId::new(record.conversation_id.as_str()), u64::try_from(record.revision)) {
				(Ok(id), Ok(revision)) if id == request.conversation_id => Receipt::Recorded {
					conversation_id: id,
					creation_revision: EntityRevision(revision),
				},
				_ => Receipt::Unavailable,
			},
		Ok(None) => Receipt::NotRecorded,
		Err(StoreError::IdempotencyConflict) => Receipt::Conflict,
		Err(_) => Receipt::Unavailable,
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[tokio::test]
	async fn creation_query_distinguishes_exact_local_record_from_missing_and_conflict() {
		let temp = tempfile::tempdir().unwrap();
		let root = decodex_core::DecodexRoot::new(temp.path().canonicalize().unwrap()).unwrap();
		let store = decodex_database::SqliteStore::open(&root.paths()).unwrap();
		let owner = ProductStore::Available(store.clone());
		let mut request = ConversationCreationReceiptRequest {
			initial_model_source: Some(Box::new(decodex_protocol::InitialModelSource {
				account_id: EntityId::new("10000000-0000-4000-8000-000000000001").unwrap(),
				account_revision: 7,
			})),
			idempotency_key: decodex_protocol::IdempotencyKey::new("original").unwrap(),
			conversation_id: EntityId::new("30000000-0000-4000-8000-000000000001").unwrap(),
			message: decodex_protocol::HistoryText::new("Original input").unwrap(),
			working_directory: decodex_protocol::ConversationWorkingDirectory::new("/tmp").unwrap(),
			execution: decodex_protocol::ConversationExecutionSettings {
				model: decodex_protocol::ConversationModel::new("native-model").unwrap(),
				reasoning_effort: None,
				fast: false,
				service_tier: None,
			},
		};
		assert_eq!(
			query_creation_receipt(&owner, &request).await,
			ConversationCreationReceiptResult::NotRecorded
		);
		let command = CreateConversation {
			initial_model_source: super::super::runtime_initial_model_source(
				request.initial_model_source.as_deref(),
			)
			.unwrap(),
			operation_key: "original".into(),
			correlation_id: "correlation".into(),
			causation_id: None,
			conversation_id: ConversationId::new(request.conversation_id.as_str()).unwrap(),
			message: request.message.as_str().into(),
			working_directory: "/tmp".into(),
			execution: runtime_execution_settings(&request.execution),
		};
		store
			.create_conversation(
				&command.creation_identity().unwrap(),
				&decodex_database::CreateConversationRecord {
					initial_model_source: command.initial_model_source.clone(),
					conversation_id: command.conversation_id.clone(),
					title: "Original input".into(),
					message: command.message.clone(),
					working_directory: command.working_directory.clone(),
					model: command.execution.model.clone(),
					reasoning_effort: None,
					fast: false,
					service_tier: None,
				},
			)
			.await
			.unwrap();
		assert_eq!(
			query_creation_receipt(&owner, &request).await,
			ConversationCreationReceiptResult::Recorded {
				conversation_id: request.conversation_id.clone(),
				creation_revision: EntityRevision(1),
			}
		);
		let mut changed_source = request.clone();
		changed_source.initial_model_source.as_mut().unwrap().account_revision = 8;
		assert_eq!(
			query_creation_receipt(&owner, &changed_source).await,
			ConversationCreationReceiptResult::Conflict
		);
		changed_source.initial_model_source = None;
		assert_eq!(
			query_creation_receipt(&owner, &changed_source).await,
			ConversationCreationReceiptResult::Conflict
		);
		request.message = decodex_protocol::HistoryText::new("Later composer text").unwrap();
		assert_eq!(
			query_creation_receipt(&owner, &request).await,
			ConversationCreationReceiptResult::Conflict
		);
		let unavailable =
			ProductStore::Unavailable(super::super::ProductStoreUnavailableReason::Unreachable);
		assert_eq!(
			query_creation_receipt(&unavailable, &request).await,
			ConversationCreationReceiptResult::Unavailable
		);
		assert_eq!(
			store
				.read_conversation_request(&command.conversation_id)
				.await
				.unwrap()
				.unwrap()
				.message,
			"Original input"
		);
	}
}

#[cfg(test)]
#[path = "application_creation_receipt_socket_tests.rs"]
mod socket_tests;
