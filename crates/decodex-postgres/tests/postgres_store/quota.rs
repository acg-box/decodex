use std::{error::Error, time::Duration};

use tokio::task::JoinSet;
#[cfg(feature = "test-support")] use tokio::time;
use tokio_postgres::Client;

use crate::{ACCOUNT_ID, CREDENTIAL_VALUE_VECTORS, HOLDER_A, WORKER_A};
use decodex_core::{ObservationConfidence, QuotaWindowClass, RemainingPercent};
use decodex_postgres::{
	AccountId, AccountMutation, AccountState, CommandIdentity, PostgresStore,
	QuotaExclusionMutation, QuotaExclusionReceipt, QuotaTimestampMicros, QuotaWindowMutation,
	StoreError,
};

pub(super) async fn assert_inert_window_and_credential_boundary(
	store: &PostgresStore,
	client: &Client,
) -> Result<(), Box<dyn Error>> {
	let mutation = QuotaWindowMutation {
		account_id: AccountId::new(ACCOUNT_ID)?,
		window: QuotaWindowClass::FiveHour,
		remaining_percent: None,
		resets_at: None,
		observed_at: quota_timestamp("2026-07-13T07:00:00Z")?,
		confidence: ObservationConfidence::Unknown,
		metadata: serde_json::json!({
			"observation": "unknown",
			"token_budget": 8_192,
			"session_id": "ordinary-session-id",
		}),
		expected_revision: None,
	};
	let command = CommandIdentity::new("window-create", b"window-create-v1")?;
	#[cfg(feature = "test-support")]
	let initial_evictions = PostgresStore::quota_lock_evictions_fixture();

	#[cfg(feature = "test-support")]
	PostgresStore::fail_next_quota_unlock_fixture();

	let window = store.mutate_quota_window(&command, &mutation).await?;

	#[cfg(feature = "test-support")]
	assert_eq!(PostgresStore::quota_lock_evictions_fixture(), initial_evictions + 1);
	assert_eq!(window.window, QuotaWindowClass::FiveHour);
	assert_eq!(window.remaining_percent, None);
	assert_eq!(window.confidence, ObservationConfidence::Unknown);
	assert!(CommandIdentity::new("token_budget/session_id", b"ordinary-id").is_ok());

	assert_api_credential_boundary(store, &mutation).await?;
	assert_quota_timestamp_validation(client).await?;
	assert_inert_exclusion_transactions(store, client, &mutation).await?;

	for (key, command_key) in [
		("refresh_token", "window-credential"),
		("password", "account-credential"),
		("Authorization", "authorization-credential"),
		("session-token", "session-credential"),
		("accessToken", "camel-token-credential"),
		("bearer", "bearer-credential"),
		("value", "secret-value-credential"),
		("header", "bearer-value-credential"),
		("header", "basic-value-credential"),
		("value", "slack-value-credential"),
		("value", "gitlab-value-credential"),
		("value", "npm-value-credential"),
	] {
		if key != "password" {
			let mut forbidden = mutation.clone();

			forbidden.window = QuotaWindowClass::SevenDay;
			forbidden.metadata = match command_key {
				"secret-value-credential" => serde_json::json!({key: "sk-proj-0123456789abcdef"}),
				"bearer-value-credential" => serde_json::json!({key: "Bearer abcdefghijklmnop"}),
				"basic-value-credential" => serde_json::json!({key: "Basic dXNlcjpwYXNz"}),
				"slack-value-credential" => serde_json::json!({key: "xoxb-1234567890-abcdef"}),
				"gitlab-value-credential" => serde_json::json!({key: "glpat-1234567890abcdef"}),
				"npm-value-credential" => serde_json::json!({key: "npm_1234567890abcdef"}),
				_ => serde_json::json!({key: "forbidden"}),
			};

			let command = CommandIdentity::new(command_key, command_key.as_bytes())?;

			assert!(matches!(
				store.mutate_quota_window(&command, &forbidden).await,
				Err(StoreError::CredentialRejected)
			));
		} else {
			let forbidden = AccountMutation {
				account_id: AccountId::new("10000000-0000-0000-0000-000000000002")?,
				display_label: "Forbidden metadata".into(),
				state: AccountState::Unknown,
				metadata: serde_json::json!({key: "forbidden"}),
				expected_revision: None,
			};
			let command = CommandIdentity::new(command_key, command_key.as_bytes())?;

			assert!(matches!(
				store.mutate_account(&command, &forbidden).await,
				Err(StoreError::CredentialRejected)
			));
		}
	}

	crate::assert_direct_credential_and_scope_boundary(store, client).await
}

