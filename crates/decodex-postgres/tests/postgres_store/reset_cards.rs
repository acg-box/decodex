use std::{error::Error, sync::Arc, time::Duration};

use decodex_core::{
	AccountLifecycleReadiness, AccountOperationId, AccountOperationKind, AccountOperationPhase,
	AccountProvider, AccountQuotaWindow, CredentialBinding, CredentialFingerprint,
	CredentialStoreSchemaVersion, CredentialVersion, ProcessGenerationAccountBinding,
	ProviderIdentity, ResetCardConsumeOutcome, ResetCardDescriptor, ResetCardTimestamp,
};
use decodex_postgres::{
	AccountAdministrationOutcome, AccountCommandKind, AccountCommandReceiptClaim, AccountId,
	AccountLifecycleMutationOutcome, AccountLifecycleRejection, AccountOperationPreparation,
	CodexAccountCapabilityAttestation, CommandIdentity, OutboxReconciliation, PostgresStore,
	ReconciliationOutcome, ResetCardClaim, ResetCardFailureCode, ResetCardOperationStatus,
	ResetCardPreparation, StoreError,
};
use serde_json::{Value, json};
use tokio::time;
use tokio_postgres::NoTls;

use super::{expected_peer_uid, owner_runtime_configs};

const ACCOUNT_ID: &str = "71000000-0000-4000-8000-000000000001";
const DISABLE_ACCOUNT_ID: &str = "71000000-0000-4000-8000-000000000002";
const ROTATION_ACCOUNT_ID: &str = "71000000-0000-4000-8000-000000000003";
const ATOMIC_ACCOUNT_ID: &str = "71000000-0000-4000-8000-000000000004";
const DUPLICATE_WINNER_ACCOUNT_ID: &str = "71000000-0000-4000-8000-000000000005";
const DUPLICATE_LOSER_ACCOUNT_ID: &str = "71000000-0000-4000-8000-000000000006";
const DUPLICATE_WINNER_OPERATION_ID: &str = "71000000-0000-4000-8000-000000000021";
const DUPLICATE_LOSER_OPERATION_ID: &str = "71000000-0000-4000-8000-000000000022";
const CONCURRENT_ACCOUNT_A_ID: &str = "71000000-0000-4000-8000-000000000007";
const CONCURRENT_ACCOUNT_B_ID: &str = "71000000-0000-4000-8000-000000000008";
const CONCURRENT_OPERATION_A_ID: &str = "71000000-0000-4000-8000-000000000023";
const CONCURRENT_OPERATION_B_ID: &str = "71000000-0000-4000-8000-000000000024";
const DUPLICATE_PROVIDER_ACCOUNT_ID: &str = "duplicate-provider-account";
const CONCURRENT_PROVIDER_ACCOUNT_ID: &str = "concurrent-duplicate-provider-account";
const GENERIC_WORKER: &str = "72000000-0000-4000-8000-000000000001";
const RESET_WORKER_A: &str = "73000000-0000-4000-8000-000000000001";
const RESET_WORKER_B: &str = "73000000-0000-4000-8000-000000000002";
const RESET_WORKER_C: &str = "73000000-0000-4000-8000-000000000003";
const RESET_WORKER_D: &str = "73000000-0000-4000-8000-000000000004";
const OPERATION_KEY: &str = "reset-card-integration-operation";
const COMPETING_OPERATION_KEY: &str = "reset-card-integration-competing-operation";
const EXHAUSTED_OPERATION_KEY: &str = "reset-card-integration-exhausted-operation";
const NOTHING_TO_RESET_KEY: &str = "reset-card-integration-nothing-to-reset";
const REUSABLE_OPERATION_KEY: &str = "reset-card-integration-reusable-operation";
const PENDING_OPERATION_KEY: &str = "reset-card-integration-pending-operation";
const EXACT_PROVIDER_CREDIT_ID: &str = "sk-live-provider-id";
const CREDENTIAL_FINGERPRINT: &str =
	"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const CALLBACK_PROFILE: &str = "64a98c3328d1eba74aaf18a3995523e07fd2f1395bc6fb4a121b74338c404a29";
const CREDENTIAL_WRITER: &str = "71000000-0000-4000-8000-000000000010";
const ROTATED_CREDENTIAL_FINGERPRINT: &str =
	"dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const ROTATED_CREDENTIAL_WRITER: &str = "71000000-0000-4000-8000-000000000011";
