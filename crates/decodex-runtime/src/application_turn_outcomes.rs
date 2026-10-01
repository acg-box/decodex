//! Observe existing provider-attempt authority without creating an execution journal.
use crate::{application::ProductStore, conversation::ordinary_provider_attempt_id};
use decodex_core::{ConversationId, ProviderAttemptConsumer, ProviderAttemptState, TurnId};
use decodex_protocol::{
	ConversationTurnOutcomeRequest, ConversationTurnOutcomeResult, ConversationTurnOutcomeState,
};

pub(super) async fn query_turn_outcome(
	store: &ProductStore,
	request: &ConversationTurnOutcomeRequest,
) -> ConversationTurnOutcomeResult {
	let ProductStore::Available(store) = store else {
		return ConversationTurnOutcomeResult::Unavailable;
	};
	let (Ok(conversation), Ok(turn)) = (
		ConversationId::new(request.conversation_id.as_str()),
		TurnId::new(request.turn_id.as_str()),
	) else {
		return ConversationTurnOutcomeResult::Conflict;
	};
	let Ok(attempt_id) = ordinary_provider_attempt_id(request.idempotency_key.as_str(), &turn)
	else {
		return ConversationTurnOutcomeResult::Conflict;
	};
	let attempt = match store.read_provider_attempt(&attempt_id).await {
		Ok(Some(attempt)) => attempt,
		Ok(None) => return ConversationTurnOutcomeResult::NotRecorded,
		Err(_) => return ConversationTurnOutcomeResult::Unavailable,
	};
	let expected =
		ProviderAttemptConsumer::ConversationTurn { conversation_id: conversation, turn_id: turn };

	if attempt.consumer != expected {
		return ConversationTurnOutcomeResult::Conflict;
	}

	let evidence_matches = match store.provider_attempt_terminal_evidence_matches(&attempt).await {
		Ok(matches) => matches,
		Err(_) => return ConversationTurnOutcomeResult::Unavailable,
	};
	let Some(outcome) = project_state(attempt.state, evidence_matches) else {
		return ConversationTurnOutcomeResult::Unavailable;
	};

	ConversationTurnOutcomeResult::Observed {
		conversation_id: request.conversation_id.clone(),
		turn_id: request.turn_id.clone(),
		outcome,
	}
}

fn project_state(
	state: ProviderAttemptState,
	terminal_evidence: bool,
) -> Option<ConversationTurnOutcomeState> {
	match state {
		ProviderAttemptState::Prepared | ProviderAttemptState::DispatchAuthorized =>
			Some(ConversationTurnOutcomeState::Pending),
		ProviderAttemptState::Unknown => Some(ConversationTurnOutcomeState::Unknown),
		ProviderAttemptState::Canceled => Some(ConversationTurnOutcomeState::NotSubmitted),
		ProviderAttemptState::Succeeded if terminal_evidence =>
			Some(ConversationTurnOutcomeState::Completed),
		ProviderAttemptState::FailedDefinitive if terminal_evidence =>
			Some(ConversationTurnOutcomeState::Failed),
		ProviderAttemptState::NotSubmitted if terminal_evidence =>
			Some(ConversationTurnOutcomeState::NotSubmitted),
		ProviderAttemptState::Succeeded
		| ProviderAttemptState::FailedDefinitive
		| ProviderAttemptState::NotSubmitted => None,
	}
}

#[cfg(test)]
mod tests {
	use crate::application::turn_outcomes::{
		self, ConversationTurnOutcomeRequest, ConversationTurnOutcomeResult,
		ConversationTurnOutcomeState, ProductStore, ProviderAttemptState, TurnId,
	};
	#[test]
	fn outcome_lookup_preserves_the_existing_attempt_identity() {
		let turn = TurnId::new("50000000-0000-4000-8000-000000000001").unwrap();

		assert_eq!(
			turn_outcomes::ordinary_provider_attempt_id("original-submission", &turn)
				.unwrap()
				.as_str(),
			"7dc84ac4-d679-42cd-8f43-235a20459b8d"
		);
	}

	#[test]
	fn provider_terminal_results_require_positive_evidence() {
		for (source, expected) in [
			(ProviderAttemptState::Succeeded, ConversationTurnOutcomeState::Completed),
			(ProviderAttemptState::FailedDefinitive, ConversationTurnOutcomeState::Failed),
			(ProviderAttemptState::NotSubmitted, ConversationTurnOutcomeState::NotSubmitted),
		] {
			assert_eq!(turn_outcomes::project_state(source, false), None);
			assert_eq!(turn_outcomes::project_state(source, true), Some(expected));
		}

		assert_eq!(
			turn_outcomes::project_state(ProviderAttemptState::Unknown, false),
			Some(ConversationTurnOutcomeState::Unknown)
		);
		assert_eq!(
			turn_outcomes::project_state(ProviderAttemptState::Prepared, false),
			Some(ConversationTurnOutcomeState::Pending)
		);
		assert_eq!(
			turn_outcomes::project_state(ProviderAttemptState::DispatchAuthorized, false),
			Some(ConversationTurnOutcomeState::Pending)
		);
		assert_eq!(
			turn_outcomes::project_state(ProviderAttemptState::Canceled, false),
			Some(ConversationTurnOutcomeState::NotSubmitted)
		);
	}
	#[tokio::test]
	async fn missing_attempt_is_not_reported_as_not_submitted() {
		let temp = tempfile::tempdir().unwrap();
		let root = decodex_core::DecodexRoot::new(temp.path().canonicalize().unwrap()).unwrap();
		let store = decodex_database::SqliteStore::open(&root.paths()).unwrap();
		let request = ConversationTurnOutcomeRequest {
			idempotency_key: decodex_protocol::IdempotencyKey::new("original").unwrap(),
			conversation_id: decodex_protocol::EntityId::new(
				"30000000-0000-4000-8000-000000000001",
			)
			.unwrap(),
			turn_id: decodex_protocol::EntityId::new("50000000-0000-4000-8000-000000000001")
				.unwrap(),
		};

		assert_eq!(
			turn_outcomes::query_turn_outcome(&ProductStore::Available(store), &request).await,
			ConversationTurnOutcomeResult::NotRecorded
		);
	}
}
#[cfg(test)]
#[path = "application_turn_outcome_store_tests.rs"]
mod store_tests;
