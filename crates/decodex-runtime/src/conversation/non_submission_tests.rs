//! Qualify the runtime receipt, revision readback and manual recovery together.
use crate::{
	account_service::{CredentialRefreshError, CredentialRefreshPort, CredentialRefreshResult},
	conversation as c,
	host_credentials::CredentialSecretBundle,
};
use decodex_core::{
	BlobStore, DecodexRoot, ProcessExecutionAuthorization, ProcessExecutionEpochId,
};
use decodex_database::SqliteStore;
use std::sync::Arc;

const CONVERSATION: &str = "44000000-0000-4000-8000-000000000001";
const TURN: &str = "45000000-0000-4000-8000-000000000001";
const SESSION: &str = "41000000-0000-4000-8000-000000000001";
const ACCOUNT: &str = "46000000-0000-4000-8000-000000000001";
const GENERATION: &str = "42000000-0000-4000-8000-000000000001";
const ATTEMPT: &str = "50000000-0000-4000-8000-000000000001";
const REQUEST: &str = "51000000-0000-4000-8000-000000000001";
const EPOCH: &str = "43000000-0000-4000-8000-000000000001";

struct NoRefresh;
impl CredentialRefreshPort for NoRefresh {
	fn refresh(
		&self,
		_: &CredentialSecretBundle,
	) -> Result<CredentialRefreshResult, CredentialRefreshError> {
		panic!("non-submission recovery must not refresh credentials")
	}
}

async fn runtime(root: &DecodexRoot, store: &SqliteStore) -> c::ConversationRuntime {
	let accounts = Arc::new(c::AccountService::new(
		store.clone(),
		Arc::new(crate::host_credentials::SqliteCredentialStore::new(store.clone())),
		Arc::new(NoRefresh),
	));
	c::ConversationRuntime::new(
		store.clone(),
		BlobStore::open(root.paths()).expect("fixture blobs"),
		accounts,
		c::ProcessGenerationControl::start(store.clone()).await.expect("fixture supervisor"),
		c::ProviderAttemptControl::start(store.clone()).await.expect("fixture evidence owner"),
		ProcessExecutionAuthorization::new(
			ProcessExecutionEpochId::new(EPOCH).expect("fixture epoch"),
			"c".repeat(64),
		)
		.expect("fixture authorization"),
		crate::account_launch::process::tests::ordinary_runtime_fixture_profile(root.as_path()),
		c::RunnerCapacity::daemon().expect("fixture capacity"),
	)
}

fn seed(root: &DecodexRoot, failure: &str) {
	let connection =
		rusqlite::Connection::open(root.paths().product_database_file()).expect("fixture database");
	connection
		.execute_batch(include_str!("../../tests/fixtures/opaque_resume_authority.sql"))
		.expect("fixture authority");
	connection.execute("INSERT INTO provider_attempts (
        attempt_id, conversation_id, turn_id, continuation_plan_id, routing_decision_id,
        runtime_session_id, runtime_session_revision, account_id, process_generation_id,
        process_generation_revision, execution_epoch_id, request_id, request_sha256,
        provider_correlation_key, state, unknown_reason, revision, created_at_micros, updated_at_micros
    ) VALUES (?1, ?2, ?3, '4a000000-0000-4000-8000-000000000001',
        '4b000000-0000-4000-8000-000000000001', ?4, 4, ?5, ?6, 3, ?7, ?8, ?9,
        'original-provider-key', 'unknown', 'dispatch_outcome_unavailable', 1, 1, 1)",
        rusqlite::params![ATTEMPT, CONVERSATION, TURN, SESSION, ACCOUNT, GENERATION, EPOCH, REQUEST, "f".repeat(64)]).expect("fixture attempt");
	if failure == "evidence" {
		connection.execute_batch("CREATE TRIGGER reject_non_submission BEFORE INSERT ON history_items WHEN json_extract(NEW.metadata_json,'$.type')='native_turn_not_submitted' BEGIN SELECT RAISE(ABORT,'fixture failure'); END;").expect("fixture failure injection");
	}
	if failure == "readback" {
		connection.execute_batch("CREATE TRIGGER replace_non_submission_session AFTER INSERT ON history_items WHEN json_extract(NEW.metadata_json,'$.type')='native_turn_not_submitted' BEGIN UPDATE runtime_sessions SET codex_thread_id='replaced-thread'; END;").expect("fixture readback failure");
	}
}