fn quota_timestamp(value: &str) -> Result<QuotaTimestampMicros, StoreError> {
	decodex_postgres::parse_quota_timestamp_rfc3339(value)
}

async fn assert_api_credential_boundary(
	store: &PostgresStore,
	mutation: &QuotaWindowMutation,
) -> Result<(), Box<dyn Error>> {
	for value in CREDENTIAL_VALUE_VECTORS {
		assert!(matches!(
			CommandIdentity::new(*value, b"forbidden"),
			Err(StoreError::CredentialRejected)
		));
	}

	let forbidden_label = AccountMutation {
		account_id: AccountId::new("10000000-0000-0000-0000-000000000002")?,
		display_label: "Basic dXNlcjpwYXNz".into(),
		state: AccountState::Unknown,
		metadata: serde_json::json!({}),
		expected_revision: None,
	};
	let safe_command = CommandIdentity::new("forbidden-label", b"forbidden-label")?;

	assert!(matches!(
		store.mutate_account(&safe_command, &forbidden_label).await,
		Err(StoreError::CredentialRejected)
	));

	let mut forbidden_metadata = mutation.clone();

	forbidden_metadata.metadata = serde_json::json!({"api_key": "forbidden"});

	assert!(matches!(
		store.mutate_quota_window(&safe_command, &forbidden_metadata).await,
		Err(StoreError::CredentialRejected)
	));
	assert!(matches!(
		store
			.try_acquire_lease("Bearer abcdefghijklmnop", HOLDER_A, Duration::from_millis(1),)
			.await,
		Err(StoreError::CredentialRejected)
	));
	assert!(matches!(
		store
			.retry_outbox_before_effect(
				0,
				WORKER_A,
				WORKER_A,
				"sk_proj_0123456789",
				Duration::from_millis(1),
			)
			.await,
		Err(StoreError::CredentialRejected)
	));

	Ok(())
}

async fn assert_quota_timestamp_validation(client: &Client) -> Result<(), Box<dyn Error>> {
	let utc = quota_timestamp("2026-07-13T07:00:00.123456Z")?;
	let offset = quota_timestamp("2026-07-13T09:30:00.123456+02:30")?;

	assert_eq!(utc, offset);

	for (index, invalid) in [
		"2026-07-13T07:00:00.123456789Z",
		"1969-12-31T23:59:59.999999Z",
		"10000-01-01T00:00:00Z",
		"2026-12-31T23:59:60Z",
		"infinity",
		"tomorrow",
		"2026-07-13 07:00:00Z",
	]
	.into_iter()
	.enumerate()
	{
		let key = format!("window-invalid-timestamp-{index}");

		assert!(decodex_postgres::parse_quota_timestamp_rfc3339(invalid).is_err(), "{invalid}");

		let receipt_count: i64 = client
			.query_one(
				"SELECT count(*) FROM decodex.command_receipts WHERE idempotency_key=$1",
				&[&key],
			)
			.await?
			.get(0);

		assert_eq!(receipt_count, 0);
	}
	for value in [-1_i64, QuotaTimestampMicros::MAX + 1] {
		assert!(QuotaTimestampMicros::new(value).is_err());
	}

	Ok(())
}

