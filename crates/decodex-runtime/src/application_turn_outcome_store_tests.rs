//! Reopen and verify actual stored provider outcomes, including missing evidence.
use super::*;
use decodex_core::{
	DecodexRoot, ProviderEvidenceId, ProviderEvidenceSource, ProviderPositiveEvidence,
	ProviderRequestId, ProviderRequestKey, ProviderTerminalOutcome,
};
use decodex_database::SqliteStore;
use decodex_protocol::{EntityId, IdempotencyKey};

fn request() -> ConversationTurnOutcomeRequest {
	ConversationTurnOutcomeRequest {
		idempotency_key: IdempotencyKey::new("original").unwrap(),
		conversation_id: EntityId::new("44000000-0000-4000-8000-000000000001").unwrap(),
		turn_id: EntityId::new("45000000-0000-4000-8000-000000000001").unwrap(),
	}
}

fn seed(root: &DecodexRoot, request: &ConversationTurnOutcomeRequest) {
	drop(SqliteStore::open(&root.paths()).unwrap());
	let connection = rusqlite::Connection::open(root.paths().product_database_file()).unwrap();
	connection
		.execute_batch(include_str!("../tests/fixtures/opaque_resume_authority.sql"))
		.unwrap();
	let id = ordinary_provider_attempt_id(
		request.idempotency_key.as_str(),
		&TurnId::new(request.turn_id.as_str()).unwrap(),
	)
	.unwrap();
	connection.execute("INSERT INTO provider_attempts (
        attempt_id, conversation_id, turn_id, continuation_plan_id, routing_decision_id,
        runtime_session_id, runtime_session_revision, account_id, process_generation_id,
        process_generation_revision, execution_epoch_id, request_id, request_sha256,
        provider_correlation_key, state, unknown_reason, revision, created_at_micros, updated_at_micros
    ) VALUES (?1, ?2, ?3, '4a000000-0000-4000-8000-000000000001',
        '4b000000-0000-4000-8000-000000000001', '41000000-0000-4000-8000-000000000001', 4,
        '46000000-0000-4000-8000-000000000001', '42000000-0000-4000-8000-000000000001', 3,
        '43000000-0000-4000-8000-000000000001', '51000000-0000-4000-8000-000000000001', ?4,
        'original-provider-key', 'unknown', 'dispatch_outcome_unavailable', 1, 1, 1)",
        rusqlite::params![id.as_str(), request.conversation_id.as_str(), request.turn_id.as_str(), "f".repeat(64)]).unwrap();
	connection
		.execute(
			"UPDATE turns SET status='failed', revision=revision+1 WHERE turn_id=?1",
			[request.turn_id.as_str()],
		)
		.unwrap();
}

#[tokio::test]
async fn stored_turn_outcomes_require_exact_consumer_and_real_terminal_evidence() {
	for (terminal, expected) in [
		(ProviderTerminalOutcome::Succeeded, StateDto::Completed),
		(ProviderTerminalOutcome::FailedDefinitive, StateDto::Failed),
		(ProviderTerminalOutcome::NotSubmitted, StateDto::NotSubmitted),
	] {
		let temp = tempfile::tempdir().unwrap();
		let root = DecodexRoot::new(temp.path().canonicalize().unwrap()).unwrap();
		let request = request();
		seed(&root, &request);
		let store = SqliteStore::open(&root.paths()).unwrap();
		let owner = ProductStore::Available(store.clone());
		let observed = |outcome| ResultDto::Observed {
			conversation_id: request.conversation_id.clone(),
			turn_id: request.turn_id.clone(),
			outcome,
		};
		assert_eq!(
			query_turn_outcome(&owner, &request).await,
			observed(StateDto::Unknown),
			"local failed turn is not provider failure"
		);
		let mut foreign = request.clone();
		foreign.conversation_id = EntityId::new("44000000-0000-4000-8000-000000000099").unwrap();
		assert_eq!(query_turn_outcome(&owner, &foreign).await, ResultDto::Conflict);
		let attempt = ordinary_provider_attempt_id(
			request.idempotency_key.as_str(),
			&TurnId::new(request.turn_id.as_str()).unwrap(),
		)
		.unwrap();
		let evidence = ProviderPositiveEvidence::new(
			ProviderEvidenceId::new("52000000-0000-4000-8000-000000000001").unwrap(),
			attempt,
			ProviderRequestId::new("51000000-0000-4000-8000-000000000001").unwrap(),
			ProviderEvidenceSource::PositiveIdempotencyLookup,
			terminal,
			ProviderRequestKey::new("original-provider-key").unwrap(),
			None,
			None,
			None,
			"a".repeat(64),
		)
		.unwrap();
		assert!(matches!(
			store.record_provider_attempt_positive_evidence(1, &evidence).await.unwrap(),
			decodex_database::ProviderAttemptMutationOutcome::Applied(_)
		));
		drop(owner);
		drop(store);
		let owner = ProductStore::Available(SqliteStore::open(&root.paths()).unwrap());
		assert_eq!(query_turn_outcome(&owner, &request).await, observed(expected));
		let connection = rusqlite::Connection::open(root.paths().product_database_file()).unwrap();
		connection
			.execute(
				"DELETE FROM provider_attempt_positive_evidence WHERE evidence_id=?1",
				[evidence.evidence_id.as_str()],
			)
			.unwrap();
		assert_eq!(
			query_turn_outcome(&owner, &request).await,
			ResultDto::Unavailable,
			"a dangling evidence ID is not positive evidence"
		);
	}
}
