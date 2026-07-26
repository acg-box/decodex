use std::{error::Error, time::Duration};

use decodex_core::{
	RESET_CARD_PROVIDER_BINDING_METADATA_FIELD, ResetCardConsumeOutcome, ResetCardDescriptor,
	ResetCardTimestamp,
};
use decodex_postgres::{
	AccountId, AccountMutation, AccountState, CommandIdentity, OutboxReconciliation, PostgresStore,
	ReconciliationOutcome, ResetCardFailureCode, ResetCardOperationStatus, StoreError,
};
use serde_json::{Value, json};
use tokio::time;
use tokio_postgres::NoTls;

use super::{expected_peer_uid, separated_configs};

const ACCOUNT_ID: &str = "71000000-0000-4000-8000-000000000001";
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
const PROVIDER_BINDING_FINGERPRINT: &str =
	"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires a fresh isolated PostgreSQL 18 reset-card database"]
async fn reset_card_private_claim_and_reclaim_contract() -> Result<(), Box<dyn Error>> {
	let (migration, runtime) = separated_configs("DECODEX_TEST")?;
	let store = PostgresStore::connect(migration.clone(), runtime, expected_peer_uid()).await?;
	let (owner, owner_connection) = migration.connect(NoTls).await?;
	let owner_connection_task = tokio::spawn(owner_connection);
	let account_id = AccountId::new(ACCOUNT_ID)?;
	let account_command =
		CommandIdentity::new("reset-card-integration-account", b"reset-card-account-v1")?;

	store
		.mutate_account(
			&account_command,
			&AccountMutation {
				account_id: account_id.clone(),
				display_label: "Reset-card integration".into(),
				state: AccountState::Available,
				metadata: account_metadata(),
				expected_revision: None,
			},
		)
		.await?;

	let drift_command =
		CommandIdentity::new("reset-card-integration-binding-drift", b"binding-drift-v1")?;
	let drift_result = store
		.mutate_account(
			&drift_command,
			&AccountMutation {
				account_id: account_id.clone(),
				display_label: "Reset-card integration".into(),
				state: AccountState::Available,
				metadata: json!({
					"fixture": "reset_card",
					(RESET_CARD_PROVIDER_BINDING_METADATA_FIELD):
						"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
				}),
				expected_revision: Some(1),
			},
		)
		.await;
	assert!(
		matches!(
			&drift_result,
			Err(StoreError::RevisionConflict { expected: Some(1), actual: Some(1), .. })
		),
		"unexpected immutable-binding mutation result: {drift_result:?}"
	);

	let descriptor = ResetCardDescriptor::new(
		ResetCardTimestamp::from_unix_seconds(2_000_000_000)?,
		ResetCardTimestamp::from_unix_seconds(2_000_003_600)?,
	)?;
	let operation_command = CommandIdentity::new(OPERATION_KEY, b"reset-card-operation-v1")?;
	let preparation =
		store.prepare_reset_card_operation(&operation_command, &account_id, 1, descriptor).await?;
	assert!(store.reset_card_account_has_unsettled_operations(&account_id).await?);

	assert_eq!(preparation.account_id, account_id);
	assert_eq!(preparation.account_revision, 1);
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
	assert!(!prepared_payload.to_string().contains(OPERATION_KEY));
	assert_activity_remains_public(&store, &owner, &public_aggregate_id).await?;

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
	assert_activity_remains_public(&store, &owner, &public_aggregate_id).await?;

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

	let competing_command =
		CommandIdentity::new(COMPETING_OPERATION_KEY, b"reset-card-operation-v1")?;
	let competing_preparation =
		store.prepare_reset_card_operation(&competing_command, &account_id, 1, descriptor).await?;

	assert_eq!(competing_preparation.account_revision, 1);
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
	assert_private_effect_scrubbed(&owner, competing.id).await?;

	let exhausted_command =
		CommandIdentity::new(EXHAUSTED_OPERATION_KEY, b"reset-card-exhausted-operation-v1")?;
	store.prepare_reset_card_operation(&exhausted_command, &account_id, 1, descriptor).await?;
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
	assert_private_effect_scrubbed(&owner, exhausted.id).await?;

	let account_update =
		CommandIdentity::new("reset-card-integration-account-update", b"account-disabled-v2")?;
	let updated = store
		.mutate_account(
			&account_update,
			&AccountMutation {
				account_id: account_id.clone(),
				display_label: "Reset-card integration".into(),
				state: AccountState::Disabled,
				metadata: account_metadata(),
				expected_revision: Some(1),
			},
		)
		.await?;

	assert_eq!(updated.revision, 2);
	assert_eq!(updated.state, AccountState::Disabled);
	assert_eq!(
		store.prepare_reset_card_operation(&operation_command, &account_id, 1, descriptor).await?,
		preparation,
		"an exact completed key must replay before current account admission",
	);
	let stale_command =
		CommandIdentity::new("reset-card-integration-stale-operation", b"stale-operation-v1")?;
	let stale_result =
		store.prepare_reset_card_operation(&stale_command, &account_id, 1, descriptor).await;
	assert!(
		matches!(
			&stale_result,
			Err(StoreError::RevisionConflict { expected: Some(1), actual: Some(2), .. })
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
		store.prepare_reset_card_operation(&stale_command, &account_id, 1, descriptor).await,
		Err(StoreError::RevisionConflict { expected: Some(1), actual: Some(2), .. })
	));

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
	assert_eq!(recovered.account_revision, 1);
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
	assert_eq!(final_claim.account_revision, 1);
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

	assert_private_effect_scrubbed(&owner, final_claim.id).await?;
	assert_eq!(
		store.reset_card_operation_status(OPERATION_KEY).await?,
		ResetCardOperationStatus::Completed(ResetCardConsumeOutcome::Reset)
	);
	assert!(!store.reset_card_account_has_unsettled_operations(&account_id).await?);

	let account_reenable =
		CommandIdentity::new("reset-card-integration-account-reenable", b"account-available-v3")?;
	let reenabled = store
		.mutate_account(
			&account_reenable,
			&AccountMutation {
				account_id: account_id.clone(),
				display_label: "Reset-card integration".into(),
				state: AccountState::Available,
				metadata: account_metadata(),
				expected_revision: Some(2),
			},
		)
		.await?;
	assert_eq!(reenabled.revision, 3);

	let reusable_descriptor = ResetCardDescriptor::new(
		ResetCardTimestamp::from_unix_seconds(2_100_000_000)?,
		ResetCardTimestamp::from_unix_seconds(2_100_003_600)?,
	)?;
	let nothing_command =
		CommandIdentity::new(NOTHING_TO_RESET_KEY, b"reset-card-nothing-to-reset-v1")?;
	let nothing_preparation = store
		.prepare_reset_card_operation(&nothing_command, &account_id, 3, reusable_descriptor)
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
					"account_revision": 3,
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
	assert_private_effect_scrubbed(&owner, nothing_claim.id).await?;
	assert_eq!(
		store.reset_card_operation_status(NOTHING_TO_RESET_KEY).await?,
		ResetCardOperationStatus::Completed(ResetCardConsumeOutcome::NothingToReset),
	);

	let reusable_command =
		CommandIdentity::new(REUSABLE_OPERATION_KEY, b"reset-card-reusable-operation-v1")?;
	store
		.prepare_reset_card_operation(&reusable_command, &account_id, 3, reusable_descriptor)
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
		store.prepare_reset_card_operation(&operation_command, &account_id, 1, descriptor).await?,
		preparation,
	);
	assert_eq!(
		store
			.prepare_reset_card_operation(&nothing_command, &account_id, 3, reusable_descriptor)
			.await?,
		nothing_preparation,
	);

	let final_account_update =
		CommandIdentity::new("reset-card-integration-account-final", b"account-disabled-v4")?;
	let final_account = store
		.mutate_account(
			&final_account_update,
			&AccountMutation {
				account_id: account_id.clone(),
				display_label: "Reset-card integration".into(),
				state: AccountState::Disabled,
				metadata: account_metadata(),
				expected_revision: Some(3),
			},
		)
		.await?;
	assert_eq!(final_account.revision, 4);

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
				&3_i64,
				&descriptor_source,
			],
		)
		.await?;
	let pending_command = CommandIdentity::new(PENDING_OPERATION_KEY, &pending_request)?;
	assert!(matches!(
		store
			.replay_reset_card_preparation(&pending_command, &account_id, 3, reusable_descriptor,)
			.await,
		Err(StoreError::ResetCardCommitOutcomeUnknown)
	));
	time::sleep(Duration::from_millis(550)).await;
	assert!(matches!(
		store
			.replay_reset_card_preparation(&pending_command, &account_id, 3, reusable_descriptor,)
			.await,
		Err(StoreError::RevisionConflict { expected: Some(3), actual: Some(4), .. })
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

	store.close();
	drop(owner);
	owner_connection_task.await??;

	Ok(())
}

fn account_metadata() -> Value {
	json!({
		"fixture": "reset_card",
		(RESET_CARD_PROVIDER_BINDING_METADATA_FIELD): PROVIDER_BINDING_FINGERPRINT,
	})
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

	Ok(())
}