async fn assert_inert_exclusion_transactions(
	store: &PostgresStore,
	client: &Client,
	baseline: &QuotaWindowMutation,
) -> Result<(), Box<dyn Error>> {
	let observed = quota_timestamp("2026-07-13T07:05:00Z")?;
	let mut depleted = baseline.clone();

	depleted.remaining_percent = Some(RemainingPercent::new(0).expect("zero is valid"));
	depleted.resets_at = Some(quota_timestamp("2026-07-13T08:00:00Z")?);
	depleted.observed_at = observed;
	depleted.confidence = ObservationConfidence::High;
	depleted.metadata = serde_json::json!({
		"source": "synthetic_exact_microsecond_fixture",
		"nested": {"zeta": 2, "alpha": 1},
		"array": [1, "1", true],
	});
	depleted.expected_revision = Some(1);

	let exclusion = QuotaExclusionMutation {
		observation: depleted,
		excluded_at: QuotaTimestampMicros::new(observed.get() + 300_000_000)?,
	};
	let command =
		CommandIdentity::new("quota-exclusion-create", b"caller-bytes-are-not-authority")?;
	#[cfg(feature = "test-support")]
	let evictions_before_exclusion = PostgresStore::quota_lock_evictions_fixture();

	#[cfg(feature = "test-support")]
	PostgresStore::fail_next_quota_unlock_fixture();

	let receipt = store.persist_quota_exclusion(&command, &exclusion).await?;

	#[cfg(feature = "test-support")]
	assert_eq!(PostgresStore::quota_lock_evictions_fixture(), evictions_before_exclusion + 1);
	assert_eq!(receipt.observation_revision, 2);
	assert!(!receipt.hypothetical_fallback.dispatch_enabled());
	assert_eq!(store.persist_quota_exclusion(&command, &exclusion).await?, receipt);
	assert_eq!(
		store
			.persist_quota_exclusion(
				&CommandIdentity::new("quota-exclusion-create", b"different-caller-bytes")?,
				&exclusion,
			)
			.await?,
		receipt
	);

	let mut reordered = exclusion.clone();

	reordered.observation.metadata = serde_json::from_str(
		r#"{"array":[1,"1",true],"nested":{"alpha":1,"zeta":2},"source":"synthetic_exact_microsecond_fixture"}"#,
	)?;

	assert_eq!(store.persist_quota_exclusion(&command, &reordered).await?, receipt);

	let mut changed = exclusion.clone();

	changed.excluded_at = QuotaTimestampMicros::new(changed.excluded_at.get() - 1)?;

	assert!(matches!(
		store.persist_quota_exclusion(&command, &changed).await,
		Err(StoreError::IdempotencyConflict)
	));

	let mut stale = exclusion.clone();

	stale.observation.expected_revision = Some(2);
	stale.observation.observed_at = QuotaTimestampMicros::new(observed.get() + 1_000_000)?;
	stale.excluded_at =
		QuotaTimestampMicros::new(stale.observation.observed_at.get() + 300_000_001)?;

	let stale_key = "quota-exclusion-stale-by-one-microsecond";

	assert!(matches!(
		store
			.persist_quota_exclusion(
				&CommandIdentity::new(stale_key, b"invalid-before-receipt")?,
				&stale,
			)
			.await,
		Err(StoreError::InvalidInput(_))
	));

	let stale_side_effects = client
		.query_one(
			"SELECT \
			 (SELECT count(*) FROM decodex.command_receipts WHERE idempotency_key=$1), \
			 (SELECT count(*) FROM decodex.activity WHERE correlation_key=$1), \
			 (SELECT count(*) FROM decodex.outbox AS work JOIN decodex.activity AS event \
			  ON work.payload @> jsonb_build_object('activity_sequence',event.sequence) \
			  WHERE event.correlation_key=$1), \
			 (SELECT count(*) FROM decodex.quota_exclusions WHERE observation_revision > 2)",
			&[&stale_key],
		)
		.await?;

	for index in 0..4 {
		assert_eq!(stale_side_effects.get::<_, i64>(index), 0);
	}

	assert_exclusion_rows_are_inert(client, &receipt).await?;
	assert_monotonic_quota_observations(store, client, &exclusion.observation).await?;
	assert_concurrent_quota_writers(store, client, &exclusion.observation).await?;
	#[cfg(feature = "test-support")]
	assert_exclusion_crash_retry(store, client, baseline).await?;

	Ok(())
}