const PROVIDER_ACCOUNT_ID: &str = "reset-card-provider-account";
const DISABLE_PROVIDER_ACCOUNT_ID: &str = "reset-card-disable-provider-account";
const ATOMIC_OPERATION_ID: &str = "71000000-0000-4000-8000-000000000020";
const INITIAL_ACCOUNT_REVISION: i64 = 2;
const CHANGED_ACCOUNT_REVISION: i64 = 3;
const REENABLED_ACCOUNT_REVISION: i64 = 4;
const FINAL_ACCOUNT_REVISION: i64 = 5;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires a fresh isolated PostgreSQL 18 V27 database"]
#[allow(clippy::too_many_lines)] // One complete mutation/receipt rollback and replay proof.
async fn account_terminal_mutation_and_receipt_are_atomic_and_replay_exactly()
-> Result<(), Box<dyn Error>> {
	let (_, runtime) = owner_runtime_configs("DECODEX_TEST")?;
	let store =
		PostgresStore::connect_runtime_fixture(runtime.clone(), expected_peer_uid()).await?;
	let account_id = AccountId::new(ATOMIC_ACCOUNT_ID)?;
	let operation_id = AccountOperationId::new(ATOMIC_OPERATION_ID)?;
	let provider = ProviderIdentity::new(AccountProvider::Chatgpt, "atomic-provider-account")?;
	let target = CredentialBinding {
		schema_version: CredentialStoreSchemaVersion::V1,
		version: CredentialVersion::new(1)?,
		fingerprint: CredentialFingerprint::new(
			"eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
		)?,
		provider: provider.clone(),
		writer_operation_id: operation_id.clone(),
	};
	let prepared = store
		.prepare_account_operation(&AccountOperationPreparation {
			operation_id: operation_id.clone(),
			account_id: account_id.clone(),
			kind: AccountOperationKind::Enroll,
			display_label: Some("Atomic account".to_owned()),
			enabled: Some(true),
			expected_account_revision: None,
			expected: None,
			target: Some(target),
			provider,
		})
		.await?;
	assert!(matches!(
		prepared,
		AccountLifecycleMutationOutcome::Applied(ref mutation)
			if mutation.phase == AccountOperationPhase::Prepared
	));
	let (visible_accounts, routing) = store.read_account_registry_snapshot(512).await?;
	assert_eq!(visible_accounts.len(), 1);
	assert_eq!(visible_accounts[0].account_id, account_id);
	assert_eq!(routing.order, vec![account_id.clone()]);

	store
		.advance_account_operation(
			&operation_id,
			AccountOperationPhase::Prepared,
			AccountOperationPhase::StoreApplied,
			None,
		)
		.await?;
	let failed_command =
		CommandIdentity::new("account-atomic-forced-rollback", b"account-atomic-rollback-v1")?;
	let failed_lease = match store
		.reserve_account_command(
			&failed_command,
			AccountCommandKind::Enroll,
			account_id.as_str(),
			None,
		)
		.await?
	{
		AccountCommandReceiptClaim::Owned(lease) => lease,
		AccountCommandReceiptClaim::Replayed(_) => panic!("fresh rollback command replayed"),
	};
	assert!(matches!(
		store
			.complete_account_operation_command(
				failed_lease,
				&operation_id,
				AccountOperationPhase::StoreApplied,
				AccountOperationPhase::Committed,
				None,
				|_, _, _| Err(StoreError::InvalidInput("forced response failure")),
			)
			.await,
		Err(StoreError::InvalidInput("forced response failure"))
	));
	assert_eq!(
		store
			.read_account_operation(&operation_id)
			.await?
			.expect("operation survives the rolled-back response")
			.phase,
		AccountOperationPhase::StoreApplied,
	);

	let command = CommandIdentity::new("account-atomic-success", b"account-atomic-success-v1")?;
	let lease = match store
		.reserve_account_command(&command, AccountCommandKind::Enroll, account_id.as_str(), None)
		.await?
	{
		AccountCommandReceiptClaim::Owned(lease) => lease,
		AccountCommandReceiptClaim::Replayed(_) => panic!("fresh success command replayed"),
	};
	let response = store
		.complete_account_operation_command(
			lease,
			&operation_id,
			AccountOperationPhase::StoreApplied,
			AccountOperationPhase::Committed,
			None,
			|outcome, operation, account| {
				assert!(matches!(
					outcome,
					AccountLifecycleMutationOutcome::Applied(mutation)
						if mutation.phase == AccountOperationPhase::Committed
				));
				assert_eq!(
					operation.expect("operation is visible in the terminal transaction").phase,
					AccountOperationPhase::Committed,
				);
				let account = account.expect("account is visible in the terminal transaction");
				Ok(json!({
					"schema": "decodex/account-command-result/1",
					"outcome": "succeeded",
					"data": {
						"account_id": account.account_id.as_str(),
						"alias": account.label.as_str(),
						"revision": account.revision,
					},
				}))
			},
		)
		.await?;
	assert_eq!(response["data"]["alias"], "Atomic account");
	assert_eq!(
		store.set_account_enabled(&account_id, 2, false).await?,
		AccountAdministrationOutcome::Updated { revision: 3 },
	);
	let replay = match store
		.reserve_account_command(&command, AccountCommandKind::Enroll, account_id.as_str(), None)
		.await?
	{
		AccountCommandReceiptClaim::Replayed(value) => value,
		AccountCommandReceiptClaim::Owned(_) => panic!("completed command was not replayed"),
	};
	assert_eq!(replay, response);
	assert_eq!(replay["data"]["alias"], "Atomic account");
	let current = &store.read_account_registry(Some(&account_id), 1).await?[0];
	assert_eq!(current.label, "Atomic account");
	assert!(!current.enabled);
	let projection_command =
		CommandIdentity::new("use-in-codex-durable-replay", b"use-in-codex-target-a")?;
	let projection_lease = match store
		.reserve_account_command(
			&projection_command,
			AccountCommandKind::UseInCodex,
			account_id.as_str(),
			Some(3),
		)
		.await?
	{
		AccountCommandReceiptClaim::Owned(lease) => lease,
		AccountCommandReceiptClaim::Replayed(_) => panic!("fresh projection command replayed"),
	};
	let projection_response = json!({
		"schema": "decodex/account-command-result/1",
		"outcome": "succeeded",
		"data": {"account_id": account_id.as_str(), "revision": 3},
	});
	store.complete_account_command(projection_lease, &projection_response).await?;
	assert!(matches!(
		store
			.reserve_account_command(
				&projection_command,
				AccountCommandKind::UseInCodex,
				account_id.as_str(),
				Some(3),
			)
			.await?,
		AccountCommandReceiptClaim::Replayed(value) if value == projection_response
	));
	let conflicting_target =
		CommandIdentity::new("use-in-codex-durable-replay", b"use-in-codex-target-b")?;
	assert!(matches!(
		store
			.reserve_account_command(
				&conflicting_target,
				AccountCommandKind::UseInCodex,
				DUPLICATE_LOSER_ACCOUNT_ID,
				Some(3),
			)
			.await,
		Err(StoreError::IdempotencyConflict)
	));

	store.close();
	Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires a fresh isolated PostgreSQL 18 V27 database"]
async fn duplicate_provider_enrollment_rejects_without_effects_and_replays_exactly()
-> Result<(), Box<dyn Error>> {
	let (schema_owner, runtime) = owner_runtime_configs("DECODEX_TEST")?;
	let store =
		PostgresStore::connect_runtime_fixture(runtime.clone(), expected_peer_uid()).await?;
	let (owner, owner_connection) = schema_owner.connect(NoTls).await?;
	let owner_connection_task = tokio::spawn(owner_connection);

	assert_sequential_duplicate_provider_rejection(&store, &owner).await?;
	assert_concurrent_duplicate_provider_rejection(&store, &owner).await?;

	store.close();
	drop(owner);
	owner_connection_task.await??;
	Ok(())
}

async fn assert_sequential_duplicate_provider_rejection(
	store: &PostgresStore,
	owner: &tokio_postgres::Client,
) -> Result<(), Box<dyn Error>> {
	let winner = duplicate_enrollment_preparation(
		DUPLICATE_WINNER_ACCOUNT_ID,
		DUPLICATE_WINNER_OPERATION_ID,
		DUPLICATE_PROVIDER_ACCOUNT_ID,
		"Duplicate provider winner",
	)?;
	assert!(matches!(
		store.prepare_account_operation(&winner).await?,
		AccountLifecycleMutationOutcome::Applied(ref mutation)
			if mutation.phase == AccountOperationPhase::Prepared
	));
	let counts_before = account_lifecycle_side_effect_counts(owner).await?;
	let loser = duplicate_enrollment_preparation(
		DUPLICATE_LOSER_ACCOUNT_ID,
		DUPLICATE_LOSER_OPERATION_ID,
		DUPLICATE_PROVIDER_ACCOUNT_ID,
		"Duplicate provider loser",
	)?;
	let command = CommandIdentity::new(
		"duplicate-provider-enrollment-loser",
		b"duplicate-provider-enrollment-loser-v1",
	)?;
	let lease = match store
		.reserve_account_command(
			&command,
			AccountCommandKind::Enroll,
			loser.account_id.as_str(),
			None,
		)
		.await?
	{
		AccountCommandReceiptClaim::Owned(lease) => lease,
		AccountCommandReceiptClaim::Replayed(_) => panic!("fresh duplicate command replayed"),
	};
	let rejected = store.prepare_account_operation(&loser).await?;
	assert!(matches!(
		rejected,
		AccountLifecycleMutationOutcome::Rejected {
			rejection: AccountLifecycleRejection::IdentityConflict,
			ref actual,
		} if actual.account_revision == 0 && actual.phase == AccountOperationPhase::Prepared
	));
	let response = provider_mismatch_receipt();
	store.complete_account_command(lease, &response).await?;

	assert_eq!(account_lifecycle_side_effect_counts(owner).await?, counts_before);
	let loser_projection = owner
		.query_one(
			"SELECT \
			 EXISTS(SELECT 1 FROM decodex.accounts WHERE account_id=$1::text::uuid),\
			 EXISTS(SELECT 1 FROM decodex.account_operations \
			  WHERE operation_id=$2::text::uuid OR account_id=$1::text::uuid),\
			 EXISTS(SELECT 1 FROM decodex.account_routing_order \
			  WHERE account_id=$1::text::uuid),\
			 EXISTS(SELECT 1 FROM decodex.accounts \
			  WHERE credential_writer_operation_id=$2::text::uuid)",
			&[&DUPLICATE_LOSER_ACCOUNT_ID, &DUPLICATE_LOSER_OPERATION_ID],
		)
		.await?;
	assert!(!(0..4).any(|index| loser_projection.get::<_, bool>(index)));

	let receipt = owner
		.query_one(
			"SELECT operation,entity_id,receipt_state::text,response,response_bytes,\
			 completed_at IS NOT NULL FROM decodex.command_receipts WHERE idempotency_key=$1",
			&[&"duplicate-provider-enrollment-loser"],
		)
		.await?;
	assert_eq!(receipt.get::<_, &str>(0), "enroll_account");
	assert_eq!(receipt.get::<_, &str>(1), DUPLICATE_LOSER_ACCOUNT_ID);
	assert_eq!(receipt.get::<_, &str>(2), "completed");
	assert_eq!(receipt.get::<_, Value>(3), response);
	assert_eq!(receipt.get::<_, Vec<u8>>(4), serde_json::to_vec(&response)?);
	assert!(receipt.get::<_, bool>(5));
	let replay = match store
		.reserve_account_command(
			&command,
			AccountCommandKind::Enroll,
			loser.account_id.as_str(),
			None,
		)
		.await?
	{
		AccountCommandReceiptClaim::Replayed(value) => value,
		AccountCommandReceiptClaim::Owned(_) => panic!("completed duplicate command was reclaimed"),
	};
	assert_eq!(replay, response);
	Ok(())
}

async fn assert_concurrent_duplicate_provider_rejection(
	store: &PostgresStore,
	owner: &tokio_postgres::Client,
) -> Result<(), Box<dyn Error>> {
	let account_a = AccountId::new(CONCURRENT_ACCOUNT_A_ID)?;
	let account_b = AccountId::new(CONCURRENT_ACCOUNT_B_ID)?;
	let operation_a = AccountOperationId::new(CONCURRENT_OPERATION_A_ID)?;
	let operation_b = AccountOperationId::new(CONCURRENT_OPERATION_B_ID)?;
	let preparation_a = duplicate_enrollment_preparation(
		CONCURRENT_ACCOUNT_A_ID,
		CONCURRENT_OPERATION_A_ID,
		CONCURRENT_PROVIDER_ACCOUNT_ID,
		"Concurrent provider A",
	)?;
	let preparation_b = duplicate_enrollment_preparation(
		CONCURRENT_ACCOUNT_B_ID,
		CONCURRENT_OPERATION_B_ID,
		CONCURRENT_PROVIDER_ACCOUNT_ID,
		"Concurrent provider B",
	)?;
	let counts_before = account_lifecycle_side_effect_counts(owner).await?;
	let barrier = Arc::new(tokio::sync::Barrier::new(3));
	let first_store = store.clone();
	let first_barrier = barrier.clone();
	let first_task = tokio::spawn(async move {
		first_barrier.wait().await;
		first_store.prepare_account_operation(&preparation_a).await
	});
	let second_store = store.clone();
	let second_barrier = barrier.clone();
	let second_task = tokio::spawn(async move {
		second_barrier.wait().await;
		second_store.prepare_account_operation(&preparation_b).await
	});
	barrier.wait().await;
	let first = first_task.await??;
	let second = second_task.await??;
	let (winner_account, loser_account, loser_operation) = match (&first, &second) {
		(
			AccountLifecycleMutationOutcome::Applied(first_mutation),
			AccountLifecycleMutationOutcome::Rejected {
				rejection: AccountLifecycleRejection::IdentityConflict,
				..
			},
		) if first_mutation.phase == AccountOperationPhase::Prepared =>
			(&account_a, &account_b, &operation_b),
		(
			AccountLifecycleMutationOutcome::Rejected {
				rejection: AccountLifecycleRejection::IdentityConflict,
				..
			},
			AccountLifecycleMutationOutcome::Applied(second_mutation),
		) if second_mutation.phase == AccountOperationPhase::Prepared =>
			(&account_b, &account_a, &operation_a),
		_ => panic!("concurrent duplicate enrollment did not produce one typed loser"),
	};
	let counts_after = account_lifecycle_side_effect_counts(owner).await?;
	assert_eq!(counts_after[0], counts_before[0] + 1);
	assert_eq!(counts_after[1], counts_before[1] + 1);
	assert_eq!(counts_after[2], counts_before[2] + 1);
	assert_eq!(counts_after[3], counts_before[3]);
	assert_eq!(counts_after[4], counts_before[4]);
	assert_eq!(counts_after[5], counts_before[5]);
	assert_eq!(counts_after[6], counts_before[6]);
	assert_eq!(counts_after[7], counts_before[7] + 1);
	let provider_rows = owner
		.query(
			"SELECT account_id::text FROM decodex.accounts \
			 WHERE provider_kind='chatgpt' AND provider_account_id=$1 \
			 AND tombstoned_at IS NULL",
			&[&CONCURRENT_PROVIDER_ACCOUNT_ID],
		)
		.await?;
	assert_eq!(provider_rows.len(), 1);
	assert_eq!(provider_rows[0].get::<_, &str>(0), winner_account.as_str());
	let loser_projection = owner
		.query_one(
			"SELECT \
			 EXISTS(SELECT 1 FROM decodex.accounts WHERE account_id=$1::text::uuid),\
			 EXISTS(SELECT 1 FROM decodex.account_operations \
			  WHERE operation_id=$2::text::uuid OR account_id=$1::text::uuid),\
			 EXISTS(SELECT 1 FROM decodex.account_routing_order \
			  WHERE account_id=$1::text::uuid)",
			&[&loser_account.as_str(), &loser_operation.as_str()],
		)
		.await?;
	assert!(!(0..3).any(|index| loser_projection.get::<_, bool>(index)));
	Ok(())
}

fn duplicate_enrollment_preparation(
	account_id: &str,
	operation_id: &str,
	provider_account_id: &str,
	label: &str,
) -> Result<AccountOperationPreparation, Box<dyn Error>> {
	let account_id = AccountId::new(account_id)?;
	let operation_id = AccountOperationId::new(operation_id)?;
	let provider = ProviderIdentity::new(AccountProvider::Chatgpt, provider_account_id)?;
	Ok(AccountOperationPreparation {
		operation_id: operation_id.clone(),
		account_id,
		kind: AccountOperationKind::Enroll,
		display_label: Some(label.to_owned()),
		enabled: Some(true),
		expected_account_revision: None,
		expected: None,
		target: Some(CredentialBinding {
			schema_version: CredentialStoreSchemaVersion::V1,
			version: CredentialVersion::new(1)?,
			fingerprint: CredentialFingerprint::new(CREDENTIAL_FINGERPRINT)?,
			provider: provider.clone(),
			writer_operation_id: operation_id,
		}),
		provider,
	})
}

async fn account_lifecycle_side_effect_counts(
	owner: &tokio_postgres::Client,
) -> Result<[i64; 8], tokio_postgres::Error> {
	let row = owner
		.query_one(
			"SELECT \
			 (SELECT count(*) FROM decodex.accounts),\
			 (SELECT count(*) FROM decodex.account_operations),\
			 (SELECT count(*) FROM decodex.account_routing_order),\
			 (SELECT count(*) FROM decodex.accounts \
			  WHERE credential_store_schema_version IS NOT NULL),\
			 (SELECT count(*) FROM decodex.account_quota_facts),\
			 (SELECT count(*) FROM decodex.activity),\
			 (SELECT count(*) FROM decodex.outbox),\
			 (SELECT revision FROM decodex.account_routing_control WHERE singleton)",
			&[],
		)
		.await?;
	Ok(std::array::from_fn(|index| row.get(index)))
}

fn provider_mismatch_receipt() -> Value {
	json!({
		"outcome": "rejected",
		"data": {
			"schema": "decodex/account-command-result/1",
			"error": {
				"reason": "account_command_rejected",
				"rejection": "provider_mismatch",
			},
		},
	})
}

struct InitialResetCardOperation {
	command: CommandIdentity,
	descriptor: ResetCardDescriptor,
	preparation: ResetCardPreparation,
	claim: ResetCardClaim,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires a fresh isolated PostgreSQL 18 reset-card database"]
async fn reset_card_private_claim_and_reclaim_contract() -> Result<(), Box<dyn Error>> {
	let (schema_owner, runtime) = owner_runtime_configs("DECODEX_TEST")?;
	let store =
		PostgresStore::connect_runtime_fixture(runtime.clone(), expected_peer_uid()).await?;
	let (owner, owner_connection) = schema_owner.connect(NoTls).await?;
	let owner_connection_task = tokio::spawn(owner_connection);
	let account_id = AccountId::new(ACCOUNT_ID)?;
	enroll_v27_account(
		&store,
		&account_id,
		"Reset-card integration",
		PROVIDER_ACCOUNT_ID,
		CREDENTIAL_WRITER,
	)
	.await?;

	assert_provider_binding_is_immutable(&store, &account_id).await?;
	let initial = prepare_initial_reset_card_operation(&store, &owner, &account_id).await?;
	assert_competing_and_exhausted_operations(
		&store,
		&owner,
		&account_id,
		initial.descriptor,
		&initial.claim,
	)
	.await?;
	change_account_health_and_assert_stale_replay(&store, &owner, &account_id, &initial).await?;
	reconcile_ambiguous_initial_operation(&store, &owner, &account_id, &initial.claim).await?;

	let reusable_descriptor = ResetCardDescriptor::new(
		ResetCardTimestamp::from_unix_seconds(2_100_000_000)?,
		ResetCardTimestamp::from_unix_seconds(2_100_003_600)?,
	)?;
	let nothing_preparation =
		complete_nothing_to_reset(&store, &owner, &account_id, reusable_descriptor).await?;
	start_reusable_operation_and_assert_replays(
		&store,
		&account_id,
		&initial,
		reusable_descriptor,
		&nothing_preparation,
	)
	.await?;
	reject_pending_replay_after_account_change(
		&store,
		&owner,
		&schema_owner,
		&account_id,
		reusable_descriptor,
	)
	.await?;

	store.close();
	drop(owner);
	owner_connection_task.await??;

	Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires a fresh isolated PostgreSQL 18 reset-card database"]
async fn accepted_reset_card_effect_survives_administrative_disable() -> Result<(), Box<dyn Error>>
{
	let (schema_owner, runtime) = owner_runtime_configs("DECODEX_TEST")?;
	let store =
		PostgresStore::connect_runtime_fixture(runtime.clone(), expected_peer_uid()).await?;
	let (owner, owner_connection) = schema_owner.connect(NoTls).await?;
	let owner_connection_task = tokio::spawn(owner_connection);
	let account_id = AccountId::new(DISABLE_ACCOUNT_ID)?;
	enroll_v27_account(
		&store,
		&account_id,
		"Disable after acceptance",
		DISABLE_PROVIDER_ACCOUNT_ID,
		"71000000-0000-4000-8000-000000000012",
	)
	.await?;
	let descriptor = ResetCardDescriptor::new(
		ResetCardTimestamp::from_unix_seconds(2_200_000_000)?,
		ResetCardTimestamp::from_unix_seconds(2_200_003_600)?,
	)?;
	store
		.prepare_reset_card_operation(
			&CommandIdentity::new(
				"reset-card-disable-operation",
				b"reset-card-disable-operation-v1",
			)?,
			&account_id,
			INITIAL_ACCOUNT_REVISION,
			&process_binding_with_writer(
				INITIAL_ACCOUNT_REVISION,
				DISABLE_PROVIDER_ACCOUNT_ID,
				"71000000-0000-4000-8000-000000000012",
			)?,
			descriptor,
		)
		.await?;
	let claim = store
		.claim_reset_card_operation(RESET_WORKER_A, Duration::from_secs(2))
		.await?
		.expect("the accepted reset-card operation must be claimable");
	store.bind_reset_card_credit(&claim, RESET_WORKER_A, "disable-provider-credit").await?;
	assert_eq!(
		store.set_account_enabled(&account_id, INITIAL_ACCOUNT_REVISION, false).await?,
		AccountAdministrationOutcome::Updated { revision: CHANGED_ACCOUNT_REVISION },
	);
	store.begin_reset_card_effect(&claim, RESET_WORKER_A).await?;
	let effect_state: String = owner
		.query_one("SELECT effect_state::text FROM decodex.outbox WHERE id=$1", &[&claim.id])
		.await?
		.get(0);
	assert_eq!(effect_state, "ambiguous");

	store.close();
	drop(owner);
	owner_connection_task.await??;
	Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires a fresh isolated PostgreSQL 18 reset-card database"]
async fn accepted_reset_card_effect_survives_credential_rotation() -> Result<(), Box<dyn Error>> {
	let (schema_owner, runtime) = owner_runtime_configs("DECODEX_TEST")?;
	let store =
		PostgresStore::connect_runtime_fixture(runtime.clone(), expected_peer_uid()).await?;
	let (owner, owner_connection) = schema_owner.connect(NoTls).await?;
	let owner_connection_task = tokio::spawn(owner_connection);
	let account_id = AccountId::new(ROTATION_ACCOUNT_ID)?;
	enroll_v27_account(
		&store,
		&account_id,
		"Rotate after acceptance",
		PROVIDER_ACCOUNT_ID,
		CREDENTIAL_WRITER,
	)
	.await?;
	let descriptor = ResetCardDescriptor::new(
		ResetCardTimestamp::from_unix_seconds(2_300_000_000)?,
		ResetCardTimestamp::from_unix_seconds(2_300_003_600)?,
	)?;
	store
		.prepare_reset_card_operation(
			&CommandIdentity::new(
				"reset-card-rotation-operation",
				b"reset-card-rotation-operation-v1",
			)?,
			&account_id,
			INITIAL_ACCOUNT_REVISION,
			&process_binding(INITIAL_ACCOUNT_REVISION)?,
			descriptor,
		)
		.await?;
	let claim = store
		.claim_reset_card_operation(RESET_WORKER_A, Duration::from_millis(200))
		.await?
		.expect("the accepted reset-card operation must be claimable");
	store.bind_reset_card_credit(&claim, RESET_WORKER_A, "rotation-provider-credit").await?;
	store.begin_reset_card_effect(&claim, RESET_WORKER_A).await?;
	owner
		.execute(
			"UPDATE decodex.accounts SET credential_version=2,credential_fingerprint=$2, \
				 credential_writer_operation_id=$3::text::uuid,revision=revision+1, \
				 updated_at=clock_timestamp() WHERE account_id=$1::text::uuid",
			&[&account_id.as_str(), &ROTATED_CREDENTIAL_FINGERPRINT, &ROTATED_CREDENTIAL_WRITER],
		)
		.await?;
	time::sleep(Duration::from_millis(250)).await;

	let recovered = store
		.claim_reset_card_operation(RESET_WORKER_B, Duration::from_secs(2))
		.await?
		.expect("the ambiguous effect must be reclaimed after credential rotation");
	assert_eq!(recovered.id, claim.id);
	assert!(recovered.requires_reconciliation);
	assert_eq!(recovered.account_revision, INITIAL_ACCOUNT_REVISION);
	assert_eq!(recovered.process_binding.credential.version.get(), 1);
	assert_eq!(recovered.process_binding.credential.fingerprint.as_str(), CREDENTIAL_FINGERPRINT,);
	assert_eq!(
		recovered.process_binding.credential.writer_operation_id.as_str(),
		CREDENTIAL_WRITER,
	);
	let current_binding = owner
		.query_one(
			"SELECT revision,credential_version,credential_fingerprint, \
			 credential_writer_operation_id::text FROM decodex.accounts \
				 WHERE account_id=$1::text::uuid",
			&[&account_id.as_str()],
		)
		.await?;
	assert_eq!(
		(
			current_binding.get::<_, i64>(0),
			current_binding.get::<_, i64>(1),
			current_binding.get::<_, String>(2),
			current_binding.get::<_, String>(3),
		),
		(
			CHANGED_ACCOUNT_REVISION,
			2,
			ROTATED_CREDENTIAL_FINGERPRINT.into(),
			ROTATED_CREDENTIAL_WRITER.into(),
		)
	);
	store
		.record_outbox_receipt(
			recovered.id,
			RESET_WORKER_B,
			recovered.claim_token(),
			&json!({"outcome": "reset"}),
		)
		.await?;
	store
		.reconcile_outbox(
			recovered.id,
			RESET_WORKER_B,
			recovered.claim_token(),
			&OutboxReconciliation {
				readback: json!({
					"schema": "decodex/reset-card-readback/1",
					"outcome": "reset",
					"selected_card_still_available": false,
				}),
				outcome: ReconciliationOutcome::EffectPresent,
			},
			Duration::from_millis(1),
			Duration::from_secs(1),
		)
		.await?;
	assert_eq!(
		store.reset_card_operation_status("reset-card-rotation-operation").await?,
		ResetCardOperationStatus::Completed(ResetCardConsumeOutcome::Reset),
	);

	store.close();
	drop(owner);
	owner_connection_task.await??;
	Ok(())
}

async fn assert_provider_binding_is_immutable(
	store: &PostgresStore,
	account_id: &AccountId,
) -> Result<(), Box<dyn Error>> {
	let operation_id = AccountOperationId::new("71000000-0000-4000-8000-000000000014")?;
	let expected = process_binding(INITIAL_ACCOUNT_REVISION)?.credential;
	let drifted_provider = ProviderIdentity::new(AccountProvider::Chatgpt, "provider-drift")?;
	let drift_result = store
		.prepare_account_operation(&AccountOperationPreparation {
			operation_id: operation_id.clone(),
			account_id: account_id.clone(),
			kind: AccountOperationKind::Refresh,
			display_label: None,
			enabled: None,
			expected_account_revision: Some(INITIAL_ACCOUNT_REVISION),
			expected: Some(expected),
			target: Some(CredentialBinding {
				schema_version: CredentialStoreSchemaVersion::V1,
				version: CredentialVersion::new(2)?,
				fingerprint: CredentialFingerprint::new(
					"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
				)?,
				provider: drifted_provider.clone(),
				writer_operation_id: operation_id,
			}),
			provider: drifted_provider,
		})
		.await?;
	assert!(
		matches!(
			&drift_result,
			AccountLifecycleMutationOutcome::Rejected {
				rejection: AccountLifecycleRejection::StaleAccount,
				actual,
			} if actual.account_revision == INITIAL_ACCOUNT_REVISION
		),
		"unexpected immutable-binding mutation result: {drift_result:?}"
	);

	Ok(())
}

async fn prepare_initial_reset_card_operation(
	store: &PostgresStore,
	owner: &tokio_postgres::Client,
	account_id: &AccountId,
) -> Result<InitialResetCardOperation, Box<dyn Error>> {
	let descriptor = ResetCardDescriptor::new(
		ResetCardTimestamp::from_unix_seconds(2_000_000_000)?,
		ResetCardTimestamp::from_unix_seconds(2_000_003_600)?,
	)?;
	let operation_command = CommandIdentity::new(OPERATION_KEY, b"reset-card-operation-v1")?;
	let preparation = store
		.prepare_reset_card_operation(
			&operation_command,
			account_id,
			INITIAL_ACCOUNT_REVISION,
			&process_binding(INITIAL_ACCOUNT_REVISION)?,
			descriptor,
		)
		.await?;
	assert!(store.reset_card_account_has_unsettled_operations(account_id).await?);

	assert_eq!(&preparation.account_id, account_id);
	assert_eq!(preparation.account_revision, INITIAL_ACCOUNT_REVISION);
	assert_eq!(preparation.descriptor, descriptor);

	let prepared_row = owner
		.query_one(
			"SELECT id,aggregate_id,payload FROM decodex.outbox \
			 WHERE aggregate_kind='reset_card_operation'",
			&[],
		)
		.await?;
	let reset_outbox_id: i64 = prepared_row.get(0);
	let public_aggregate_id: String = prepared_row.get(1);
	let prepared_payload: Value = prepared_row.get(2);

	assert_public_payload_is_private_material_free(&prepared_payload);
	assert_ne!(public_aggregate_id, OPERATION_KEY);
	assert!(!public_aggregate_id.contains(OPERATION_KEY));
	let encoded_provider_key = prepared_payload
		.pointer("/reset_card_effect/provider_idempotency_key_hex")
		.and_then(Value::as_str)
		.expect("the private projection must retain the encoded provider retry key");
	assert_ne!(encoded_provider_key, OPERATION_KEY);
	assert_eq!(
		prepared_payload
			.pointer("/reset_card_effect/credential_writer_operation_id")
			.and_then(Value::as_str),
		Some(CREDENTIAL_WRITER),
	);
	assert!(!prepared_payload.to_string().contains(OPERATION_KEY));
	assert_activity_remains_public(store, owner, &public_aggregate_id).await?;

	let generic_claims = store.claim_outbox(GENERIC_WORKER, 100, Duration::from_secs(2)).await?;

	assert!(
		generic_claims.iter().all(|claim| claim.id != reset_outbox_id),
		"the generic claimant must not receive the pending private reset-card row"
	);

	let first = store
		.claim_reset_card_operation(RESET_WORKER_A, Duration::from_secs(2))
		.await?
		.expect("the typed reset-card worker must claim its pending row");

	assert_eq!(first.id, reset_outbox_id);
	assert_eq!(first.provider_idempotency_key(), OPERATION_KEY);
	assert_eq!(first.exact_credit_id(), None);
	assert!(!first.requires_reconciliation);
	assert!(!format!("{first:?}").contains(OPERATION_KEY));

	store.bind_reset_card_credit(&first, RESET_WORKER_A, EXACT_PROVIDER_CREDIT_ID).await?;

	let private_payload: Value = owner
		.query_one("SELECT payload FROM decodex.outbox WHERE id=$1", &[&first.id])
		.await?
		.get(0);
	let encoded_credit_id = private_payload
		.pointer("/reset_card_effect/provider_credit_id_hex")
		.and_then(Value::as_str)
		.expect("the exact provider identity must use the private encoded projection");

	assert_ne!(encoded_credit_id, EXACT_PROVIDER_CREDIT_ID);
	assert!(!private_payload.to_string().contains(EXACT_PROVIDER_CREDIT_ID));
	assert!(!private_payload.to_string().contains(OPERATION_KEY));
	assert_public_payload_is_private_material_free(&private_payload);
	assert_activity_remains_public(store, owner, &public_aggregate_id).await?;

	store
		.renew_reset_card_claim(
			first.id,
			RESET_WORKER_A,
			first.claim_token(),
			Duration::from_secs(10),
		)
		.await?;
	assert!(matches!(
		store.begin_outbox_effect(first.id, RESET_WORKER_A, first.claim_token()).await,
		Err(StoreError::OwnershipLost("outbox claim"))
	));
	store.begin_reset_card_effect(&first, RESET_WORKER_A).await?;

	Ok(InitialResetCardOperation {
		command: operation_command,
		descriptor,
		preparation,
		claim: first,
	})
}

async fn assert_competing_and_exhausted_operations(
	store: &PostgresStore,
	owner: &tokio_postgres::Client,
	account_id: &AccountId,
	descriptor: ResetCardDescriptor,
	first: &ResetCardClaim,
) -> Result<(), Box<dyn Error>> {
	let competing_command =
		CommandIdentity::new(COMPETING_OPERATION_KEY, b"reset-card-operation-v1")?;
	let competing_preparation = store
		.prepare_reset_card_operation(
			&competing_command,
			account_id,
			INITIAL_ACCOUNT_REVISION,
			&process_binding(INITIAL_ACCOUNT_REVISION)?,
			descriptor,
		)
		.await?;

	assert_eq!(competing_preparation.account_revision, INITIAL_ACCOUNT_REVISION);
	assert_eq!(competing_preparation.descriptor, descriptor);

	let competing = store
		.claim_reset_card_operation(RESET_WORKER_B, Duration::from_secs(2))
		.await?
		.expect("the later same-selection operation must remain independently claimable");

	assert_ne!(competing.id, first.id);
	assert_eq!(competing.provider_idempotency_key(), COMPETING_OPERATION_KEY);
	assert!(!competing.requires_reconciliation);
	store.bind_reset_card_credit(&competing, RESET_WORKER_B, EXACT_PROVIDER_CREDIT_ID).await?;
	assert!(matches!(
		store.begin_reset_card_effect(&competing, RESET_WORKER_B).await,
		Err(StoreError::ResetCardSelectionConflict)
	));
	store
		.fail_reset_card_before_effect(
			&competing,
			RESET_WORKER_B,
			ResetCardFailureCode::InventoryChanged,
		)
		.await?;
	assert_eq!(
		store.reset_card_operation_status(COMPETING_OPERATION_KEY).await?,
		ResetCardOperationStatus::FailedBeforeEffect(ResetCardFailureCode::InventoryChanged),
	);
	assert_private_effect_scrubbed(owner, competing.id).await?;

	let exhausted_command =
		CommandIdentity::new(EXHAUSTED_OPERATION_KEY, b"reset-card-exhausted-operation-v1")?;
	store
		.prepare_reset_card_operation(
			&exhausted_command,
			account_id,
			INITIAL_ACCOUNT_REVISION,
			&process_binding(INITIAL_ACCOUNT_REVISION)?,
			descriptor,
		)
		.await?;
	let exhausted = store
		.claim_reset_card_operation(RESET_WORKER_C, Duration::from_secs(2))
		.await?
		.expect("the operation that will exhaust its claim budget must be claimable");
	store.bind_reset_card_credit(&exhausted, RESET_WORKER_C, EXACT_PROVIDER_CREDIT_ID).await?;
	owner
		.execute(
			"UPDATE decodex.outbox SET attempt_count=max_attempts, \
			 lease_acquired_at=created_at, \
			 lease_expires_at=created_at+interval '1 millisecond' WHERE id=$1",
			&[&exhausted.id],
		)
		.await?;
	time::sleep(Duration::from_millis(5)).await;
	assert!(
		store.claim_reset_card_operation(RESET_WORKER_D, Duration::from_secs(2)).await?.is_none(),
		"the exhausted pre-effect operation must become terminal instead of being reclaimed"
	);
	assert_eq!(
		store.reset_card_operation_status(EXHAUSTED_OPERATION_KEY).await?,
		ResetCardOperationStatus::FailedBeforeEffect(ResetCardFailureCode::ProviderUnavailable),
	);
	assert_private_effect_scrubbed(owner, exhausted.id).await?;

	Ok(())
}

async fn change_account_health_and_assert_stale_replay(
	store: &PostgresStore,
	owner: &tokio_postgres::Client,
	account_id: &AccountId,
	initial: &InitialResetCardOperation,
) -> Result<(), Box<dyn Error>> {
	assert_eq!(
		store.set_account_enabled(account_id, INITIAL_ACCOUNT_REVISION, false).await?,
		AccountAdministrationOutcome::Updated { revision: CHANGED_ACCOUNT_REVISION },
	);
	assert_eq!(
		store
			.prepare_reset_card_operation(
				&initial.command,
				account_id,
				INITIAL_ACCOUNT_REVISION,
				&process_binding(INITIAL_ACCOUNT_REVISION)?,
				initial.descriptor,
			)
			.await?,
		initial.preparation,
		"an exact completed key must replay before current account admission",
	);
	let stale_command =
		CommandIdentity::new("reset-card-integration-stale-operation", b"stale-operation-v1")?;
	let stale_result = store
		.prepare_reset_card_operation(
			&stale_command,
			account_id,
			INITIAL_ACCOUNT_REVISION,
			&process_binding(INITIAL_ACCOUNT_REVISION)?,
			initial.descriptor,
		)
		.await;
	assert!(
		matches!(
			&stale_result,
			Err(StoreError::RevisionConflict {
				expected: Some(INITIAL_ACCOUNT_REVISION),
				actual: Some(CHANGED_ACCOUNT_REVISION),
				..
			})
		),
		"unexpected proved-rejection result: {stale_result:?}"
	);
	let stale_receipt_state: String = owner
		.query_one(
			"SELECT receipt_state::text FROM decodex.command_receipts \
			 WHERE idempotency_key=$1",
			&[&"reset-card-integration-stale-operation"],
		)
		.await?
		.get(0);
	assert_eq!(
		stale_receipt_state, "completed",
		"a proved pre-effect rejection must close its pending command reservation"
	);
	assert!(matches!(
		store
			.prepare_reset_card_operation(
				&stale_command,
				account_id,
				INITIAL_ACCOUNT_REVISION,
				&process_binding(INITIAL_ACCOUNT_REVISION)?,
				initial.descriptor,
			)
			.await,
		Err(StoreError::RevisionConflict {
			expected: Some(INITIAL_ACCOUNT_REVISION),
			actual: Some(CHANGED_ACCOUNT_REVISION),
			..
		})
	));

	Ok(())
}

async fn reconcile_ambiguous_initial_operation(
	store: &PostgresStore,
	owner: &tokio_postgres::Client,
	account_id: &AccountId,
	first: &ResetCardClaim,
) -> Result<(), Box<dyn Error>> {
	store
		.renew_reset_card_claim(
			first.id,
			RESET_WORKER_A,
			first.claim_token(),
			Duration::from_millis(200),
		)
		.await?;
	time::sleep(Duration::from_millis(250)).await;

	let recovered = store
		.claim_reset_card_operation(RESET_WORKER_C, Duration::from_secs(2))
		.await?
		.expect("an expired ambiguous effect must be reclaimed after account revision changes");

	assert_eq!(recovered.id, first.id);
	assert_ne!(recovered.claim_token(), first.claim_token());
	assert_eq!(recovered.account_revision, INITIAL_ACCOUNT_REVISION);
	assert_eq!(recovered.provider_idempotency_key(), OPERATION_KEY);
	assert_eq!(recovered.exact_credit_id(), Some(EXACT_PROVIDER_CREDIT_ID));
	assert!(recovered.requires_reconciliation);
	assert_eq!(recovered.recorded_outcome, None);
	assert!(!format!("{recovered:?}").contains(EXACT_PROVIDER_CREDIT_ID));
	assert!(matches!(
		store
			.record_outbox_receipt(
				first.id,
				RESET_WORKER_A,
				first.claim_token(),
				&json!({"outcome": "reset"}),
			)
			.await,
		Err(StoreError::OwnershipLost("outbox claim"))
	));

	store
		.renew_reset_card_claim(
			recovered.id,
			RESET_WORKER_C,
			recovered.claim_token(),
			Duration::from_millis(200),
		)
		.await?;
	store
		.record_outbox_receipt(
			recovered.id,
			RESET_WORKER_C,
			recovered.claim_token(),
			&json!({"outcome": "reset"}),
		)
		.await?;

	time::sleep(Duration::from_millis(250)).await;

	let final_claim = store
		.claim_reset_card_operation(RESET_WORKER_D, Duration::from_secs(2))
		.await?
		.expect("a recorded receipt must survive lease expiry and reclaim");

	assert_eq!(final_claim.id, first.id);
	assert_eq!(final_claim.account_revision, INITIAL_ACCOUNT_REVISION);
	assert_eq!(final_claim.provider_idempotency_key(), OPERATION_KEY);
	assert_eq!(final_claim.exact_credit_id(), Some(EXACT_PROVIDER_CREDIT_ID));
	assert_eq!(final_claim.recorded_outcome, Some(ResetCardConsumeOutcome::Reset));
	assert!(final_claim.requires_reconciliation);

	store
		.reconcile_outbox(
			final_claim.id,
			RESET_WORKER_D,
			final_claim.claim_token(),
			&OutboxReconciliation {
				readback: json!({
					"schema": "decodex/reset-card-readback/1",
					"outcome": "reset",
					"selected_card_still_available": false,
				}),
				outcome: ReconciliationOutcome::EffectPresent,
			},
			Duration::from_millis(1),
			Duration::from_secs(1),
		)
		.await?;

	assert_private_effect_scrubbed(owner, final_claim.id).await?;
	assert_eq!(
		store.reset_card_operation_status(OPERATION_KEY).await?,
		ResetCardOperationStatus::Completed(ResetCardConsumeOutcome::Reset)
	);
	assert!(!store.reset_card_account_has_unsettled_operations(account_id).await?);

	Ok(())
}

async fn complete_nothing_to_reset(
	store: &PostgresStore,
	owner: &tokio_postgres::Client,
	account_id: &AccountId,
	reusable_descriptor: ResetCardDescriptor,
) -> Result<ResetCardPreparation, Box<dyn Error>> {
	assert_eq!(
		store.set_account_enabled(account_id, CHANGED_ACCOUNT_REVISION, true).await?,
		AccountAdministrationOutcome::Updated { revision: REENABLED_ACCOUNT_REVISION },
	);

	let nothing_command =
		CommandIdentity::new(NOTHING_TO_RESET_KEY, b"reset-card-nothing-to-reset-v1")?;
	let nothing_preparation = store
		.prepare_reset_card_operation(
			&nothing_command,
			account_id,
			REENABLED_ACCOUNT_REVISION,
			&process_binding(REENABLED_ACCOUNT_REVISION)?,
			reusable_descriptor,
		)
		.await?;
	let nothing_claim = store
		.claim_reset_card_operation(RESET_WORKER_A, Duration::from_secs(2))
		.await?
		.expect("the NothingToReset operation must be claimable");
	store
		.bind_reset_card_credit(&nothing_claim, RESET_WORKER_A, "reusable-provider-credit")
		.await?;
	store.begin_reset_card_effect(&nothing_claim, RESET_WORKER_A).await?;
	store
		.record_outbox_receipt(
			nothing_claim.id,
			RESET_WORKER_A,
			nothing_claim.claim_token(),
			&json!({"outcome": "nothing_to_reset"}),
		)
		.await?;
	store
		.reconcile_outbox(
			nothing_claim.id,
			RESET_WORKER_A,
			nothing_claim.claim_token(),
			&OutboxReconciliation {
				readback: json!({
					"schema": "decodex/reset-card-readback/1",
					"account_id": account_id.as_str(),
					"account_revision": REENABLED_ACCOUNT_REVISION,
					"outcome": "nothing_to_reset",
					"available_count": 1,
					"selected_exact_credit_available": true,
					"selected_descriptor_expired": false,
				}),
				outcome: ReconciliationOutcome::EffectPresent,
			},
			Duration::from_millis(1),
			Duration::from_secs(1),
		)
		.await?;
	assert_private_effect_scrubbed(owner, nothing_claim.id).await?;
	assert_eq!(
		store.reset_card_operation_status(NOTHING_TO_RESET_KEY).await?,
		ResetCardOperationStatus::Completed(ResetCardConsumeOutcome::NothingToReset),
	);

	Ok(nothing_preparation)
}

async fn start_reusable_operation_and_assert_replays(
	store: &PostgresStore,
	account_id: &AccountId,
	initial: &InitialResetCardOperation,
	reusable_descriptor: ResetCardDescriptor,
	nothing_preparation: &ResetCardPreparation,
) -> Result<(), Box<dyn Error>> {
	let reusable_command =
		CommandIdentity::new(REUSABLE_OPERATION_KEY, b"reset-card-reusable-operation-v1")?;
	store
		.prepare_reset_card_operation(
			&reusable_command,
			account_id,
			REENABLED_ACCOUNT_REVISION,
			&process_binding(REENABLED_ACCOUNT_REVISION)?,
			reusable_descriptor,
		)
		.await?;
	let reusable_claim = store
		.claim_reset_card_operation(RESET_WORKER_B, Duration::from_secs(2))
		.await?
		.expect("a card retained by NothingToReset must be reusable under a new key");
	store
		.bind_reset_card_credit(&reusable_claim, RESET_WORKER_B, "reusable-provider-credit")
		.await?;
	store.begin_reset_card_effect(&reusable_claim, RESET_WORKER_B).await?;

	time::sleep(Duration::from_millis(1_100)).await;
	assert_eq!(store.prune_delivered_outbox(1_000).await?, 0);
	assert_eq!(
		store.reset_card_operation_status(OPERATION_KEY).await?,
		ResetCardOperationStatus::Completed(ResetCardConsumeOutcome::Reset),
	);
	assert_eq!(
		store.reset_card_operation_status(NOTHING_TO_RESET_KEY).await?,
		ResetCardOperationStatus::Completed(ResetCardConsumeOutcome::NothingToReset),
	);
	assert_eq!(
		store
			.prepare_reset_card_operation(
				&initial.command,
				account_id,
				INITIAL_ACCOUNT_REVISION,
				&process_binding(INITIAL_ACCOUNT_REVISION)?,
				initial.descriptor,
			)
			.await?,
		initial.preparation,
	);
	let nothing_command =
		CommandIdentity::new(NOTHING_TO_RESET_KEY, b"reset-card-nothing-to-reset-v1")?;
	assert_eq!(
		store
			.prepare_reset_card_operation(
				&nothing_command,
				account_id,
				REENABLED_ACCOUNT_REVISION,
				&process_binding(REENABLED_ACCOUNT_REVISION)?,
				reusable_descriptor,
			)
			.await?,
		*nothing_preparation,
	);

	Ok(())
}

#[allow(clippy::too_many_lines)] // One complete durable rejection and replay matrix.
async fn reject_pending_replay_after_account_change(
	store: &PostgresStore,
	owner: &tokio_postgres::Client,
	schema_owner: &tokio_postgres::Config,
	account_id: &AccountId,
	reusable_descriptor: ResetCardDescriptor,
) -> Result<(), Box<dyn Error>> {
	assert_eq!(
		store.set_account_enabled(account_id, REENABLED_ACCOUNT_REVISION, false).await?,
		AccountAdministrationOutcome::Updated { revision: FINAL_ACCOUNT_REVISION },
	);

	let effects_before = reset_card_effect_counts(owner).await?;
	let pending_command = insert_pending_reset_card_receipt(
		owner,
		PENDING_OPERATION_KEY,
		b"reset-card-pending-operation-v1",
		account_id,
		REENABLED_ACCOUNT_REVISION,
		reusable_descriptor,
	)
	.await?;
	assert!(matches!(
		store
			.replay_reset_card_preparation(
				&pending_command,
				account_id,
				REENABLED_ACCOUNT_REVISION,
				reusable_descriptor,
				Some(CALLBACK_PROFILE),
			)
			.await,
		Err(StoreError::ResetCardCommitOutcomeUnknown)
	));
	let state_rejected_command = insert_pending_reset_card_receipt(
		owner,
		"reset-card-expired-state-rejected",
		b"reset-card-expired-state-rejected-v1",
		account_id,
		FINAL_ACCOUNT_REVISION,
		reusable_descriptor,
	)
	.await?;
	let admitted_command = insert_pending_reset_card_receipt(
		owner,
		"reset-card-expired-admitted",
		b"reset-card-expired-admitted-v1",
		account_id,
		FINAL_ACCOUNT_REVISION,
		reusable_descriptor,
	)
	.await?;
	let absent_account_id = AccountId::new("71000000-0000-4000-8000-000000000009")?;
	let absent_command = insert_pending_reset_card_receipt(
		owner,
		"reset-card-expired-absent",
		b"reset-card-expired-absent-v1",
		&absent_account_id,
		1,
		reusable_descriptor,
	)
	.await?;
	let lower_command = insert_pending_reset_card_receipt(
		owner,
		"reset-card-expired-lower",
		b"reset-card-expired-lower-v1",
		account_id,
		FINAL_ACCOUNT_REVISION + 1,
		reusable_descriptor,
	)
	.await?;
	let missing_callback_command = insert_pending_reset_card_receipt(
		owner,
		"reset-card-expired-missing-callback",
		b"reset-card-expired-missing-callback-v1",
		account_id,
		FINAL_ACCOUNT_REVISION,
		reusable_descriptor,
	)
	.await?;
	let store_unavailable_command = insert_pending_reset_card_receipt(
		owner,
		"reset-card-expired-store-unavailable",
		b"reset-card-expired-store-unavailable-v1",
		account_id,
		FINAL_ACCOUNT_REVISION,
		reusable_descriptor,
	)
	.await?;
	let completed_race_command = insert_pending_reset_card_receipt(
		owner,
		"reset-card-expired-completed-race",
		b"reset-card-expired-completed-race-v1",
		account_id,
		FINAL_ACCOUNT_REVISION,
		reusable_descriptor,
	)
	.await?;
	let drift_after_reclaim_command = insert_pending_reset_card_receipt(
		owner,
		"reset-card-expired-drift-after-reclaim",
		b"reset-card-expired-drift-after-reclaim-v1",
		account_id,
		FINAL_ACCOUNT_REVISION,
		reusable_descriptor,
	)
	.await?;
	time::sleep(Duration::from_millis(550)).await;

	assert!(matches!(
		store
			.replay_reset_card_preparation(
				&pending_command,
				account_id,
				REENABLED_ACCOUNT_REVISION,
				reusable_descriptor,
				Some(CALLBACK_PROFILE),
			)
			.await,
		Err(StoreError::RevisionConflict {
			expected: Some(REENABLED_ACCOUNT_REVISION),
			actual: Some(FINAL_ACCOUNT_REVISION),
			..
		})
	));
	let higher_revision_receipt =
		completed_reset_card_receipt(owner, PENDING_OPERATION_KEY).await?;
	assert!(matches!(
		store
			.replay_reset_card_preparation(
				&pending_command,
				account_id,
				REENABLED_ACCOUNT_REVISION,
				reusable_descriptor,
				Some(CALLBACK_PROFILE),
			)
			.await,
		Err(StoreError::RevisionConflict {
			expected: Some(REENABLED_ACCOUNT_REVISION),
			actual: Some(FINAL_ACCOUNT_REVISION),
			..
		})
	));
	assert_eq!(
		completed_reset_card_receipt(owner, PENDING_OPERATION_KEY).await?,
		higher_revision_receipt,
		"completed revision rejection bytes must replay unchanged",
	);

	owner
		.execute(
			"UPDATE decodex.accounts SET state='unknown' \
				 WHERE account_id=$1::text::uuid AND revision=$2",
			&[&account_id.as_str(), &FINAL_ACCOUNT_REVISION],
		)
		.await?;
	assert!(matches!(
		store
			.replay_reset_card_preparation(
				&state_rejected_command,
				account_id,
				FINAL_ACCOUNT_REVISION,
				reusable_descriptor,
				Some(CALLBACK_PROFILE),
			)
			.await,
		Err(StoreError::InvalidInput("account state rejects manual reset-card use"))
	));
	let state_rejected_receipt =
		completed_reset_card_receipt(owner, "reset-card-expired-state-rejected").await?;
	owner
		.execute(
			"UPDATE decodex.accounts SET state='available',enabled=true \
				 WHERE account_id=$1::text::uuid AND revision=$2",
			&[&account_id.as_str(), &FINAL_ACCOUNT_REVISION],
		)
		.await?;
	assert!(matches!(
		store
			.replay_reset_card_preparation(
				&state_rejected_command,
				account_id,
				FINAL_ACCOUNT_REVISION,
				reusable_descriptor,
				Some(CALLBACK_PROFILE),
			)
			.await,
		Err(StoreError::InvalidInput("account state rejects manual reset-card use"))
	));
	assert_eq!(
		completed_reset_card_receipt(owner, "reset-card-expired-state-rejected").await?,
		state_rejected_receipt,
		"lifecycle recovery must not replace completed rejection bytes",
	);

	for (key, command, candidate_account, expected_revision) in [
		("reset-card-expired-admitted", &admitted_command, account_id, FINAL_ACCOUNT_REVISION),
		("reset-card-expired-absent", &absent_command, &absent_account_id, 1),
		("reset-card-expired-lower", &lower_command, account_id, FINAL_ACCOUNT_REVISION + 1),
	] {
		let receipt_before = pending_reset_card_receipt_fence(owner, key).await?;
		assert_eq!(
			store
				.replay_reset_card_preparation(
					command,
					candidate_account,
					expected_revision,
					reusable_descriptor,
					Some(CALLBACK_PROFILE),
				)
				.await?,
			None,
		);
		assert_eq!(
			pending_reset_card_receipt_fence(owner, key).await?,
			receipt_before,
			"a clear live-continuation observation must not rotate the pending receipt",
		);
	}

	let missing_callback_before =
		pending_reset_card_receipt_fence(owner, "reset-card-expired-missing-callback").await?;
	assert!(matches!(
		store
			.replay_reset_card_preparation(
				&missing_callback_command,
				account_id,
				FINAL_ACCOUNT_REVISION,
				reusable_descriptor,
				None,
			)
			.await,
		Err(StoreError::ResetCardCommitOutcomeUnknown)
	));
	assert_eq!(
		pending_reset_card_receipt_fence(owner, "reset-card-expired-missing-callback").await?,
		missing_callback_before,
	);

	owner
		.execute(
			"UPDATE decodex.accounts SET credential_store_observation='unavailable' \
				 WHERE account_id=$1::text::uuid AND revision=$2",
			&[&account_id.as_str(), &FINAL_ACCOUNT_REVISION],
		)
		.await?;
	let store_unavailable_before =
		pending_reset_card_receipt_fence(owner, "reset-card-expired-store-unavailable").await?;
	assert!(matches!(
		store
			.replay_reset_card_preparation(
				&store_unavailable_command,
				account_id,
				FINAL_ACCOUNT_REVISION,
				reusable_descriptor,
				Some(CALLBACK_PROFILE),
			)
			.await,
		Err(StoreError::ResetCardCommitOutcomeUnknown)
	));
	assert_eq!(
		pending_reset_card_receipt_fence(owner, "reset-card-expired-store-unavailable").await?,
		store_unavailable_before,
	);
	owner
		.execute(
			"UPDATE decodex.accounts SET credential_store_observation='exact' \
				 WHERE account_id=$1::text::uuid AND revision=$2",
			&[&account_id.as_str(), &FINAL_ACCOUNT_REVISION],
		)
		.await?;

	owner
		.execute(
			"UPDATE decodex.accounts SET state='unknown' \
				 WHERE account_id=$1::text::uuid AND revision=$2",
			&[&account_id.as_str(), &FINAL_ACCOUNT_REVISION],
		)
		.await?;
	let (completed_race_left, completed_race_right) = tokio::join!(
		store.replay_reset_card_preparation(
			&completed_race_command,
			account_id,
			FINAL_ACCOUNT_REVISION,
			reusable_descriptor,
			Some(CALLBACK_PROFILE),
		),
		store.replay_reset_card_preparation(
			&completed_race_command,
			account_id,
			FINAL_ACCOUNT_REVISION,
			reusable_descriptor,
			Some(CALLBACK_PROFILE),
		),
	);
	assert!(matches!(
		completed_race_left,
		Err(StoreError::InvalidInput("account state rejects manual reset-card use"))
	));
	assert!(matches!(
		completed_race_right,
		Err(StoreError::InvalidInput("account state rejects manual reset-card use"))
	));
	assert_eq!(
		pending_reset_card_receipt_fence(owner, "reset-card-expired-completed-race")
			.await?
			.receipt_state,
		"completed",
	);

	let drift_before =
		pending_reset_card_receipt_fence(owner, "reset-card-expired-drift-after-reclaim").await?;
	let (mut receipt_blocker, receipt_blocker_connection) = schema_owner.connect(NoTls).await?;
	let receipt_blocker_connection_task = tokio::spawn(receipt_blocker_connection);
	let receipt_blocker_transaction = receipt_blocker.transaction().await?;
	receipt_blocker_transaction
		.query_one(
			"SELECT idempotency_key FROM decodex.command_receipts \
			 WHERE idempotency_key=$1 FOR UPDATE",
			&[&"reset-card-expired-drift-after-reclaim"],
		)
		.await?;
	let replay_store = store.clone();
	let replay_account_id = account_id.clone();
	let replay_task = tokio::spawn(async move {
		replay_store
			.replay_reset_card_preparation(
				&drift_after_reclaim_command,
				&replay_account_id,
				FINAL_ACCOUNT_REVISION,
				reusable_descriptor,
				Some(CALLBACK_PROFILE),
			)
			.await
	});
	time::sleep(Duration::from_millis(50)).await;

	let (mut lifecycle_owner, lifecycle_owner_connection) = schema_owner.connect(NoTls).await?;
	let lifecycle_owner_connection_task = tokio::spawn(lifecycle_owner_connection);
	let lifecycle_transaction = lifecycle_owner.transaction().await?;
	let lifecycle_lock = time::timeout(
		Duration::from_millis(250),
		lifecycle_transaction.query_one(
			"SELECT pg_catalog.pg_advisory_xact_lock(1422,pg_catalog.hashtext($1))",
			&[&account_id.as_str()],
		),
	)
	.await;
	assert!(
		matches!(lifecycle_lock, Ok(Ok(_))),
		"receipt reservation must not wait while holding the account lifecycle lock",
	);
	receipt_blocker_transaction.commit().await?;

	let mut reclaimed_fence = None;
	for _ in 0..50 {
		let observed =
			pending_reset_card_receipt_fence(owner, "reset-card-expired-drift-after-reclaim")
				.await?;
		if observed.claim_token != drift_before.claim_token {
			reclaimed_fence = Some(observed);
			break;
		}
		time::sleep(Duration::from_millis(10)).await;
	}
	let reclaimed_fence =
		reclaimed_fence.expect("the expired receipt must be reclaimed before account revalidation");
	assert_eq!(reclaimed_fence.receipt_state, "pending");
	lifecycle_transaction
		.execute(
			"UPDATE decodex.accounts SET state='available' \
				 WHERE account_id=$1::text::uuid AND revision=$2",
			&[&account_id.as_str(), &FINAL_ACCOUNT_REVISION],
		)
		.await?;
	lifecycle_transaction.commit().await?;
	assert!(matches!(replay_task.await?, Err(StoreError::ResetCardCommitOutcomeUnknown)));
	let drift_after =
		pending_reset_card_receipt_fence(owner, "reset-card-expired-drift-after-reclaim").await?;
	assert_eq!(drift_after.receipt_state, "pending");
	assert_eq!(drift_after.claim_token, reclaimed_fence.claim_token);

	drop(receipt_blocker);
	drop(lifecycle_owner);
	receipt_blocker_connection_task.await??;
	lifecycle_owner_connection_task.await??;
	assert_eq!(
		reset_card_effect_counts(owner).await?,
		effects_before,
		"rejected and unknown replay recovery must not append activity or outbox effects",
	);

	Ok(())
}

#[derive(Debug, Eq, PartialEq)]
struct PendingResetCardReceiptFence {
	receipt_state: String,
	claim_token: Option<String>,
	claim_expires_at: Option<String>,
}

#[derive(Debug, Eq, PartialEq)]
struct CompletedResetCardReceipt {
	response: Value,
	response_bytes: Vec<u8>,
}

async fn insert_pending_reset_card_receipt(
	owner: &tokio_postgres::Client,
	key: &str,
	request: &[u8],
	account_id: &AccountId,
	expected_revision: i64,
	descriptor: ResetCardDescriptor,
) -> Result<CommandIdentity, Box<dyn Error>> {
	let request = request.to_vec();
	let descriptor_source = format!(
		"{}:{}",
		descriptor.granted_at().unix_seconds(),
		descriptor.expires_at().unix_seconds(),
	);
	owner
		.execute(
			"INSERT INTO decodex.command_receipts \
			 (idempotency_key,request_hash,protocol_version,operation,project_scope,scope_id, \
			  entity_id,expected_revision,payload_hash,payload_length,receipt_state, \
			  claim_token,claim_expires_at) \
			 VALUES ($1,encode(digest($2::bytea,'sha256'),'hex'), \
			  'decodex/store-command/1','consume_reset_card','global','reset_cards',$3,$4, \
			  encode(digest($5::text,'sha256'),'hex'),NULL,'pending',gen_random_uuid(), \
			  clock_timestamp()+interval '500 milliseconds')",
			&[&key, &request, &account_id.as_str(), &expected_revision, &descriptor_source],
		)
		.await?;

	Ok(CommandIdentity::new(key, &request)?)
}

async fn pending_reset_card_receipt_fence(
	owner: &tokio_postgres::Client,
	key: &str,
) -> Result<PendingResetCardReceiptFence, tokio_postgres::Error> {
	let row = owner
		.query_one(
			"SELECT receipt_state::text,claim_token::text,claim_expires_at::text \
			 FROM decodex.command_receipts WHERE idempotency_key=$1",
			&[&key],
		)
		.await?;

	Ok(PendingResetCardReceiptFence {
		receipt_state: row.get(0),
		claim_token: row.get(1),
		claim_expires_at: row.get(2),
	})
}

async fn completed_reset_card_receipt(
	owner: &tokio_postgres::Client,
	key: &str,
) -> Result<CompletedResetCardReceipt, tokio_postgres::Error> {
	let row = owner
		.query_one(
			"SELECT response,response_bytes FROM decodex.command_receipts \
			 WHERE idempotency_key=$1 AND receipt_state='completed'",
			&[&key],
		)
		.await?;

	Ok(CompletedResetCardReceipt { response: row.get(0), response_bytes: row.get(1) })
}

async fn reset_card_effect_counts(
	owner: &tokio_postgres::Client,
) -> Result<[i64; 2], tokio_postgres::Error> {
	let row = owner
		.query_one(
			"SELECT (SELECT count(*) FROM decodex.activity), \
			 (SELECT count(*) FROM decodex.outbox)",
			&[],
		)
		.await?;

	Ok([row.get(0), row.get(1)])
}

fn process_binding(revision: i64) -> Result<ProcessGenerationAccountBinding, Box<dyn Error>> {
	process_binding_with_writer(revision, PROVIDER_ACCOUNT_ID, CREDENTIAL_WRITER)
}

fn process_binding_with_writer(
	revision: i64,
	provider_account_id: &str,
	writer_operation_id: &str,
) -> Result<ProcessGenerationAccountBinding, Box<dyn Error>> {
	Ok(ProcessGenerationAccountBinding::new(
		revision,
		CredentialBinding {
			schema_version: CredentialStoreSchemaVersion::V1,
			version: CredentialVersion::new(1)?,
			fingerprint: CredentialFingerprint::new(CREDENTIAL_FINGERPRINT)?,
			provider: ProviderIdentity::new(AccountProvider::Chatgpt, provider_account_id)?,
			writer_operation_id: AccountOperationId::new(writer_operation_id)?,
		},
		CALLBACK_PROFILE,
	)?)
}

async fn enroll_v27_account(
	store: &PostgresStore,
	account_id: &AccountId,
	label: &str,
	provider_account_id: &str,
	writer_operation_id: &str,
) -> Result<(), Box<dyn Error>> {
	let operation_id = AccountOperationId::new(writer_operation_id)?;
	let provider = ProviderIdentity::new(AccountProvider::Chatgpt, provider_account_id)?;
	let prepared = store
		.prepare_account_operation(&AccountOperationPreparation {
			operation_id: operation_id.clone(),
			account_id: account_id.clone(),
			kind: AccountOperationKind::Enroll,
			display_label: Some(label.to_owned()),
			enabled: Some(true),
			expected_account_revision: None,
			expected: None,
			target: Some(CredentialBinding {
				schema_version: CredentialStoreSchemaVersion::V1,
				version: CredentialVersion::new(1)?,
				fingerprint: CredentialFingerprint::new(CREDENTIAL_FINGERPRINT)?,
				provider: provider.clone(),
				writer_operation_id: operation_id.clone(),
			}),
			provider,
		})
		.await?;
	assert!(matches!(
		prepared,
		AccountLifecycleMutationOutcome::Applied(ref mutation)
			if mutation.phase == AccountOperationPhase::Prepared
	));
	store
		.advance_account_operation(
			&operation_id,
			AccountOperationPhase::Prepared,
			AccountOperationPhase::StoreApplied,
			None,
		)
		.await?;
	let committed = store
		.advance_account_operation(
			&operation_id,
			AccountOperationPhase::StoreApplied,
			AccountOperationPhase::Committed,
			None,
		)
		.await?;
	assert!(matches!(
		committed,
		AccountLifecycleMutationOutcome::Applied(ref mutation)
			if mutation.phase == AccountOperationPhase::Committed
				&& mutation.account_revision == INITIAL_ACCOUNT_REVISION
	));
	store
		.observe_account_quota(
			account_id,
			AccountQuotaWindow::new(300, 50, 2_100_000_000_000_000)?,
			2_000_000_000_000_000,
		)
		.await?;
	assert!(
		store
			.attest_codex_account_capability(&CodexAccountCapabilityAttestation {
				build_identity: "codex-cli 0.146.0-alpha.9.2".to_owned(),
				executable_sha256:
					"d96ae1ca1ff6fc8587842fa04c92d3ee4d31651a811c2f89b65fcfd9c28473e2".to_owned(),
				schema_sha256: "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"
					.to_owned(),
				callback_profile_sha256: CALLBACK_PROFILE.to_owned(),
				login_chatgpt_auth_tokens: true,
				refresh_callback: true,
			})
			.await?
	);
	let account = &store.read_account_registry(Some(account_id), 1).await?[0];
	assert_eq!(account.lifecycle_readiness, AccountLifecycleReadiness::Ready);
	Ok(())
}

async fn assert_private_effect_scrubbed(
	owner: &tokio_postgres::Client,
	outbox_id: i64,
) -> Result<(), Box<dyn Error>> {
	let payload: Value = owner
		.query_one("SELECT payload FROM decodex.outbox WHERE id=$1", &[&outbox_id])
		.await?
		.get(0);

	assert!(
		payload.get("reset_card_effect").is_none(),
		"every terminal operation must erase the reversible private reset-card projection"
	);
	assert!(
		payload.get("payload").is_some(),
		"terminal reconciliation must retain the public operation projection"
	);

	Ok(())
}

fn assert_public_payload_is_private_material_free(payload: &Value) {
	let public =
		payload.get("payload").expect("the outbox must retain its public activity projection");
	let rendered = public.to_string();

	assert!(!rendered.contains("provider_idempotency"));
	assert!(!rendered.contains("exact_credit"));
	assert!(!rendered.contains("reset_card_effect"));
	assert!(!rendered.contains(EXACT_PROVIDER_CREDIT_ID));
	assert!(!rendered.contains(CREDENTIAL_WRITER));
}

async fn assert_activity_remains_public(
	store: &PostgresStore,
	owner: &tokio_postgres::Client,
	public_aggregate_id: &str,
) -> Result<(), Box<dyn Error>> {
	let persisted_identity = owner
		.query_one(
			"SELECT aggregate_id,correlation_key FROM decodex.activity \
			 WHERE aggregate_kind='reset_card_operation'",
			&[],
		)
		.await?;
	let persisted_aggregate_id: String = persisted_identity.get(0);
	let persisted_correlation_key: String = persisted_identity.get(1);
	let activities = store.activity_after(0, 1_000).await?;
	let activity = activities
		.iter()
		.find(|activity| activity.aggregate_kind == "reset_card_operation")
		.expect("the prepared reset-card operation must append public activity");
	let rendered = format!("{activity:?}");

	assert_eq!(persisted_aggregate_id, public_aggregate_id);
	assert_eq!(persisted_correlation_key, public_aggregate_id);
	assert!(!persisted_correlation_key.contains(OPERATION_KEY));
	assert_eq!(activity.aggregate_id, public_aggregate_id);
	assert_ne!(activity.aggregate_id, OPERATION_KEY);
	assert!(!rendered.contains("provider_idempotency"));
	assert!(!rendered.contains("exact_credit"));
	assert!(!rendered.contains("reset_card_effect"));
	assert!(!rendered.contains(EXACT_PROVIDER_CREDIT_ID));
	assert!(!rendered.contains(OPERATION_KEY));
	assert!(!rendered.contains(CREDENTIAL_WRITER));

	Ok(())
}
