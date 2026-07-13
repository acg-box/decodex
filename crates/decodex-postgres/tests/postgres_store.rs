//! Real PostgreSQL contract coverage for the XY-1267 persistence foundation.

use std::{collections::HashSet, env, error::Error, str::FromStr as _, time::Duration};

use ::time as _;
use deadpool_postgres as _;
use refinery as _;
use regex as _;
use serde_json::Value;
use sha2 as _;
use tokio::{task::JoinSet, time};
use tokio_postgres::{Client, Config, NoTls, types::ToSql};

use decodex_core::{Availability, ProductState as _};
use decodex_postgres::{
	AccountId, AccountMutation, AccountState, CLOSED, CommandIdentity,
	MAX_OPERATION_DURATION_MILLISECONDS, OutboxClaim, OutboxReconciliation, PostgresStore,
	QuotaWindowMutation, ReconciliationOutcome, StoreError,
};

const ACCOUNT_ID: &str = "10000000-0000-0000-0000-000000000001";
const HOLDER_A: &str = "20000000-0000-0000-0000-000000000001";
const HOLDER_B: &str = "20000000-0000-0000-0000-000000000002";
const WORKER_A: &str = "30000000-0000-0000-0000-000000000001";
const WORKER_B: &str = "30000000-0000-0000-0000-000000000002";
const CREDENTIAL_VALUE_VECTORS: &[&str] = &[
	"Bearer abcdefghijklmnop",
	"Bearer\nabcdefghijklmnop",
	"Bearer\u{a0}abcdefghijklmnop",
	"Bearer\u{85}abcdefghijklmnop",
	"Basic\u{202f}dXNlcjpwYXNz",
	"password\u{3000}=\u{3000}forbidden",
	"Basic\tdXNlcjpwYXNz",
	"sk-0123456789abcdef",
	"sk_live_0123456789abcdef",
	"xoxb-1234567890-abcdef",
	"glpat-1234567890abcdef",
	"npm_1234567890abcdef",
	"ghp_01234567890123456789",
	"eyJ0123456789.abcdefghij.klmnopqrst",
	"-----BEGIN RSA PRIVATE\nKEY-----",
	"password\n=\nforbidden",
	"https://user:password@example.invalid/path",
	"AKIA0123456789ABCDEF",
];
const UNICODE_WHITESPACE_VECTORS: &[&str] = &["\u{a0}", "\u{85}", "\u{202f}", "\u{3000}"];

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "requires the isolated PostgreSQL 18 harness"]
async fn postgres_store_contract() -> Result<(), Box<dyn Error>> {
	let database_url = env::var("DECODEX_TEST_DATABASE_URL")?;
	let config = Config::from_str(&database_url)?;
	let store = PostgresStore::connect(config.clone()).await?;
	let (client, connection) = config.connect(NoTls).await?;
	let connection_task = tokio::spawn(connection);

	assert_eq!(store.availability(), Availability::Available);

	assert_bootstrap_and_history(&client).await?;
	assert_account_idempotency_and_revision(&store, &client).await?;
	assert_inert_window_and_credential_boundary(&store, &client).await?;
	assert_duration_validation(&store, &client).await?;
	assert_lease_contention_and_reclaim(&store).await?;
	assert_outbox_concurrency_retry_and_restart(&store, &client, &config).await?;

	assert_eq!(store.availability(), Availability::Unavailable { reason: CLOSED });

	assert_closed_pool_behavior(&store).await?;
	assert_primary_indexes_are_plan_eligible(&client).await?;
	assert_incompatible_history_fails_closed(&client, &config).await?;
	drop(client);

	connection_task.await??;

	Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated PostgreSQL 18 restore harness"]
async fn postgres_store_restored_contract() -> Result<(), Box<dyn Error>> {
	let database_url = env::var("DECODEX_TEST_DATABASE_URL")?;
	let config = Config::from_str(&database_url)?;
	let store = PostgresStore::connect(config.clone()).await?;
	let (client, connection) = config.connect(NoTls).await?;
	let connection_task = tokio::spawn(connection);

	assert_eq!(store.availability(), Availability::Available);

	assert_bootstrap_and_history(&client).await?;

	assert!(store.account(&AccountId::new(ACCOUNT_ID)?).await?.is_some());

	let ordinary_rows: i64 = client
		.query_one(
			"SELECT (SELECT count(*) FROM decodex.accounts) \
			      + (SELECT count(*) FROM decodex.command_receipts) \
			      + (SELECT count(*) FROM decodex.activity) \
			      + (SELECT count(*) FROM decodex.outbox)",
			&[],
		)
		.await?
		.get(0);

	assert!(ordinary_rows > 0);

	store.close();

	assert_eq!(store.availability(), Availability::Unavailable { reason: CLOSED });

	drop(client);

	connection_task.await??;

	Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated PostgreSQL 18 Turkish ICU collation harness"]
async fn postgres_store_turkish_collation_contract() -> Result<(), Box<dyn Error>> {
	let database_url = env::var("DECODEX_TEST_COLLATION_DATABASE_URL")?;
	let config = Config::from_str(&database_url)?;
	let store = PostgresStore::connect(config.clone()).await?;
	let (client, connection) = config.connect(NoTls).await?;
	let connection_task = tokio::spawn(connection);
	let locale = client
		.query_one(
			"SELECT datlocprovider::text, datlocale FROM pg_database \
			 WHERE datname = current_database()",
			&[],
		)
		.await?;
	let provider: String = locale.get(0);
	let locale: Option<String> = locale.get(1);

	assert_eq!(provider, "i");
	assert!(locale.as_deref().is_some_and(|value| value.starts_with("tr")));

	for (index, response) in [
		serde_json::json!({"AUTHORIZATION": "forbidden"}),
		serde_json::json!({"HEADER": "BEARER ABCDEFGHIJKLMNOP"}),
		serde_json::json!({"PRIVATE_KEY": "forbidden"}),
	]
	.into_iter()
	.enumerate()
	{
		let key = format!("turkish-collation-{index}");

		assert_credential_constraint(
			&client,
			"INSERT INTO decodex.command_receipts \
			 (idempotency_key, request_hash, response, completed_at) \
			 VALUES ($1, repeat('a', 64), $2, clock_timestamp())",
			&[&key, &response],
			"command_receipts_no_credentials",
		)
		.await?;
	}

	store.close();

	drop(client);

	connection_task.await??;

	Ok(())
}

async fn assert_bootstrap_and_history(client: &Client) -> Result<(), Box<dyn Error>> {
	let version: i32 = client
		.query_one("SELECT current_setting('server_version_num')::integer / 10000", &[])
		.await?
		.get(0);
	let checksums: String =
		client.query_one("SELECT current_setting('data_checksums')", &[]).await?.get(0);
	let rows = client
		.query("SELECT version, name, checksum FROM refinery_schema_history ORDER BY version", &[])
		.await?;
	let history: Vec<(i32, String, String)> =
		rows.into_iter().map(|row| (row.get(0), row.get(1), row.get(2))).collect();

	assert_eq!(version, 18);
	assert_eq!(checksums, "on");
	assert_eq!(history.len(), 2);
	assert_eq!(history[0].0, 1);
	assert_eq!(history[0].1, "persistence_foundation");
	assert!(!history[0].2.is_empty());
	assert_eq!(history[1].0, 2);
	assert_eq!(history[1].1, "claim_indexes");
	assert!(!history[1].2.is_empty());

	PostgresStore::connect(Config::from_str(&env::var("DECODEX_TEST_DATABASE_URL")?)?).await?;

	let mut tcp = Config::new();

	tcp.host("127.0.0.1");

	assert!(matches!(
		PostgresStore::connect(tcp).await,
		Err(StoreError::Incompatible(reason)) if reason.contains("Unix socket")
	));

	let mut missing_database = Config::from_str(&env::var("DECODEX_TEST_DATABASE_URL")?)?;

	missing_database.dbname("decodex_xy1267_missing");

	assert!(matches!(PostgresStore::connect(missing_database).await, Err(StoreError::Pool(_))));

	Ok(())
}

async fn assert_account_idempotency_and_revision(
	store: &PostgresStore,
	client: &Client,
) -> Result<(), Box<dyn Error>> {
	let account_id = AccountId::new(ACCOUNT_ID)?;
	let mutation = AccountMutation {
		account_id: account_id.clone(),
		display_label: "Primary metadata".into(),
		state: AccountState::Unknown,
		metadata: serde_json::json!({"provider": "codex", "source": "synthetic"}),
		expected_revision: None,
	};
	let command = CommandIdentity::new("account-create", b"account-create-v1")?;
	let mut tasks = JoinSet::new();

	for _ in 0..16 {
		let store = store.clone();
		let mutation = mutation.clone();
		let command = command.clone();

		tasks.spawn(async move { store.mutate_account(&command, &mutation).await });
	}

	while let Some(result) = tasks.join_next().await {
		let account = result??;

		assert_eq!(account.revision, 1);
		assert_eq!(account.state, AccountState::Unknown);
	}

	let counts: (i64, i64, i64) = {
		let row = client
			.query_one(
				"SELECT (SELECT count(*) FROM decodex.accounts), \
				 (SELECT count(*) FROM decodex.activity WHERE aggregate_kind = 'account'), \
				 (SELECT count(*) FROM decodex.outbox WHERE aggregate_kind = 'account')",
				&[],
			)
			.await?;

		(row.get(0), row.get(1), row.get(2))
	};

	assert_eq!(counts, (1, 1, 1));

	let different = CommandIdentity::new("account-create", b"different-request")?;

	assert!(matches!(
		store.mutate_account(&different, &mutation).await,
		Err(StoreError::IdempotencyConflict)
	));

	let mut tasks = JoinSet::new();

	for writer in 0..16 {
		let store = store.clone();
		let mutation = AccountMutation {
			account_id: account_id.clone(),
			display_label: format!("Writer {writer}"),
			state: AccountState::Unknown,
			metadata: serde_json::json!({"writer": writer}),
			expected_revision: Some(1),
		};
		let command = CommandIdentity::new(
			format!("account-update-{writer}"),
			format!("writer-{writer}").as_bytes(),
		)?;

		tasks.spawn(async move { store.mutate_account(&command, &mutation).await });
	}

	let mut winners = 0;
	let mut conflicts = 0;

	while let Some(result) = tasks.join_next().await {
		match result? {
			Ok(account) => {
				assert_eq!(account.revision, 2);

				winners += 1;
			},
			Err(StoreError::RevisionConflict { expected: Some(1), actual: Some(2), .. }) => {
				conflicts += 1;
			},
			Err(error) => return Err(error.into()),
		}
	}

	assert_eq!((winners, conflicts), (1, 15));
	assert_eq!(store.account(&account_id).await?.expect("account exists").revision, 2);

	Ok(())
}

async fn assert_inert_window_and_credential_boundary(
	store: &PostgresStore,
	client: &Client,
) -> Result<(), Box<dyn Error>> {
	let mutation = QuotaWindowMutation {
		account_id: AccountId::new(ACCOUNT_ID)?,
		window_class: "usage".into(),
		duration_seconds: 300,
		remaining_amount: None,
		resets_at: None,
		observed_at: "2026-07-13T07:00:00Z".into(),
		confidence: 0.0,
		metadata: serde_json::json!({
			"observation": "unknown",
			"token_budget": 8_192,
			"session_id": "ordinary-session-id",
		}),
		expected_revision: None,
	};
	let command = CommandIdentity::new("window-create", b"window-create-v1")?;
	let window = store.mutate_quota_window(&command, &mutation).await?;

	assert_eq!(window.duration_seconds, 300);
	assert_eq!(window.remaining_amount, None);
	assert_eq!(window.confidence, 0.0);
	assert!(CommandIdentity::new("token_budget/session_id", b"ordinary-id").is_ok());

	assert_api_credential_boundary(store, &mutation).await?;
	assert_quota_timestamp_validation(store, client, &mutation).await?;

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

			forbidden.duration_seconds = 10_080;
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

	assert_direct_credential_and_scope_boundary(store, client).await
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

	let mut forbidden_window_class = mutation.clone();

	forbidden_window_class.window_class = "sk_proj_0123456789".into();

	assert!(matches!(
		store.mutate_quota_window(&safe_command, &forbidden_window_class).await,
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

async fn assert_quota_timestamp_validation(
	store: &PostgresStore,
	client: &Client,
	base: &QuotaWindowMutation,
) -> Result<(), Box<dyn Error>> {
	for (index, (observed_at, resets_at, expected_observed_at, expected_resets_at)) in [
		(
			"2026-07-13T07:00:00.123456789Z",
			Some("2026-07-13T09:30:00.25+02:30"),
			"2026-07-13T07:00:00.123457Z",
			Some("2026-07-13T07:00:00.250000Z"),
		),
		("2026-07-13T01:30:00-05:30", None, "2026-07-13T07:00:00.000000Z", None),
	]
	.into_iter()
	.enumerate()
	{
		let mut valid = base.clone();

		valid.duration_seconds = 301 + i64::try_from(index)?;
		valid.observed_at = observed_at.into();
		valid.resets_at = resets_at.map(str::to_owned);

		let key = format!("window-valid-timestamp-{index}");
		let command = CommandIdentity::new(&key, key.as_bytes())?;
		let stored = store.mutate_quota_window(&command, &valid).await?;

		assert_eq!(stored.observed_at, expected_observed_at);
		assert_eq!(stored.resets_at.as_deref(), expected_resets_at);

		let duplicate = store.mutate_quota_window(&command, &valid).await?;

		assert_eq!(duplicate, stored);

		let persisted_row = client
			.query_one(
				"SELECT decodex.rfc3339_utc(observed_at), \
				 CASE WHEN resets_at IS NULL THEN NULL ELSE decodex.rfc3339_utc(resets_at) END \
				 FROM decodex.quota_windows WHERE account_id = $1::text::uuid \
				 AND window_class = $2 AND duration_seconds = $3",
				&[&ACCOUNT_ID, &valid.window_class, &valid.duration_seconds],
			)
			.await?;
		let persisted: (String, Option<String>) = (persisted_row.get(0), persisted_row.get(1));
		let receipt: Value = client
			.query_one(
				"SELECT response FROM decodex.command_receipts WHERE idempotency_key = $1",
				&[&key],
			)
			.await?
			.get(0);

		assert_eq!(persisted.0, expected_observed_at);
		assert_eq!(persisted.1.as_deref(), expected_resets_at);
		assert_eq!(receipt["observed_at"], expected_observed_at);
		assert_eq!(receipt.get("resets_at").and_then(Value::as_str), expected_resets_at);
	}
	for (index, (observed_at, resets_at)) in [
		("infinity", None),
		("tomorrow", None),
		("2026-07-13 07:00:00Z", None),
		("2026-07-13T07:00:00", None),
		("2026-07-13", None),
		("0000-01-01T00:00:00Z", None),
		("2026-07-13T07:00:00Z", Some("infinity")),
		("2026-07-13T07:00:00Z", Some("next monday")),
	]
	.into_iter()
	.enumerate()
	{
		let mut invalid = base.clone();

		invalid.duration_seconds = 400 + i64::try_from(index)?;
		invalid.observed_at = observed_at.into();
		invalid.resets_at = resets_at.map(str::to_owned);

		let key = format!("window-invalid-timestamp-{index}");
		let command = CommandIdentity::new(&key, key.as_bytes())?;
		let result = store.mutate_quota_window(&command, &invalid).await;

		assert!(
			matches!(&result, Err(StoreError::InvalidInput(_))),
			"invalid timestamp vector {index} was accepted: {result:?}"
		);

		let receipt_count: i64 = client
			.query_one(
				"SELECT count(*) FROM decodex.command_receipts WHERE idempotency_key = $1",
				&[&key],
			)
			.await?
			.get(0);

		assert_eq!(receipt_count, 0);
	}
	for statement in [
		"UPDATE decodex.quota_windows SET observed_at = 'infinity' \
		 WHERE account_id = '10000000-0000-0000-0000-000000000001' \
		 AND window_class = 'usage' AND duration_seconds = 300",
		"UPDATE decodex.quota_windows SET resets_at = '-infinity' \
		 WHERE account_id = '10000000-0000-0000-0000-000000000001' \
		 AND window_class = 'usage' AND duration_seconds = 300",
		"UPDATE decodex.quota_windows SET updated_at = 'infinity' \
		 WHERE account_id = '10000000-0000-0000-0000-000000000001' \
		 AND window_class = 'usage' AND duration_seconds = 300",
	] {
		let error =
			client.execute(statement, &[]).await.expect_err("infinite quota timestamp rejected");

		assert_eq!(
			error.as_db_error().and_then(|error| error.constraint()),
			Some("quota_windows_finite_timestamps")
		);
	}

	Ok(())
}

async fn assert_direct_credential_and_scope_boundary(
	store: &PostgresStore,
	client: &Client,
) -> Result<(), Box<dyn Error>> {
	let rejected_receipts: i64 = client
		.query_one(
			"SELECT count(*) FROM decodex.command_receipts \
			 WHERE idempotency_key LIKE '%credential%'",
			&[],
		)
		.await?
		.get(0);

	assert_eq!(rejected_receipts, 0);

	assert_credential_constraint(
		client,
		"INSERT INTO decodex.outbox \
			 (effect_key, aggregate_kind, aggregate_id, aggregate_revision, payload) \
			 VALUES ('forbidden', 'account', $1, 1, '{\"access_token\": \"forbidden\"}')",
		&[&ACCOUNT_ID],
		"outbox_no_credentials",
	)
	.await?;

	for (index, candidate) in CREDENTIAL_VALUE_VECTORS.iter().enumerate() {
		let aggregate_id = format!("credential-vector-{index}");
		let error = client
			.execute(
				"INSERT INTO decodex.activity \
				 (aggregate_kind, aggregate_id, revision, event_kind, correlation_key, payload) \
				 VALUES ('account', $1, 1, 'credential_vector_tested', $2, '{}')",
				&[&aggregate_id, candidate],
			)
			.await
			.expect_err("SQL credential boundary rejected the shared Rust vector");

		assert_eq!(
			error.as_db_error().and_then(|error| error.constraint()),
			Some("activity_no_credentials"),
			"candidate {candidate:?}",
		);
	}

	assert_direct_delivered_invariant(store, client).await?;
	assert_direct_credential_rows(client).await?;

	assert_no_credential_columns_or_routing(client).await
}

async fn assert_direct_credential_rows(client: &Client) -> Result<(), Box<dyn Error>> {
	for (statement, constraint) in [
		(
			"INSERT INTO decodex.accounts (account_id, display_label) VALUES ('10000000-0000-0000-0000-000000000099', 'Bearer abcdefghijklmnop')",
			"accounts_no_credentials",
		),
		(
			"INSERT INTO decodex.quota_windows (account_id, window_class, duration_seconds, observed_at, confidence) VALUES ('10000000-0000-0000-0000-000000000001', 'sk_proj_0123456789', 60, clock_timestamp(), 1)",
			"quota_windows_no_credentials",
		),
		(
			"INSERT INTO decodex.command_receipts (idempotency_key, request_hash) VALUES ('Basic dXNlcjpwYXNz', repeat('c', 64))",
			"command_receipts_no_credentials",
		),
		(
			"INSERT INTO decodex.activity (aggregate_kind, aggregate_id, revision, event_kind, correlation_key, payload) VALUES ('account', 'credential-test', 1, 'tested', 'credential-test', '{\"nested\":[{\"sessionToken\":\"forbidden\"}]}')",
			"activity_no_credentials",
		),
		(
			"INSERT INTO decodex.activity (aggregate_kind, aggregate_id, revision, event_kind, correlation_key, payload) VALUES ('account', 'credential-value-test', 1, 'tested', 'credential-value-test', '{\"header\":\"Basic dXNlcjpwYXNz\"}')",
			"activity_no_credentials",
		),
		(
			"INSERT INTO decodex.activity (aggregate_kind, aggregate_id, revision, event_kind, correlation_key, payload) VALUES ('account', 'Bearer abcdefghijklmnop', 1, 'tested', 'ordinary', '{}')",
			"activity_no_credentials",
		),
		(
			"INSERT INTO decodex.activity (aggregate_kind, aggregate_id, revision, event_kind, correlation_key, payload) VALUES ('account', 'ordinary', 1, 'tested', 'sk-proj-0123456789', '{}')",
			"activity_no_credentials",
		),
		(
			"INSERT INTO decodex.command_receipts (idempotency_key, request_hash, response, completed_at) VALUES ('direct-credential', repeat('a', 64), '{\"Authorization\":\"forbidden\"}', clock_timestamp())",
			"command_receipts_no_credentials",
		),
		(
			"INSERT INTO decodex.command_receipts (idempotency_key, request_hash, response, completed_at) VALUES ('direct-value-credential', repeat('b', 64), '{\"value\":\"xoxb-1234567890-abcdef\"}', clock_timestamp())",
			"command_receipts_no_credentials",
		),
		(
			"INSERT INTO decodex.leases (resource_key, holder_id, expires_at) VALUES ('Basic dXNlcjpwYXNz', '20000000-0000-0000-0000-000000000099', clock_timestamp() + interval '1 minute')",
			"leases_no_credentials",
		),
		(
			"INSERT INTO decodex.outbox (effect_key, aggregate_kind, aggregate_id, aggregate_revision, payload, effect_state, receipt, reconciliation) VALUES ('forbidden-evidence', 'account', 'credential-test', 1, '{}', 'receipt_recorded', '{\"bearer\":\"forbidden\"}', '{\"api-key\":\"forbidden\"}')",
			"outbox_no_credentials",
		),
		(
			"INSERT INTO decodex.outbox (effect_key, aggregate_kind, aggregate_id, aggregate_revision, payload, effect_state, receipt, reconciliation) VALUES ('forbidden-value-evidence', 'account', 'credential-test', 1, '{}', 'receipt_recorded', '{\"value\":\"glpat-1234567890abcdef\"}', '{\"value\":\"npm_1234567890abcdef\"}')",
			"outbox_no_credentials",
		),
		(
			"INSERT INTO decodex.outbox (effect_key, aggregate_kind, aggregate_id, aggregate_revision, payload) VALUES ('Bearer abcdefghijklmnop', 'account', 'ordinary', 1, '{}')",
			"outbox_no_credentials",
		),
		(
			"INSERT INTO decodex.outbox (effect_key, aggregate_kind, aggregate_id, aggregate_revision, payload, last_failure_code) VALUES ('ordinary-effect', 'account', 'ordinary', 1, '{}', 'sk_proj_0123456789')",
			"outbox_no_credentials",
		),
	] {
		assert_credential_constraint(client, statement, &[], constraint).await?;
	}

	client
		.batch_execute(
			"INSERT INTO decodex.activity \
			 (aggregate_kind, aggregate_id, revision, event_kind, correlation_key, payload) \
			 VALUES ('token_budget', 'session_id', 1, 'session_id', 'token_budget', '{}')",
		)
		.await?;

	Ok(())
}

async fn assert_no_credential_columns_or_routing(client: &Client) -> Result<(), Box<dyn Error>> {
	let forbidden_columns: i64 = client
		.query_one(
			"SELECT count(*) FROM information_schema.columns \
			 WHERE table_schema = 'decodex' AND table_name IN ('accounts', 'quota_windows') \
			 AND lower(column_name) IN \
			 ('credential', 'credentials', 'password', 'private_key', 'secret', \
			  'access_token', 'refresh_token', 'api_key')",
			&[],
		)
		.await?
		.get(0);
	let routing_functions: i64 = client
		.query_one(
			"SELECT count(*) FROM pg_proc JOIN pg_namespace ON pg_namespace.oid = pronamespace \
			 WHERE nspname = 'decodex' AND (proname LIKE '%eligible%' OR proname LIKE '%route%' \
			 OR proname LIKE '%select_account%')",
			&[],
		)
		.await?
		.get(0);

	assert_eq!(forbidden_columns, 0);
	assert_eq!(routing_functions, 0);

	Ok(())
}

async fn assert_direct_delivered_invariant(
	store: &PostgresStore,
	client: &Client,
) -> Result<(), Box<dyn Error>> {
	assert_invalid_delivered_evidence(client).await?;
	assert_unicode_whitespace_evidence(client).await?;
	assert_delivered_retention(client).await?;
	assert_delivered_is_terminal(store, client).await?;

	let delivered_rows: i64 = client
		.query_one(
			"SELECT count(*) FROM decodex.outbox WHERE effect_key LIKE 'direct-delivered-%'",
			&[],
		)
		.await?
		.get(0);

	assert_eq!(delivered_rows, 0);

	Ok(())
}

async fn assert_invalid_delivered_evidence(client: &Client) -> Result<(), Box<dyn Error>> {
	let invalid_evidence = [
		("'receipt_recorded'", "NULL", "'{\"observed\":true}'"),
		("'receipt_recorded'", "'null'", "'{\"observed\":true}'"),
		("'receipt_recorded'", "'\"   \"'", "'{\"observed\":true}'"),
		("'receipt_recorded'", "'{}'", "'{\"observed\":true}'"),
		("'receipt_recorded'", "'[]'", "'{\"observed\":true}'"),
		("'receipt_recorded'", "'{\"nested\":{}}'", "'{\"observed\":true}'"),
		("'ambiguous'", "NULL", "'{\"observed\":true}'"),
		("'receipt_recorded'", "'{\"provider_receipt\":\"receipt\"}'", "NULL"),
		("'receipt_recorded'", "'{\"provider_receipt\":\"receipt\"}'", "'null'"),
		("'receipt_recorded'", "'{\"provider_receipt\":\"receipt\"}'", "'{}'"),
		("'receipt_recorded'", "'{\"provider_receipt\":\"receipt\"}'", "'[]'"),
		("'receipt_recorded'", "'{\"provider_receipt\":\"receipt\"}'", "'{\"nested\":[]}'"),
		(
			"'receipt_recorded'",
			"'{\"provider_receipt\":\"receipt\"}'",
			"'{\"Authorization\":\"forbidden\"}'",
		),
		(
			"'receipt_recorded'",
			"'{\"value\":\"Bearer abcdefghijklmnop\"}'",
			"'{\"observed\":true}'",
		),
	];

	for (index, (effect_state, receipt, reconciliation)) in invalid_evidence.into_iter().enumerate()
	{
		let statement = format!(
			"INSERT INTO decodex.outbox \
			 (effect_key, aggregate_kind, aggregate_id, aggregate_revision, payload, state, \
			  effect_state, receipt, reconciliation, created_at, delivered_at, retain_until) \
			 VALUES ('direct-delivered-{index}', 'account', 'direct-delivered', 1, '{{}}', \
			  'delivered', {effect_state}, {receipt}, {reconciliation}, \
			  statement_timestamp(), statement_timestamp(), \
			  statement_timestamp() + interval '1 day')"
		);
		let error = client
			.execute(&statement, &[])
			.await
			.expect_err("delivered outbox evidence invariant rejected direct SQL bypass");

		assert_eq!(
			error.as_db_error().map(|error| error.code()),
			Some(&tokio_postgres::error::SqlState::CHECK_VIOLATION),
		);
	}

	Ok(())
}

async fn assert_unicode_whitespace_evidence(client: &Client) -> Result<(), Box<dyn Error>> {
	for (index, whitespace) in UNICODE_WHITESPACE_VECTORS.iter().enumerate() {
		let whitespace_evidence = Value::String((*whitespace).into());
		let meaningful_receipt = serde_json::json!({"provider_receipt": "receipt"});
		let meaningful_reconciliation = serde_json::json!({"observed": true});

		for (suffix, receipt, reconciliation) in [
			("receipt", &whitespace_evidence, &meaningful_reconciliation),
			("reconciliation", &meaningful_receipt, &whitespace_evidence),
		] {
			let effect_key = format!("direct-unicode-{index}-{suffix}");
			let error = client
				.execute(
					"INSERT INTO decodex.outbox \
					 (effect_key, aggregate_kind, aggregate_id, aggregate_revision, payload, state, \
					  effect_state, receipt, reconciliation, created_at, delivered_at, retain_until) \
					 VALUES ($1, 'account', 'unicode-whitespace', 1, '{}', 'delivered', \
					  'receipt_recorded', $2, $3, statement_timestamp(), statement_timestamp(), \
					  statement_timestamp() + interval '1 day')",
					&[&effect_key, receipt, reconciliation],
				)
				.await
				.expect_err("Unicode-whitespace-only evidence rejected");

			assert_eq!(
				error.as_db_error().map(|error| error.code()),
				Some(&tokio_postgres::error::SqlState::CHECK_VIOLATION)
			);
		}
	}

	Ok(())
}

async fn assert_delivered_retention(client: &Client) -> Result<(), Box<dyn Error>> {
	for (index, retain_until) in [
		"statement_timestamp()",
		"statement_timestamp() + interval '0.0005 seconds'",
		"statement_timestamp() + 31622400000 * interval '1 millisecond'",
		"'infinity'::timestamptz",
	]
	.into_iter()
	.enumerate()
	{
		let statement = format!(
			"INSERT INTO decodex.outbox \
			 (effect_key, aggregate_kind, aggregate_id, aggregate_revision, payload, state, \
			  effect_state, receipt, reconciliation, created_at, delivered_at, retain_until) \
			 VALUES ('direct-retention-{index}', 'account', 'retention', 1, '{{}}', 'delivered', \
			  'receipt_recorded', '{{\"provider_receipt\":\"receipt\"}}', '{{\"observed\":true}}', \
			  statement_timestamp(), statement_timestamp(), {retain_until})"
		);
		let error = client
			.execute(&statement, &[])
			.await
			.expect_err("invalid delivered retention rejected");

		assert_eq!(
			error.as_db_error().map(|error| error.code()),
			Some(&tokio_postgres::error::SqlState::CHECK_VIOLATION)
		);
	}

	let chronology_error = client
		.execute(
			"INSERT INTO decodex.outbox \
			 (effect_key, aggregate_kind, aggregate_id, aggregate_revision, payload, state, \
			  effect_state, receipt, reconciliation, created_at, delivered_at, retain_until) \
			 VALUES ('direct-retention-chronology', 'account', 'retention', 1, '{}', 'delivered', \
			  'receipt_recorded', '{\"provider_receipt\":\"receipt\"}', '{\"observed\":true}', \
			  statement_timestamp(), statement_timestamp() - interval '1 millisecond', \
			  statement_timestamp() + interval '1 day')",
			&[],
		)
		.await
		.expect_err("delivered timestamp cannot predate row creation");

	assert_eq!(
		chronology_error.as_db_error().and_then(|error| error.constraint()),
		Some("outbox_terminal_chronology")
	);

	let shifted_anchor_error = client
		.execute(
			"INSERT INTO decodex.outbox \
			 (effect_key, aggregate_kind, aggregate_id, aggregate_revision, payload, state, \
			  effect_state, receipt, reconciliation, created_at, delivered_at, retain_until) \
			 VALUES ('direct-retention-shifted-anchor', 'account', 'retention', 1, '{}', \
			  'delivered', 'receipt_recorded', '{\"provider_receipt\":\"receipt\"}', \
			  '{\"observed\":true}', statement_timestamp(), \
			  statement_timestamp() + interval '1000 days', \
			  statement_timestamp() + interval '1000 days 1 millisecond')",
			&[],
		)
		.await
		.expect_err("future-shifted retention anchor rejected");

	assert_eq!(
		shifted_anchor_error.as_db_error().and_then(|error| error.constraint()),
		Some("outbox_operation_time")
	);

	let shifted_retry_error = client
		.execute(
			"INSERT INTO decodex.outbox \
			 (effect_key, aggregate_kind, aggregate_id, aggregate_revision, payload, available_at) \
			 VALUES ('direct-outbox-retry-shifted', 'account', 'retry', 1, '{}', \
			  statement_timestamp() + interval '1000 days')",
			&[],
		)
		.await
		.expect_err("future-shifted direct retry schedule rejected");

	assert_eq!(
		shifted_retry_error.as_db_error().and_then(|error| error.constraint()),
		Some("outbox_operation_time")
	);

	client
		.batch_execute(
			"BEGIN; \
			 INSERT INTO decodex.outbox \
			 (effect_key, aggregate_kind, aggregate_id, aggregate_revision, payload, state, \
			  effect_state, receipt, reconciliation, created_at, delivered_at, retain_until) \
			 VALUES ('direct-retention-valid', 'account', 'retention', 1, '{}', 'delivered', \
			  'receipt_recorded', '{\"provider_receipt\":\"receipt\"}', '{\"observed\":true}', \
			  statement_timestamp(), statement_timestamp(), \
			  statement_timestamp() + 31536000000 * interval '1 millisecond'); \
			 ROLLBACK",
		)
		.await?;

	Ok(())
}

async fn assert_delivered_is_terminal(
	store: &PostgresStore,
	client: &Client,
) -> Result<(), Box<dyn Error>> {
	client
		.batch_execute(
			"INSERT INTO decodex.outbox \
			 (effect_key, aggregate_kind, aggregate_id, aggregate_revision, payload, state, \
			  effect_state, receipt, reconciliation, available_at, created_at, delivered_at, \
			  retain_until) \
			 VALUES ('terminal-retention-guard', 'account', 'terminal', 1, '{}', 'delivered', \
			  'receipt_recorded', '{\"provider_receipt\":\"receipt\"}', '{\"observed\":true}', \
			  '1970-01-01T00:00:00Z', statement_timestamp(), statement_timestamp(), \
			  statement_timestamp() + interval '1 day')",
		)
		.await?;

	let error = client
		.execute(
			"UPDATE decodex.outbox SET state = 'pending', effect_state = 'not_started', \
			 receipt = NULL, reconciliation = NULL, delivered_at = NULL, retain_until = NULL \
			 WHERE effect_key = 'terminal-retention-guard'",
			&[],
		)
		.await
		.expect_err("delivered outbox row cannot regress to replayable state");

	assert_eq!(
		error.as_db_error().map(|error| error.code()),
		Some(&tokio_postgres::error::SqlState::OBJECT_NOT_IN_PREREQUISITE_STATE)
	);

	let early_delete_error = client
		.execute("DELETE FROM decodex.outbox WHERE effect_key = 'terminal-retention-guard'", &[])
		.await
		.expect_err("delivered outbox row cannot be deleted before retention is due");

	assert_eq!(
		early_delete_error.as_db_error().and_then(|error| error.constraint()),
		Some("outbox_retention_pruning_only")
	);

	let reinsert_error = client
		.execute(
			"INSERT INTO decodex.outbox \
			 (effect_key, aggregate_kind, aggregate_id, aggregate_revision, payload) \
			 VALUES ('terminal-retention-guard', 'account', 'terminal', 2, '{}')",
			&[],
		)
		.await
		.expect_err("early delete cannot release the effect key for replay");

	assert_eq!(
		reinsert_error.as_db_error().map(|error| error.code()),
		Some(&tokio_postgres::error::SqlState::UNIQUE_VIOLATION)
	);

	let claims = store.claim_outbox(WORKER_B, 1_000, Duration::from_millis(1)).await?;

	assert!(claims.iter().all(|claim| claim.effect_key != "terminal-retention-guard"));

	let state: String = client
		.query_one(
			"SELECT state::text FROM decodex.outbox \
			 WHERE effect_key = 'terminal-retention-guard'",
			&[],
		)
		.await?
		.get(0);

	assert_eq!(state, "delivered");

	client
		.batch_execute(
			"UPDATE decodex.outbox SET state = 'pending', lease_holder = NULL, claim_token = NULL, \
			 lease_acquired_at = NULL, lease_expires_at = NULL \
			 WHERE state = 'in_flight' AND lease_holder = \
			 '30000000-0000-0000-0000-000000000002'; \
			 INSERT INTO decodex.outbox \
			 (effect_key, aggregate_kind, aggregate_id, aggregate_revision, payload, state, \
			  effect_state, receipt, reconciliation, created_at, delivered_at, retain_until) \
			 VALUES ('direct-delivered-prunable', 'account', 'terminal', 1, '{}', 'delivered', \
			  'receipt_recorded', '{\"provider_receipt\":\"receipt\"}', '{\"observed\":true}', \
			  statement_timestamp() - interval '2 days', \
			  statement_timestamp() - interval '2 days', \
			  statement_timestamp() - interval '1 day'); \
			 DELETE FROM decodex.outbox WHERE effect_key = 'direct-delivered-prunable'",
		)
		.await?;

	Ok(())
}

async fn assert_credential_constraint(
	client: &Client,
	statement: &str,
	parameters: &[&(dyn ToSql + Sync)],
	constraint: &str,
) -> Result<(), Box<dyn Error>> {
	let error = client.execute(statement, parameters).await.expect_err("credential row rejected");

	assert_eq!(error.as_db_error().and_then(|error| error.constraint()), Some(constraint));

	Ok(())
}

async fn assert_lease_contention_and_reclaim(store: &PostgresStore) -> Result<(), Box<dyn Error>> {
	let mut tasks = JoinSet::new();

	for contender in 0..32 {
		let store = store.clone();
		let holder = format!("40000000-0000-0000-0000-{contender:012}");

		tasks.spawn(async move {
			store.try_acquire_lease("managed-run/one", &holder, Duration::from_secs(1)).await
		});
	}

	let mut winners = 0;

	while let Some(result) = tasks.join_next().await {
		if result??.acquired {
			winners += 1;
		}
	}

	assert_eq!(winners, 1);

	time::sleep(Duration::from_millis(1_100)).await;

	let reclaimed =
		store.try_acquire_lease("managed-run/one", HOLDER_A, Duration::from_secs(1)).await?;

	assert!(reclaimed.acquired);
	assert!(reclaimed.revision.is_some_and(|revision| revision >= 2));
	assert!(matches!(
		store
			.renew_lease(
				"managed-run/one",
				HOLDER_B,
				reclaimed.token.as_deref().expect("lease token"),
				Duration::from_secs(1),
			)
			.await,
		Err(StoreError::OwnershipLost("lease"))
	));

	store
		.release_lease(
			"managed-run/one",
			HOLDER_A,
			reclaimed.token.as_deref().expect("lease token"),
		)
		.await?;

	let reacquired =
		store.try_acquire_lease("managed-run/one", HOLDER_B, Duration::from_secs(1)).await?;

	assert!(reacquired.acquired);
	assert!(reacquired.revision > reclaimed.revision);

	Ok(())
}

async fn assert_duration_validation(
	store: &PostgresStore,
	client: &Client,
) -> Result<(), Box<dyn Error>> {
	const INVALID_DURATION: &str =
		"duration must be a positive whole number of milliseconds no greater than 365 days";

	let overflow = Duration::from_millis(MAX_OPERATION_DURATION_MILLISECONDS + 1);
	let huge = Duration::MAX;
	let boundary = i64::try_from(MAX_OPERATION_DURATION_MILLISECONDS)?;
	let interval_is_finite: bool = client
		.query_one("SELECT isfinite($1::bigint * interval '1 millisecond')", &[&boundary])
		.await?
		.get(0);

	assert!(interval_is_finite);

	let boundary_claim = store
		.try_acquire_lease(
			"duration/boundary",
			HOLDER_A,
			Duration::from_millis(MAX_OPERATION_DURATION_MILLISECONDS),
		)
		.await?;

	assert!(boundary_claim.acquired);

	let boundary_token =
		boundary_claim.token.as_deref().expect("acquired boundary lease has token");

	store
		.renew_lease(
			"duration/boundary",
			HOLDER_A,
			boundary_token,
			Duration::from_millis(MAX_OPERATION_DURATION_MILLISECONDS),
		)
		.await?;
	store.release_lease("duration/boundary", HOLDER_A, boundary_token).await?;

	assert_direct_lease_duration_boundary(client).await?;
	assert_direct_outbox_lease_duration_boundary(client).await?;

	for duration in
		[Duration::ZERO, Duration::from_nanos(1), Duration::from_micros(1_500), overflow, huge]
	{
		assert!(matches!(
			store.try_acquire_lease("duration/lease", HOLDER_A, duration).await,
			Err(StoreError::InvalidInput(INVALID_DURATION))
		));
		assert!(matches!(
			store.claim_outbox(WORKER_A, 1, duration).await,
			Err(StoreError::InvalidInput(INVALID_DURATION))
		));
		assert!(matches!(
			store.renew_outbox_claim(0, WORKER_A, WORKER_A, duration).await,
			Err(StoreError::InvalidInput(INVALID_DURATION))
		));
		assert!(matches!(
			store
				.retry_outbox_before_effect(0, WORKER_A, WORKER_A, "temporary_failure", duration)
				.await,
			Err(StoreError::InvalidInput(INVALID_DURATION))
		));
		assert!(matches!(
			store
				.reconcile_outbox(
					0,
					WORKER_A,
					WORKER_A,
					&OutboxReconciliation {
						readback: serde_json::json!({"observed": false}),
						outcome: ReconciliationOutcome::EffectAbsent,
					},
					duration,
					Duration::from_millis(1),
				)
				.await,
			Err(StoreError::InvalidInput(INVALID_DURATION))
		));
		assert!(matches!(
			store
				.reconcile_outbox(
					0,
					WORKER_A,
					WORKER_A,
					&OutboxReconciliation {
						readback: serde_json::json!({"observed": false}),
						outcome: ReconciliationOutcome::EffectAbsent,
					},
					Duration::from_millis(1),
					duration,
				)
				.await,
			Err(StoreError::InvalidInput(INVALID_DURATION))
		));
	}

	let valid = store
		.try_acquire_lease("session_id/token_budget", HOLDER_A, Duration::from_millis(1))
		.await?;

	assert!(valid.acquired);

	Ok(())
}

async fn assert_direct_lease_duration_boundary(client: &Client) -> Result<(), Box<dyn Error>> {
	let direct_token: String = client
		.query_one(
			"SELECT lease_token::text FROM decodex.try_acquire_lease( \
			 'duration/direct', $1::text::uuid, interval '365 days') WHERE acquired",
			&[&HOLDER_A],
		)
		.await?
		.get(0);

	for statement in [
		"SELECT * FROM decodex.try_acquire_lease( \
		 'duration/direct-overflow', '20000000-0000-0000-0000-000000000001', interval '366 days')",
		"SELECT * FROM decodex.try_acquire_lease( \
		 'duration/direct-fractional', '20000000-0000-0000-0000-000000000001', interval '0.0005 seconds')",
		"SELECT * FROM decodex.try_acquire_lease( \
		 'duration/direct-month', '20000000-0000-0000-0000-000000000001', interval '1 month')",
	] {
		let error =
			client.execute(statement, &[]).await.expect_err("invalid direct lease TTL rejected");

		assert_eq!(
			error.as_db_error().map(|error| error.code()),
			Some(&tokio_postgres::error::SqlState::INVALID_PARAMETER_VALUE)
		);
	}
	for ttl in ["interval '366 days'", "interval '0.0005 seconds'"] {
		let statement = format!(
			"SELECT decodex.renew_lease( \
			 'duration/direct', '20000000-0000-0000-0000-000000000001', \
			 '{direct_token}', {ttl})"
		);
		let error =
			client.execute(&statement, &[]).await.expect_err("invalid direct renewal rejected");

		assert_eq!(
			error.as_db_error().map(|error| error.code()),
			Some(&tokio_postgres::error::SqlState::INVALID_PARAMETER_VALUE)
		);
	}
	for statement in [
		"INSERT INTO decodex.leases (resource_key, holder_id, expires_at, updated_at) \
			 VALUES ('duration/table-overflow', '20000000-0000-0000-0000-000000000001', \
			 statement_timestamp() + 31622400000 * interval '1 millisecond', \
			 statement_timestamp())",
		"INSERT INTO decodex.leases (resource_key, holder_id, expires_at, updated_at) \
			 VALUES ('duration/table-fractional', '20000000-0000-0000-0000-000000000001', \
			 statement_timestamp() + interval '0.0005 seconds', statement_timestamp())",
		"INSERT INTO decodex.leases (resource_key, holder_id, expires_at, updated_at) \
			 VALUES ('duration/table-infinity', '20000000-0000-0000-0000-000000000001', \
			 'infinity', statement_timestamp())",
	] {
		let error =
			client.execute(statement, &[]).await.expect_err("invalid direct lease row rejected");

		assert_eq!(
			error.as_db_error().map(|error| error.code()),
			Some(&tokio_postgres::error::SqlState::CHECK_VIOLATION)
		);
	}

	let shifted_anchor_error = client
		.execute(
			"INSERT INTO decodex.leases (resource_key, holder_id, expires_at, updated_at) \
			 VALUES ('duration/table-shifted-anchor', \
			 '20000000-0000-0000-0000-000000000001', \
			 statement_timestamp() + interval '1000 days 1 millisecond', \
			 statement_timestamp() + interval '1000 days')",
			&[],
		)
		.await
		.expect_err("future-shifted direct lease anchor rejected");

	assert_eq!(
		shifted_anchor_error.as_db_error().and_then(|error| error.constraint()),
		Some("leases_operation_time")
	);

	client
		.batch_execute(
			"INSERT INTO decodex.leases (resource_key, holder_id, expires_at, updated_at) \
			 VALUES ('duration/expired-row', '20000000-0000-0000-0000-000000000001', \
			 statement_timestamp() - interval '1 day', statement_timestamp()); \
			 DELETE FROM decodex.leases WHERE resource_key IN ('duration/direct', 'duration/expired-row')",
		)
		.await?;

	Ok(())
}

async fn assert_direct_outbox_lease_duration_boundary(
	client: &Client,
) -> Result<(), Box<dyn Error>> {
	for (index, lease_expires_at) in [
		"statement_timestamp() + interval '0.0005 seconds'",
		"statement_timestamp() + interval '366 days'",
		"'infinity'::timestamptz",
	]
	.into_iter()
	.enumerate()
	{
		let statement = format!(
			"INSERT INTO decodex.outbox \
			 (effect_key, aggregate_kind, aggregate_id, aggregate_revision, payload, state, \
			  lease_holder, claim_token, lease_acquired_at, lease_expires_at, created_at) \
			 VALUES ('direct-outbox-lease-{index}', 'account', 'lease', 1, '{{}}', 'in_flight', \
			  '30000000-0000-0000-0000-000000000001', \
			  '40000000-0000-0000-0000-000000000001', statement_timestamp(), \
			  {lease_expires_at}, statement_timestamp())"
		);
		let error = client
			.execute(&statement, &[])
			.await
			.expect_err("invalid direct outbox lease rejected");

		assert_eq!(
			error.as_db_error().map(|error| error.code()),
			Some(&tokio_postgres::error::SqlState::CHECK_VIOLATION)
		);
	}

	let shifted_anchor_error = client
		.execute(
			"INSERT INTO decodex.outbox \
			 (effect_key, aggregate_kind, aggregate_id, aggregate_revision, payload, state, \
			  lease_holder, claim_token, lease_acquired_at, lease_expires_at, created_at) \
			 VALUES ('direct-outbox-lease-shifted-anchor', 'account', 'lease', 1, '{}', \
			  'in_flight', '30000000-0000-0000-0000-000000000001', \
			  '40000000-0000-0000-0000-000000000001', \
			  statement_timestamp() + interval '1000 days', \
			  statement_timestamp() + interval '1000 days 1 millisecond', \
			  statement_timestamp())",
			&[],
		)
		.await
		.expect_err("future-shifted direct outbox lease anchor rejected");

	assert_eq!(
		shifted_anchor_error.as_db_error().and_then(|error| error.constraint()),
		Some("outbox_operation_time")
	);

	client
		.batch_execute(
			"BEGIN; \
			 INSERT INTO decodex.outbox \
			 (effect_key, aggregate_kind, aggregate_id, aggregate_revision, payload, state, \
			  lease_holder, claim_token, lease_acquired_at, lease_expires_at, created_at) \
			 VALUES ('direct-outbox-lease-valid', 'account', 'lease', 1, '{}', 'in_flight', \
			  '30000000-0000-0000-0000-000000000001', \
			  '40000000-0000-0000-0000-000000000001', statement_timestamp(), \
			  statement_timestamp() + 31536000000 * interval '1 millisecond', \
			  statement_timestamp()); \
			 ROLLBACK",
		)
		.await?;

	Ok(())
}

async fn assert_closed_pool_behavior(store: &PostgresStore) -> Result<(), Box<dyn Error>> {
	assert!(matches!(store.account(&AccountId::new(ACCOUNT_ID)?).await, Err(StoreError::Pool(_))));
	assert!(matches!(store.activity_after(0, 1).await, Err(StoreError::Pool(_))));
	assert!(matches!(
		store
			.try_acquire_lease(
				"closed/boundary-duration",
				HOLDER_A,
				Duration::from_millis(MAX_OPERATION_DURATION_MILLISECONDS),
			)
			.await,
		Err(StoreError::Pool(_))
	));
	assert!(matches!(
		store
			.try_acquire_lease(
				"closed/overflow-duration",
				HOLDER_A,
				Duration::from_millis(MAX_OPERATION_DURATION_MILLISECONDS + 1),
			)
			.await,
		Err(StoreError::InvalidInput(
			"duration must be a positive whole number of milliseconds no greater than 365 days"
		))
	));
	assert!(matches!(
		store.claim_outbox(WORKER_A, 1, Duration::from_millis(1)).await,
		Err(StoreError::Pool(_))
	));
	assert!(matches!(store.prune_delivered_outbox(1).await, Err(StoreError::Pool(_))));

	let invalid_timestamp = QuotaWindowMutation {
		account_id: AccountId::new(ACCOUNT_ID)?,
		window_class: "closed_validation".into(),
		duration_seconds: 1,
		remaining_amount: None,
		resets_at: None,
		observed_at: "infinity".into(),
		confidence: 0.0,
		metadata: serde_json::json!({}),
		expected_revision: None,
	};
	let command = CommandIdentity::new("closed-invalid-timestamp", b"closed-invalid-timestamp")?;

	assert!(matches!(
		store.mutate_quota_window(&command, &invalid_timestamp).await,
		Err(StoreError::InvalidInput("quota observed_at must be RFC 3339"))
	));

	Ok(())
}

async fn assert_outbox_concurrency_retry_and_restart(
	store: &PostgresStore,
	client: &Client,
	config: &Config,
) -> Result<(), Box<dyn Error>> {
	seed_outbox_accounts(store).await?;

	let available: i64 = client
		.query_one("SELECT count(*) FROM decodex.outbox WHERE state = 'pending'", &[])
		.await?
		.get(0);
	let mut tasks = JoinSet::new();

	for worker in 0..8 {
		let store = store.clone();
		let worker_id = format!("60000000-0000-0000-0000-{worker:012}");

		tasks.spawn(
			async move { store.claim_outbox(&worker_id, 200, Duration::from_secs(2)).await },
		);
	}

	let mut claims = Vec::new();

	while let Some(result) = tasks.join_next().await {
		claims.extend(result??);
	}

	let unique: HashSet<_> = claims.iter().map(|claim| claim.id).collect();

	assert_eq!(claims.len(), usize::try_from(available)?);
	assert_eq!(unique.len(), claims.len());

	client
		.execute(
			"UPDATE decodex.outbox SET state = 'pending', lease_holder = NULL, claim_token = NULL, \
			 lease_acquired_at = NULL, lease_expires_at = NULL WHERE state = 'in_flight'",
			&[],
		)
		.await?;

	assert_outbox_retry_and_restart(store, client, config).await
}

async fn seed_outbox_accounts(store: &PostgresStore) -> Result<(), Box<dyn Error>> {
	for index in 0..96 {
		let account_id = AccountId::new(format!("50000000-0000-0000-0000-{index:012}"))?;
		let mutation = AccountMutation {
			account_id,
			display_label: format!("Synthetic {index}"),
			state: AccountState::Unknown,
			metadata: serde_json::json!({"fixture": index}),
			expected_revision: None,
		};
		let command = CommandIdentity::new(
			format!("bulk-account-{index}"),
			format!("bulk-account-{index}").as_bytes(),
		)?;

		store.mutate_account(&command, &mutation).await?;
	}

	Ok(())
}

async fn assert_outbox_retry_and_restart(
	store: &PostgresStore,
	client: &Client,
	config: &Config,
) -> Result<(), Box<dyn Error>> {
	let retry = store.claim_outbox(WORKER_A, 1, Duration::from_secs(1)).await?.remove(0);

	client
		.execute(
			"UPDATE decodex.outbox SET available_at = clock_timestamp() + interval '1 hour' \
			 WHERE state = 'pending' AND id <> $1",
			&[&retry.id],
		)
		.await?;
	store
		.retry_outbox_before_effect(
			retry.id,
			WORKER_A,
			&retry.claim_token,
			"temporary_failure",
			Duration::from_millis(60),
		)
		.await?;

	assert!(store.claim_outbox(WORKER_B, 1, Duration::from_secs(1)).await?.is_empty());

	time::sleep(Duration::from_millis(80)).await;

	let retry_claim = store.claim_outbox(WORKER_B, 1, Duration::from_secs(1)).await?.remove(0);

	assert_eq!(retry_claim.id, retry.id);
	assert_eq!(retry_claim.attempt_count, retry.attempt_count + 1);

	store
		.retry_outbox_before_effect(
			retry_claim.id,
			WORKER_B,
			&retry_claim.claim_token,
			"fixture_release",
			Duration::from_secs(30),
		)
		.await?;
	client
		.execute(
			"UPDATE decodex.outbox SET available_at = clock_timestamp() \
			 WHERE id = (SELECT min(id) FROM decodex.outbox WHERE state = 'pending' AND id <> $1)",
			&[&retry_claim.id],
		)
		.await?;

	let ambiguous = store.claim_outbox(WORKER_A, 1, Duration::from_millis(40)).await?.remove(0);

	store.begin_outbox_effect(ambiguous.id, WORKER_A, &ambiguous.claim_token).await?;
	store.close();

	assert_restart_reconciliation(client, config, &ambiguous).await
}

async fn assert_restart_reconciliation(
	client: &Client,
	config: &Config,
	ambiguous: &OutboxClaim,
) -> Result<(), Box<dyn Error>> {
	time::sleep(Duration::from_millis(60)).await;

	let restarted = PostgresStore::connect(config.clone()).await?;
	let recovered = restarted.claim_outbox(WORKER_A, 1, Duration::from_secs(1)).await?.remove(0);

	assert_eq!(recovered.id, ambiguous.id);
	assert!(recovered.requires_reconciliation);
	assert_ne!(recovered.claim_token, ambiguous.claim_token);

	assert_stale_outbox_claim_rejected(&restarted, ambiguous, &recovered).await?;

	let receiptless_state: (String, String, Option<Value>) = {
		let row = client
			.query_one(
				"SELECT state::text, effect_state::text, receipt FROM decodex.outbox WHERE id = $1",
				&[&recovered.id],
			)
			.await?;

		(row.get(0), row.get(1), row.get(2))
	};

	assert_eq!(receiptless_state, ("in_flight".into(), "ambiguous".into(), None));

	restarted
		.record_outbox_receipt(
			recovered.id,
			WORKER_A,
			&recovered.claim_token,
			&serde_json::json!({"provider_receipt": "receipt-1"}),
		)
		.await?;

	assert_invalid_reconciliation_evidence(&restarted, &recovered).await?;

	restarted
		.reconcile_outbox(
			recovered.id,
			WORKER_A,
			&recovered.claim_token,
			&OutboxReconciliation {
				readback: serde_json::json!({"effect_key": recovered.effect_key, "observed": true}),
				outcome: ReconciliationOutcome::EffectPresent,
			},
			Duration::from_millis(1),
			Duration::from_millis(1),
		)
		.await?;

	let delivered: (String, String, Value, Value) = {
		let row = client
			.query_one(
				"SELECT state::text, effect_state::text, receipt, reconciliation \
				 FROM decodex.outbox WHERE id = $1",
				&[&recovered.id],
			)
			.await?;

		(row.get(0), row.get(1), row.get(2), row.get(3))
	};

	assert_eq!(delivered.0, "delivered");
	assert_eq!(delivered.1, "receipt_recorded");
	assert_eq!(delivered.2, serde_json::json!({"provider_receipt": "receipt-1"}));
	assert_eq!(
		delivered.3,
		serde_json::json!({"effect_key": recovered.effect_key, "observed": true})
	);

	time::sleep(Duration::from_millis(10)).await;

	assert_eq!(restarted.prune_delivered_outbox(10).await?, 1);

	assert_effect_absent_reconciliation(&restarted, client).await?;

	Ok(())
}

async fn assert_invalid_reconciliation_evidence(
	store: &PostgresStore,
	claim: &OutboxClaim,
) -> Result<(), Box<dyn Error>> {
	for readback in [
		Value::Null,
		serde_json::json!(" \n "),
		serde_json::json!("\u{a0}"),
		serde_json::json!("\u{85}"),
		serde_json::json!("\u{202f}"),
		serde_json::json!("\u{3000}"),
		serde_json::json!({}),
		serde_json::json!([]),
		serde_json::json!({"nested": []}),
	] {
		assert!(matches!(
			store
				.reconcile_outbox(
					claim.id,
					WORKER_A,
					&claim.claim_token,
					&OutboxReconciliation {
						readback,
						outcome: ReconciliationOutcome::EffectPresent,
					},
					Duration::from_millis(1),
					Duration::from_millis(1),
				)
				.await,
			Err(StoreError::InvalidInput("outbox evidence must contain a non-empty JSON value"))
		));
	}

	assert!(matches!(
		store
			.reconcile_outbox(
				claim.id,
				WORKER_A,
				&claim.claim_token,
				&OutboxReconciliation {
					readback: serde_json::json!({"Authorization": "forbidden"}),
					outcome: ReconciliationOutcome::EffectPresent,
				},
				Duration::from_millis(1),
				Duration::from_millis(1),
			)
			.await,
		Err(StoreError::CredentialRejected)
	));

	Ok(())
}

async fn assert_stale_outbox_claim_rejected(
	store: &PostgresStore,
	ambiguous: &OutboxClaim,
	recovered: &OutboxClaim,
) -> Result<(), Box<dyn Error>> {
	assert!(matches!(
		store.begin_outbox_effect(recovered.id, WORKER_A, &ambiguous.claim_token).await,
		Err(StoreError::OwnershipLost("outbox claim"))
	));
	assert!(matches!(
		store
			.renew_outbox_claim(
				recovered.id,
				WORKER_A,
				&ambiguous.claim_token,
				Duration::from_secs(1),
			)
			.await,
		Err(StoreError::OwnershipLost("outbox claim"))
	));
	assert!(matches!(
		store
			.record_outbox_receipt(
				recovered.id,
				WORKER_A,
				&ambiguous.claim_token,
				&serde_json::json!({"provider_receipt": "stale"}),
			)
			.await,
		Err(StoreError::OwnershipLost("outbox claim"))
	));
	assert!(matches!(
		store
			.record_outbox_receipt(
				recovered.id,
				WORKER_A,
				&recovered.claim_token,
				&serde_json::json!({"accessToken": "forbidden"}),
			)
			.await,
		Err(StoreError::CredentialRejected)
	));

	for evidence in [
		Value::Null,
		serde_json::json!(" \t "),
		serde_json::json!("\u{a0}"),
		serde_json::json!("\u{85}"),
		serde_json::json!("\u{202f}"),
		serde_json::json!("\u{3000}"),
		serde_json::json!({}),
		serde_json::json!([]),
		serde_json::json!({"nested": {}}),
	] {
		assert!(matches!(
			store
				.record_outbox_receipt(recovered.id, WORKER_A, &recovered.claim_token, &evidence,)
				.await,
			Err(StoreError::InvalidInput("outbox evidence must contain a non-empty JSON value"))
		));
	}

	assert!(matches!(
		store
			.reconcile_outbox(
				recovered.id,
				WORKER_A,
				&recovered.claim_token,
				&OutboxReconciliation {
					readback: serde_json::json!({"observed": true}),
					outcome: ReconciliationOutcome::EffectPresent,
				},
				Duration::from_millis(1),
				Duration::from_millis(1),
			)
			.await,
		Err(StoreError::OwnershipLost("outbox claim"))
	));

	Ok(())
}

async fn assert_effect_absent_reconciliation(
	store: &PostgresStore,
	client: &Client,
) -> Result<(), Box<dyn Error>> {
	client
		.execute(
			"INSERT INTO decodex.outbox \
			 (effect_key, aggregate_kind, aggregate_id, aggregate_revision, payload) \
			 VALUES ('effect-absent-retry', 'account', $1, 1, '{\"fixture\":\"absent\"}')",
			&[&ACCOUNT_ID],
		)
		.await?;

	let first = store.claim_outbox(WORKER_A, 1, Duration::from_millis(40)).await?.remove(0);

	store.begin_outbox_effect(first.id, WORKER_A, &first.claim_token).await?;

	time::sleep(Duration::from_millis(60)).await;

	let recovered = store.claim_outbox(WORKER_A, 1, Duration::from_secs(1)).await?.remove(0);

	assert_eq!(recovered.id, first.id);
	assert!(recovered.requires_reconciliation);
	assert_ne!(recovered.claim_token, first.claim_token);
	assert!(matches!(
		store
			.reconcile_outbox(
				recovered.id,
				WORKER_A,
				&first.claim_token,
				&OutboxReconciliation {
					readback: serde_json::json!({"observed": false}),
					outcome: ReconciliationOutcome::EffectAbsent,
				},
				Duration::from_millis(1),
				Duration::from_millis(1),
			)
			.await,
		Err(StoreError::OwnershipLost("outbox claim"))
	));
	assert!(matches!(
		store
			.retry_outbox_before_effect(
				recovered.id,
				WORKER_A,
				&recovered.claim_token,
				"blind_replay_forbidden",
				Duration::from_millis(1),
			)
			.await,
		Err(StoreError::OwnershipLost("outbox claim"))
	));

	store
		.reconcile_outbox(
			recovered.id,
			WORKER_A,
			&recovered.claim_token,
			&OutboxReconciliation {
				readback: serde_json::json!({"observed": false}),
				outcome: ReconciliationOutcome::EffectAbsent,
			},
			Duration::from_millis(60),
			Duration::from_millis(1),
		)
		.await?;

	assert!(store.claim_outbox(WORKER_B, 1, Duration::from_secs(1)).await?.is_empty());

	time::sleep(Duration::from_millis(80)).await;

	let retry = store.claim_outbox(WORKER_B, 1, Duration::from_secs(1)).await?.remove(0);

	assert_eq!(retry.id, recovered.id);
	assert_eq!(retry.attempt_count, recovered.attempt_count + 1);
	assert!(!retry.requires_reconciliation);

	store
		.retry_outbox_before_effect(
			retry.id,
			WORKER_B,
			&retry.claim_token,
			"fixture_release",
			Duration::from_secs(30),
		)
		.await?;

	assert_effect_absent_dead_letter(store, client).await?;

	Ok(())
}

async fn assert_effect_absent_dead_letter(
	store: &PostgresStore,
	client: &Client,
) -> Result<(), Box<dyn Error>> {
	client
		.execute(
			"INSERT INTO decodex.outbox \
			 (effect_key, aggregate_kind, aggregate_id, aggregate_revision, payload, max_attempts) \
			 VALUES ('effect-absent-dead-letter', 'account', $1, 1, \
			         '{\"fixture\":\"dead-letter\"}', 1)",
			&[&ACCOUNT_ID],
		)
		.await?;

	let final_attempt = store.claim_outbox(WORKER_A, 1, Duration::from_secs(1)).await?.remove(0);

	store.begin_outbox_effect(final_attempt.id, WORKER_A, &final_attempt.claim_token).await?;
	store
		.reconcile_outbox(
			final_attempt.id,
			WORKER_A,
			&final_attempt.claim_token,
			&OutboxReconciliation {
				readback: serde_json::json!({"observed": false}),
				outcome: ReconciliationOutcome::EffectAbsent,
			},
			Duration::from_millis(1),
			Duration::from_millis(1),
		)
		.await?;

	let state: String = client
		.query_one("SELECT state::text FROM decodex.outbox WHERE id = $1", &[&final_attempt.id])
		.await?
		.get(0);

	assert_eq!(state, "dead_letter");

	Ok(())
}

async fn assert_incompatible_history_fails_closed(
	client: &Client,
	config: &Config,
) -> Result<(), Box<dyn Error>> {
	client
		.execute(
			"UPDATE refinery_schema_history \
			 SET checksum = (checksum::numeric + 1)::text WHERE version = 1",
			&[],
		)
		.await?;

	let incompatible = PostgresStore::connect(config.clone()).await;

	client
		.execute(
			"UPDATE refinery_schema_history \
			 SET checksum = (checksum::numeric - 1)::text WHERE version = 1",
			&[],
		)
		.await?;

	assert!(incompatible.is_err());

	let recovered = PostgresStore::connect(config.clone()).await?;

	recovered.close();

	Ok(())
}

async fn assert_primary_indexes_are_plan_eligible(client: &Client) -> Result<(), Box<dyn Error>> {
	client
		.batch_execute("ANALYZE decodex.activity; ANALYZE decodex.outbox; SET enable_seqscan = off")
		.await?;

	let activity_plan = client
		.query(
			"EXPLAIN (COSTS OFF) SELECT sequence FROM decodex.activity \
			 WHERE aggregate_kind = 'account' AND aggregate_id = $1 \
			 ORDER BY sequence DESC LIMIT 50",
			&[&ACCOUNT_ID],
		)
		.await?
		.into_iter()
		.map(|row| row.get::<_, String>(0))
		.collect::<Vec<_>>()
		.join("\n");
	let outbox_plan = client
		.query(
			"EXPLAIN (COSTS OFF) SELECT id FROM decodex.outbox \
			 WHERE state IN ('pending', 'in_flight') AND available_at <= clock_timestamp() \
			 ORDER BY available_at, id LIMIT 100",
			&[],
		)
		.await?
		.into_iter()
		.map(|row| row.get::<_, String>(0))
		.collect::<Vec<_>>()
		.join("\n");

	client.batch_execute("RESET enable_seqscan").await?;

	assert!(activity_plan.contains("activity_timeline_idx"), "activity plan: {activity_plan}");
	assert!(outbox_plan.contains("outbox_claim_idx"), "outbox plan: {outbox_plan}");

	Ok(())
}