async fn assert_exclusion_rows_are_inert(
	client: &Client,
	receipt: &QuotaExclusionReceipt,
) -> Result<(), Box<dyn Error>> {
	let row = client
		.query_one(
			"SELECT count(*),bool_and(NOT dispatch_enabled),bool_and(maximum_age_micros=300000000), \
			 bool_and(mutation_sha256=$1),bool_and(mutation_length=$2) \
			 FROM decodex.quota_exclusions WHERE account_id=$3::text::uuid \
			 AND window_class='five_hour' AND duration_minutes=300",
			&[&receipt.mutation_sha256, &receipt.mutation_length, &ACCOUNT_ID],
		)
		.await?;

	assert_eq!(row.get::<_, i64>(0), 1);
	assert!(row.get::<_, bool>(1));
	assert!(row.get::<_, bool>(2));
	assert!(row.get::<_, bool>(3));
	assert!(row.get::<_, bool>(4));

	assert_sql_exclusion_freshness_boundaries(client).await?;

	Ok(())
}

async fn assert_sql_exclusion_freshness_boundaries(client: &Client) -> Result<(), Box<dyn Error>> {
	client.batch_execute("BEGIN").await?;
	client
		.batch_execute(
			"INSERT INTO decodex.accounts (account_id,display_label) VALUES \
			 ('10000000-0000-0000-0000-000000000093','Quota maximum fixture'); \
			 INSERT INTO decodex.quota_windows \
			 (account_id,window_class,duration_minutes,observed_at,confidence,metadata) VALUES \
			 ('10000000-0000-0000-0000-000000000093','five_hour',300, \
			  TIMESTAMPTZ '1970-01-01 00:00:00+00' \
			  + (253402300799999999::bigint / 1000000) * INTERVAL '1 second' \
			  + (253402300799999999::bigint % 1000000) * INTERVAL '1 microsecond', \
			  'unknown','{}')",
		)
		.await?;

	let maximum: i64 = client
		.query_one(
			"SELECT (extract(epoch FROM observed_at)::numeric * 1000000)::bigint \
			 FROM decodex.quota_windows \
			 WHERE account_id='10000000-0000-0000-0000-000000000093'",
			&[],
		)
		.await?
		.get(0);

	assert_eq!(maximum, QuotaTimestampMicros::MAX);

	for (revision, age) in [(900_i64, 0_i64), (901, 299_999_999), (902, 300_000_000)] {
		client
			.execute(
				"INSERT INTO decodex.quota_exclusions \
				 (account_id,window_class,duration_minutes,observation_revision,remaining_percent, \
				  confidence,observation_metadata,observed_at_micros,resets_at_micros, \
				  excluded_at_micros,maximum_age_micros,mutation_sha256,mutation_length) VALUES \
				 ($1::text::uuid,'five_hour',300,$2,0,'high','{}',1700000000000000, \
				  1800000000000000,1700000000000000+$3,300000000,repeat('a',64),1)",
				&[&ACCOUNT_ID, &revision, &age],
			)
			.await?;
	}

	client.batch_execute("SAVEPOINT stale_freshness").await?;

	let error = client
		.execute(
			"INSERT INTO decodex.quota_exclusions \
			 (account_id,window_class,duration_minutes,observation_revision,remaining_percent, \
			  confidence,observation_metadata,observed_at_micros,resets_at_micros, \
			  excluded_at_micros,maximum_age_micros,mutation_sha256,mutation_length) VALUES \
			 ($1::text::uuid,'five_hour',300,903,0,'high','{}',1700000000000000, \
			  1800000000000000,1700000300000001,300000000,repeat('b',64),1)",
			&[&ACCOUNT_ID],
		)
		.await
		.expect_err("300 seconds plus one microsecond is stale in PostgreSQL");

	assert_eq!(
		error.as_db_error().and_then(|database| database.constraint()),
		Some("quota_exclusions_freshness")
	);

	client.batch_execute("ROLLBACK TO stale_freshness; ROLLBACK").await?;

	Ok(())
}

