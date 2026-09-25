//! Observe existing provider-attempt authority without creating an execution journal.
use super::ProductStore;
use crate::conversation::ordinary_provider_attempt_id;
use decodex_core::{ConversationId, ProviderAttemptConsumer, ProviderAttemptState, TurnId};
use decodex_protocol::{
	ConversationTurnOutcomeRequest, ConversationTurnOutcomeResult as ResultDto,
	ConversationTurnOutcomeState as StateDto,
};

pub(super) async fn query_turn_outcome(
	store: &ProductStore,
	request: &ConversationTurnOutcomeRequest,
) -> ResultDto {
	let ProductStore::Available(store) = store else {
		return ResultDto::Unavailable;
	};
	let (Ok(conversation), Ok(turn)) = (
		ConversationId::new(request.conversation_id.as_str()),
		TurnId::new(request.turn_id.as_str()),
	) else {
		return ResultDto::Conflict;
	};
	let Ok(attempt_id) = ordinary_provider_attempt_id(request.idempotency_key.as_str(), &turn)
	else {
		return ResultDto::Conflict;
	};
	let attempt = match store.read_provider_attempt(&attempt_id).await {
		Ok(Some(attempt)) => attempt,
		Ok(None) => return ResultDto::NotRecorded,
		Err(_) => return ResultDto::Unavailable,
	};
	let expected =
		ProviderAttemptConsumer::ConversationTurn { conversation_id: conversation, turn_id: turn };
	if attempt.consumer != expected {
		return ResultDto::Conflict;
	}
	let evidence_matches = match store.provider_attempt_terminal_evidence_matches(&attempt).await {
		Ok(matches) => matches,
		Err(_) => return ResultDto::Unavailable,
	};
	let Some(outcome) = project_state(attempt.state, evidence_matches) else {
		return ResultDto::Unavailable;
	};
	ResultDto::Observed {
		conversation_id: request.conversation_id.clone(),
		turn_id: request.turn_id.clone(),
		outcome,
	}
}

fn project_state(state: ProviderAttemptState, terminal_evidence: bool) -> Option<StateDto> {
	use ProviderAttemptState as Source;
	match state {
		Source::Prepared | Source::DispatchAuthorized => Some(StateDto::Pending),
		Source::Unknown => Some(StateDto::Unknown),
		Source::Canceled => Some(StateDto::NotSubmitted),
		Source::Succeeded if terminal_evidence => Some(StateDto::Completed),
		Source::FailedDefinitive if terminal_evidence => Some(StateDto::Failed),
		Source::NotSubmitted if terminal_evidence => Some(StateDto::NotSubmitted),
		Source::Succeeded | Source::FailedDefinitive | Source::NotSubmitted => None,
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn outcome_lookup_preserves_the_existing_attempt_identity() {
		let turn = TurnId::new("50000000-0000-4000-8000-000000000001").unwrap();
		assert_eq!(
			ordinary_provider_attempt_id("original-submission", &turn).unwrap().as_str(),
			"7dc84ac4-d679-42cd-8f43-235a20459b8d"
		);
	}

	#[test]
	fn provider_terminal_results_require_positive_evidence() {
		use ProviderAttemptState as Source;
		for (source, expected) in [
			(Source::Succeeded, StateDto::Completed),
			(Source::FailedDefinitive, StateDto::Failed),
			(Source::NotSubmitted, StateDto::NotSubmitted),
		] {
			assert_eq!(project_state(source, false), None);
			assert_eq!(project_state(source, true), Some(expected));
		}
		assert_eq!(project_state(Source::Unknown, false), Some(StateDto::Unknown));
		assert_eq!(project_state(Source::Prepared, false), Some(StateDto::Pending));
		assert_eq!(project_state(Source::DispatchAuthorized, false), Some(StateDto::Pending));
		assert_eq!(project_state(Source::Canceled, false), Some(StateDto::NotSubmitted));
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
			query_turn_outcome(&ProductStore::Available(store), &request).await,
			ResultDto::NotRecorded
		);
	}
}

#[cfg(test)]
#[path = "application_turn_outcome_store_tests.rs"]
mod store_tests;