fn session() -> c::LocalSession {
	c::LocalSession {
		operation_key: "original".into(),
		correlation_id: "fixture".into(),
		causation_id: None,
		conversation_id: c::ConversationId::new(CONVERSATION).expect("fixture conversation"),
		conversation_revision: 1,
		runtime_session_id: c::RuntimeSessionId::new(SESSION).expect("fixture session"),
		runtime_session_revision: 4,
		codex_thread_id: "provider/thread?after#restart%opaque".into(),
		has_acknowledged_turn: true,
		account_id: c::AccountId::new(ACCOUNT).expect("fixture account"),
		process: c::FencedProcess::for_test(
			c::ProcessGenerationId::new(GENERATION).expect("fixture generation"),
			3,
		),
		model: "gpt-5.6-sol".into(),
		reasoning_effort: Some("high".into()),
		fast: false,
		service_tier: decodex_core::ServiceTier::new("default").expect("fixture tier"),
		working_directory: "/fixture".into(),
		instructions: "Follow the request.".into(),
		next_user_sequence: 3,
		execution_overrides: None,
	}
}

#[tokio::test]
async fn ordinary_non_submission_runtime_preserves_atomic_evidence_and_manual_recovery() {
	for failure in ["none", "evidence", "readback"] {
		let temp = tempfile::tempdir().expect("fixture root");
		let root = DecodexRoot::new(temp.path().canonicalize().expect("fixture path"))
			.expect("fixture root");
		let store = SqliteStore::open(&root.paths()).expect("fixture store");
		let runtime = runtime(&root, &store).await;
		seed(&root, failure);
		let session = session();
		runtime.local().insert(
			CONVERSATION.into(),
			c::LocalTask {
				operation_key: "original".into(),
				state: c::LocalTaskState::Preparing(session.clone()),
			},
		);
		let outcome = runtime
			.finish_native_non_submission(
				session,
				c::TurnId::new(TURN).expect("fixture turn"),
				c::ProviderAttemptId::new(ATTEMPT).expect("fixture attempt"),
				c::ProviderRequestId::new(REQUEST).expect("fixture request"),
				c::ProviderRequestKey::new("original-provider-key").expect("fixture key"),
				"9".repeat(64),
			)
			.await;
		rusqlite::Connection::open(root.paths().product_database_file()).expect("fixture database")
            .execute_batch("DROP TRIGGER IF EXISTS reject_non_submission; DROP TRIGGER IF EXISTS replace_non_submission_session;").expect("remove failure injection before schema validation");
		let reopened = SqliteStore::open(&root.paths()).expect("reopen fixture");
		let attempt = reopened
			.read_provider_attempt(&c::ProviderAttemptId::new(ATTEMPT).expect("fixture attempt"))
			.await
			.expect("attempt read")
			.expect("attempt");
		if failure != "none" {
			assert!(matches!(outcome, c::ConversationOutcome::Unknown { .. }));
			assert_eq!(
				attempt.state,
				if failure == "evidence" {
					c::ProviderAttemptState::Unknown
				} else {
					c::ProviderAttemptState::NotSubmitted
				}
			);
			assert!(runtime.inner.event_receiver.lock().await.try_recv().is_err());
		} else {
			let c::ConversationOutcome::ManualRecovery {
				readback,
				action: c::ConversationManualRecovery::ProcessUnavailable,
			} = outcome
			else {
				panic!("manual recovery expected: {outcome:?}")
			};
			let saved = reopened
				.read_ordinary_runtime_session_for_resume(&readback.conversation_id)
				.await
				.expect("session read")
				.expect("session");
			assert_eq!(readback.conversation_revision, Some(saved.conversation_revision));
			assert_eq!(readback.runtime_session_revision, Some(saved.runtime_session_revision));
			assert_eq!(attempt.state, c::ProviderAttemptState::NotSubmitted);
			assert!(matches!(
				runtime.inner.event_receiver.lock().await.try_recv().expect("history event"),
				c::ConversationOutcome::HistoryChanged { .. }
			));
			assert!(matches!(
				runtime.local().get(CONVERSATION).map(|t| &t.state),
				Some(c::LocalTaskState::Recovery { .. })
			));
		}
	}
}