async fn assert_monotonic_quota_observations(
	store: &PostgresStore,
	client: &Client,
	baseline: &QuotaWindowMutation,
) -> Result<(), Box<dyn Error>> {
	let mut stale = baseline.clone();

	stale.expected_revision = Some(2);
	stale.observed_at = QuotaTimestampMicros::new(baseline.observed_at.get() - 1)?;

	let key = "quota-window-stale-observation";
	let result =
		store.mutate_quota_window(&CommandIdentity::new(key, b"stale-observation")?, &stale).await;

	assert!(matches!(
		result,
		Err(StoreError::InvalidInput("quota observation cannot move backward in time"))
	));

	let state = client
		.query_one(
			"SELECT revision,(extract(epoch FROM observed_at)::numeric * 1000000)::bigint, \
			 (SELECT count(*) FROM decodex.command_receipts WHERE idempotency_key=$1), \
			 (SELECT count(*) FROM decodex.activity WHERE correlation_key=$1), \
			 (SELECT count(*) FROM decodex.outbox AS work JOIN decodex.activity AS event \
			  ON work.payload @> jsonb_build_object('activity_sequence',event.sequence) \
			  WHERE event.correlation_key=$1) \
			 FROM decodex.quota_windows WHERE account_id=$2::text::uuid \
			 AND window_class='five_hour' AND duration_minutes=300",
			&[&key, &ACCOUNT_ID],
		)
		.await?;

	assert_eq!(state.get::<_, i64>(0), 2);
	assert_eq!(state.get::<_, i64>(1), baseline.observed_at.get());
	assert_eq!(state.get::<_, i64>(2), 0);
	assert_eq!(state.get::<_, i64>(3), 0);
	assert_eq!(state.get::<_, i64>(4), 0);
	assert_eq!(
		client
			.execute(
				"UPDATE decodex.quota_windows SET observed_at=observed_at \
				 WHERE account_id=$1::text::uuid AND window_class='five_hour'",
				&[&ACCOUNT_ID],
			)
			.await?,
		1
	);

	let direct_stale = client
		.execute(
			"UPDATE decodex.quota_windows SET observed_at=observed_at-interval '1 microsecond' \
			 WHERE account_id=$1::text::uuid AND window_class='five_hour'",
			&[&ACCOUNT_ID],
		)
		.await
		.expect_err("database boundary rejects a regressing observation");

	assert_eq!(
		direct_stale.as_db_error().and_then(|database| database.constraint()),
		Some("quota_windows_observed_at_monotonic")
	);

	Ok(())
}

async fn assert_concurrent_quota_writers(
	store: &PostgresStore,
	client: &Client,
	baseline: &QuotaWindowMutation,
) -> Result<(), Box<dyn Error>> {
	let mut attempts = JoinSet::new();

	for index in 0..8 {
		let store = store.clone();
		let mut mutation = baseline.clone();

		mutation.expected_revision = Some(2);
		mutation.metadata = serde_json::json!({"concurrent_writer": index});

		attempts.spawn(async move {
			store
				.mutate_quota_window(
					&CommandIdentity::new(
						format!("quota-concurrent-{index}"),
						format!("caller-{index}").as_bytes(),
					)?,
					&mutation,
				)
				.await
		});
	}

	let mut successes = 0;
	let mut conflicts = 0;

	while let Some(result) = attempts.join_next().await {
		match result? {
			Ok(_) => successes += 1,
			Err(StoreError::RevisionConflict { expected: Some(2), actual: Some(3), .. }) => {
				conflicts += 1;
			},
			Err(error) => return Err(error.into()),
		}
	}

	assert_eq!((successes, conflicts), (1, 7));

	let receipt_count: i64 = client
		.query_one(
			"SELECT count(*) FROM decodex.command_receipts \
			 WHERE idempotency_key LIKE 'quota-concurrent-%'",
			&[],
		)
		.await?
		.get(0);

	assert_eq!(receipt_count, 1);

	Ok(())
}

