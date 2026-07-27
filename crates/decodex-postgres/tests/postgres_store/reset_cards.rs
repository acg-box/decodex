use std::{error::Error, time::Duration};

use decodex_core::{
	AccountOperationId, AccountOperationKind, AccountOperationPhase, AccountProvider,
	AccountQuotaWindow, CredentialBinding, CredentialFingerprint, CredentialStoreSchemaVersion,
	CredentialVersion, ProcessGenerationAccountBinding, ProviderIdentity, ResetCardConsumeOutcome,
	ResetCardDescriptor, ResetCardTimestamp,
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

use super::{expected_peer_uid, separated_configs};

const ACCOUNT_ID: &str = "71000000-0000-4000-8000-000000000001";
const DISABLE_ACCOUNT_ID: &str = "71000000-0000-4000-8000-000000000002";
const ROTATION_ACCOUNT_ID: &str = "71000000-0000-4000-8000-000000000003";
const ATOMIC_ACCOUNT_ID: &str = "71000000-0000-4000-8000-000000000004";
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
const CALLBACK_PROFILE: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
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
	let (migration, runtime) = separated_configs("DECODEX_TEST")?;
	let store = PostgresStore::connect(migration, runtime, expected_peer_uid()).await?;
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
						"display_label": account.label.as_str(),
						"revision": account.revision,
					},
				}))
			},
		)
		.await?;
	assert_eq!(response["data"]["display_label"], "Atomic account");
	assert_eq!(
		store.update_account_administration(&account_id, 2, Some("Renamed later"), None).await?,
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
	assert_eq!(replay["data"]["display_label"], "Atomic account");
	assert_eq!(store.read_account_registry(Some(&account_id), 1).await?[0].label, "Renamed later");

	store.close();
	Ok(())
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
	let (migration, runtime) = separated_configs("DECODEX_TEST")?;
	let store = PostgresStore::connect(migration.clone(), runtime, expected_peer_uid()).await?;
	let (owner, owner_connection) = migration.connect(NoTls).await?;
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
	reject_pending_replay_after_account_change(&store, &owner, &account_id, reusable_descriptor)
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
	let (migration, runtime) = separated_configs("DECODEX_TEST")?;
	let store = PostgresStore::connect(migration.clone(), runtime, expected_peer_uid()).await?;
	let (owner, owner_connection) = migration.connect(NoTls).await?;
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
		store
			.update_account_administration(&account_id, INITIAL_ACCOUNT_REVISION, None, Some(false),)
			.await?,
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
	let (migration, runtime) = separated_configs("DECODEX_TEST")?;
	let store = PostgresStore::connect(migration.clone(), runtime, expected_peer_uid()).await?;
	let (owner, owner_connection) = migration.connect(NoTls).await?;
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
			 credential_writer_operation_id=$3::uuid,revision=revision+1, \
			 updated_at=clock_timestamp() WHERE account_id=$1::uuid",
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
			 WHERE account_id=$1::uuid",
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
		store
			.update_account_administration(
				account_id,
				INITIAL_ACCOUNT_REVISION,
				Some("Reset-card integration changed"),
				None,
			)
			.await?,
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
		store
			.update_account_administration(
				account_id,
				CHANGED_ACCOUNT_REVISION,
				Some("Reset-card integration"),
				None,
			)
			.await?,
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

async fn reject_pending_replay_after_account_change(
	store: &PostgresStore,
	owner: &tokio_postgres::Client,
	account_id: &AccountId,
	reusable_descriptor: ResetCardDescriptor,
) -> Result<(), Box<dyn Error>> {
	assert_eq!(
		store
			.update_account_administration(
				account_id,
				REENABLED_ACCOUNT_REVISION,
				Some("Reset-card integration final"),
				None,
			)
			.await?,
		AccountAdministrationOutcome::Updated { revision: FINAL_ACCOUNT_REVISION },
	);

	let pending_request = b"reset-card-pending-operation-v1".to_vec();
	let descriptor_source = format!(
		"{}:{}",
		reusable_descriptor.granted_at().unix_seconds(),
		reusable_descriptor.expires_at().unix_seconds(),
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
			&[
				&PENDING_OPERATION_KEY,
				&pending_request,
				&account_id.as_str(),
				&REENABLED_ACCOUNT_REVISION,
				&descriptor_source,
			],
		)
		.await?;
	let pending_command = CommandIdentity::new(PENDING_OPERATION_KEY, &pending_request)?;
	assert!(matches!(
		store
			.replay_reset_card_preparation(
				&pending_command,
				account_id,
				REENABLED_ACCOUNT_REVISION,
				reusable_descriptor,
			)
			.await,
		Err(StoreError::ResetCardCommitOutcomeUnknown)
	));
	time::sleep(Duration::from_millis(550)).await;
	assert!(matches!(
		store
			.replay_reset_card_preparation(
				&pending_command,
				account_id,
				REENABLED_ACCOUNT_REVISION,
				reusable_descriptor,
			)
			.await,
		Err(StoreError::RevisionConflict {
			expected: Some(REENABLED_ACCOUNT_REVISION),
			actual: Some(FINAL_ACCOUNT_REVISION),
			..
		})
	));
	let recovered_pending_state: String = owner
		.query_one(
			"SELECT receipt_state::text FROM decodex.command_receipts \
			 WHERE idempotency_key=$1",
			&[&PENDING_OPERATION_KEY],
		)
		.await?
		.get(0);
	assert_eq!(
		recovered_pending_state, "completed",
		"an expired pending receipt must enter fenced recovery and close after proved rejection"
	);

	Ok(())
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
				build_identity: "codex-cli 0.145.0-alpha.18".to_owned(),
				executable_sha256:
					"f0b214b476e04175bee104fe441caea874baeef3efc3828bfb79e972266156a9".to_owned(),
				schema_sha256: "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"
					.to_owned(),
				callback_profile_sha256: CALLBACK_PROFILE.to_owned(),
				login_chatgpt_auth_tokens: true,
				refresh_callback: true,
			})
			.await?
	);
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