#[cfg(feature = "test-support")]
async fn assert_exclusion_crash_retry(
	store: &PostgresStore,
	client: &Client,
	baseline: &QuotaWindowMutation,
) -> Result<(), Box<dyn Error>> {
	let mut observation = baseline.clone();

	observation.window = QuotaWindowClass::SevenDay;
	observation.remaining_percent = Some(RemainingPercent::new(0).expect("zero is valid"));
	observation.resets_at = Some(quota_timestamp("2026-07-14T08:00:00Z")?);
	observation.observed_at = quota_timestamp("2026-07-14T07:00:00Z")?;
	observation.confidence = ObservationConfidence::High;
	observation.metadata = serde_json::json!({"crash_fixture": "after_reservation"});
	observation.expected_revision = None;

	let exclusion = QuotaExclusionMutation {
		excluded_at: QuotaTimestampMicros::new(observation.observed_at.get() + 299_999_999)?,
		observation,
	};
	let key = "quota-exclusion-crash-after-reservation";
	let command = CommandIdentity::new(key, b"caller-bytes-not-authority")?;

	PostgresStore::fail_next_exclusion_after_reservation_fixture();

	assert!(matches!(
		store.persist_quota_exclusion(&command, &exclusion).await,
		Err(StoreError::CapacityExhausted(_))
	));

	let state = client
		.query_one(
			"SELECT \
			 (SELECT count(*) FROM decodex.command_receipts WHERE idempotency_key=$1), \
			 (SELECT count(*) FROM decodex.quota_windows WHERE account_id=$2::text::uuid \
			  AND window_class='seven_day'), \
			 (SELECT count(*) FROM decodex.quota_exclusions WHERE account_id=$2::text::uuid \
			  AND window_class='seven_day'), \
			 (SELECT count(*) FROM decodex.activity WHERE correlation_key=$1), \
			 (SELECT count(*) FROM decodex.outbox AS work JOIN decodex.activity AS event \
			  ON work.payload @> jsonb_build_object('activity_sequence',event.sequence) \
			  WHERE event.correlation_key=$1)",
			&[&key, &ACCOUNT_ID],
		)
		.await?;

	assert_eq!(state.get::<_, i64>(0), 1);

	for index in 1..5 {
		assert_eq!(state.get::<_, i64>(index), 0);
	}

	let retry_store = store.clone();
	let retry_command = command.clone();
	let retry_exclusion = exclusion.clone();
	let retry = tokio::spawn(async move {
		retry_store.persist_quota_exclusion(&retry_command, &retry_exclusion).await
	});

	time::sleep(Duration::from_millis(100)).await;

	retry.abort();

	assert!(retry.await.expect_err("retry task is cancelled").is_cancelled());

	let mut changed = exclusion.clone();

	changed.observation.metadata = serde_json::json!({"crash_fixture": "changed"});

	assert!(matches!(
		time::timeout(Duration::from_secs(1), store.persist_quota_exclusion(&command, &changed),)
			.await
			.expect("cancelled quota lock is released"),
		Err(StoreError::IdempotencyConflict)
	));

	client
		.batch_execute(
			"ALTER TABLE decodex.command_receipts DISABLE TRIGGER command_receipts_state_guard; \
			 UPDATE decodex.command_receipts SET created_at=clock_timestamp()-interval '10 minutes', \
			 claim_expires_at=clock_timestamp()-interval '1 second' \
			 WHERE idempotency_key='quota-exclusion-crash-after-reservation'; \
			 ALTER TABLE decodex.command_receipts ENABLE TRIGGER command_receipts_state_guard",
		)
		.await?;

	let receipt = store.persist_quota_exclusion(&command, &exclusion).await?;

	assert_eq!(receipt.window, QuotaWindowClass::SevenDay);
	assert_eq!(receipt.excluded_at.get() - receipt.observed_at.get(), 299_999_999);
	assert_eq!(store.persist_quota_exclusion(&command, &exclusion).await?, receipt);
	assert!(matches!(
		store.persist_quota_exclusion(&command, &changed).await,
		Err(StoreError::IdempotencyConflict)
	));

	Ok(())
}
