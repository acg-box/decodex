mod identity;
mod response;
mod validation;

pub use crate::quota::validation::parse_quota_timestamp_rfc3339;

#[cfg(feature = "test-support")] use std::sync::atomic::Ordering;

use deadpool_postgres::{Client, Transaction};

use crate::{
	CommandIdentity, PostgresStore, QuotaExclusionMutation, QuotaExclusionReceipt,
	QuotaTimestampMicros, QuotaWindow, QuotaWindowMutation, StoreError,
	accounts::{self, CommandClaim, CommandDescriptor},
	quota::{identity::CanonicalMutationIdentity, validation::MAXIMUM_OBSERVATION_AGE_MICROS},
};

#[cfg(feature = "test-support")]
static FAIL_EXCLUSION_AFTER_RESERVATION: std::sync::atomic::AtomicBool =
	std::sync::atomic::AtomicBool::new(false);
#[cfg(feature = "test-support")]
static FAIL_QUOTA_UNLOCK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
#[cfg(feature = "test-support")]
static QUOTA_LOCK_EVICTIONS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

struct QuotaWindowLock {
	client: Option<Client>,
	key: String,
	possibly_locked: bool,
}
impl QuotaWindowLock {
	async fn acquire(client: Client, mutation: &QuotaWindowMutation) -> Result<Self, StoreError> {
		let guard = Self {
			client: Some(client),
			key: response::quota_aggregate_id(mutation),
			possibly_locked: true,
		};

		guard
			.client()
			.query_one("SELECT pg_advisory_lock(hashtextextended($1,0))", &[&guard.key])
			.await?;

		Ok(guard)
	}

	fn client(&self) -> &Client {
		self.client.as_ref().expect("quota lock retains its client")
	}

	fn client_mut(&mut self) -> &mut Client {
		self.client.as_mut().expect("quota lock retains its client")
	}

	async fn complete<T>(mut self, operation: Result<T, StoreError>) -> Result<T, StoreError> {
		let cleanup = self.unlock().await;
		let result = operation_result_precedes_cleanup(operation, cleanup);

		drop(self);

		result
	}

	async fn unlock(&mut self) -> Result<(), StoreError> {
		#[cfg(feature = "test-support")]
		if FAIL_QUOTA_UNLOCK.swap(false, Ordering::SeqCst) {
			return Err(StoreError::CapacityExhausted("injected quota unlock failure"));
		}

		let unlocked: bool = self
			.client()
			.query_one("SELECT pg_advisory_unlock(hashtextextended($1,0))", &[&self.key])
			.await?
			.get(0);

		if !unlocked {
			return Err(StoreError::Incompatible(
				"quota window serialization lock was not owned".into(),
			));
		}

		self.possibly_locked = false;

		Ok(())
	}
}

impl Drop for QuotaWindowLock {
	fn drop(&mut self) {
		if self.possibly_locked
			&& let Some(client) = self.client.take()
		{
			#[cfg(feature = "test-support")]
			QUOTA_LOCK_EVICTIONS.fetch_add(1, Ordering::SeqCst);

			drop(Client::take(client));
		}
	}
}

impl PostgresStore {
	/// Inject one synthetic process failure after durable exclusion receipt reservation.
	#[cfg(feature = "test-support")]
	#[doc(hidden)]
	pub fn fail_next_exclusion_after_reservation_fixture() {
		FAIL_EXCLUSION_AFTER_RESERVATION.store(true, Ordering::SeqCst);
	}

	/// Inject one synthetic advisory-unlock failure after an authoritative quota operation.
	#[cfg(feature = "test-support")]
	#[doc(hidden)]
	pub fn fail_next_quota_unlock_fixture() {
		FAIL_QUOTA_UNLOCK.store(true, Ordering::SeqCst);
	}

	/// Return the process-local count of possibly locked quota sessions evicted from the pool.
	#[cfg(feature = "test-support")]
	#[doc(hidden)]
	pub fn quota_lock_evictions_fixture() -> u64 {
		QUOTA_LOCK_EVICTIONS.load(Ordering::SeqCst)
	}

	/// Persist one exact, duration-typed observation without exposing routing authority.
	pub async fn mutate_quota_window(
		&self,
		command: &CommandIdentity,
		mutation: &QuotaWindowMutation,
	) -> Result<QuotaWindow, StoreError> {
		validation::validate_window(mutation)?;

		let identity = identity::quota_window_mutation_identity(mutation)?;
		let derived_command = identity::derived_command(command, &identity);
		let descriptor = identity::command_descriptor("mutate_quota_window", mutation, &identity);
		let mut guard = QuotaWindowLock::acquire(self.pool().get().await?, mutation).await?;
		let result = mutate_quota_window_locked(
			guard.client_mut(),
			command,
			mutation,
			&derived_command,
			&descriptor,
		)
		.await;

		guard.complete(result).await
	}

	/// Commit an exact account/window exclusion before returning inert fallback evidence.
	pub async fn persist_quota_exclusion(
		&self,
		command: &CommandIdentity,
		mutation: &QuotaExclusionMutation,
	) -> Result<QuotaExclusionReceipt, StoreError> {
		validation::validate_exclusion(mutation)?;

		let identity = identity::quota_exclusion_mutation_identity(mutation)?;
		let derived_command = identity::derived_command(command, &identity);
		let descriptor = identity::exclusion_command_descriptor(mutation, &identity);
		let mut guard =
			QuotaWindowLock::acquire(self.pool().get().await?, &mutation.observation).await?;
		let result = persist_quota_exclusion_locked(
			guard.client_mut(),
			command,
			mutation,
			&identity,
			&derived_command,
			&descriptor,
		)
		.await;

		guard.complete(result).await
	}
}

fn operation_result_precedes_cleanup<T>(
	operation: Result<T, StoreError>,
	_cleanup: Result<(), StoreError>,
) -> Result<T, StoreError> {
	operation
}

async fn mutate_quota_window_locked(
	client: &mut Client,
	command: &CommandIdentity,
	mutation: &QuotaWindowMutation,
	derived_command: &CommandIdentity,
	descriptor: &CommandDescriptor,
) -> Result<QuotaWindow, StoreError> {
	if let Some(response) =
		accounts::replay_completed_command(client, derived_command, descriptor).await?
	{
		return response::window_from_response(response);
	}

	preflight_window_write(client, mutation).await?;

	let reservation = match accounts::reserve_command(client, derived_command, descriptor).await? {
		CommandClaim::Completed(response) => return response::window_from_response(response),
		CommandClaim::Owned(reservation) => reservation,
	};
	let transaction = client.transaction().await?;
	let window = persist_window(&transaction, mutation).await?;
	let aggregate_id = response::quota_aggregate_id(mutation);
	let event_kind =
		if window.revision == 1 { "quota_window_created" } else { "quota_window_updated" };
	let payload = response::window_response(&window);

	accounts::append_activity_and_outbox(
		&transaction,
		"quota_window",
		&aggregate_id,
		window.revision,
		event_kind,
		&command.key,
		&payload,
	)
	.await?;
	accounts::finish_command(&transaction, &reservation, &payload).await?;

	transaction.commit().await?;

	Ok(window)
}

async fn persist_quota_exclusion_locked(
	client: &mut Client,
	command: &CommandIdentity,
	mutation: &QuotaExclusionMutation,
	identity: &CanonicalMutationIdentity,
	derived_command: &CommandIdentity,
	descriptor: &CommandDescriptor,
) -> Result<QuotaExclusionReceipt, StoreError> {
	if let Some(response) =
		accounts::replay_completed_command(client, derived_command, descriptor).await?
	{
		return response::exclusion_from_response(response);
	}

	preflight_window_write(client, &mutation.observation).await?;

	let reservation = match accounts::reserve_command(client, derived_command, descriptor).await? {
		CommandClaim::Completed(response) => return response::exclusion_from_response(response),
		CommandClaim::Owned(reservation) => reservation,
	};

	#[cfg(feature = "test-support")]
	if FAIL_EXCLUSION_AFTER_RESERVATION.swap(false, Ordering::SeqCst) {
		return Err(StoreError::CapacityExhausted(
			"injected quota exclusion failure after receipt reservation",
		));
	}

	let transaction = client.transaction().await?;
	let window = persist_window(&transaction, &mutation.observation).await?;
	let inserted = insert_exclusion(&transaction, mutation, &window, identity).await?;
	let receipt = response::exclusion_receipt(mutation, window.revision, identity)?;
	let response = response::exclusion_response(&receipt);

	if inserted {
		accounts::append_activity_and_outbox(
			&transaction,
			"quota_window",
			&response::quota_aggregate_id(&mutation.observation),
			window.revision,
			"quota_window_excluded",
			&command.key,
			&response,
		)
		.await?;
	}

	accounts::finish_command(&transaction, &reservation, &response).await?;

	transaction.commit().await?;

	Ok(receipt)
}

async fn preflight_window_write(
	client: &Client,
	mutation: &QuotaWindowMutation,
) -> Result<(), StoreError> {
	let row = client
		.query_opt(
			"SELECT revision, observed_at > TIMESTAMPTZ '1970-01-01 00:00:00+00' \
			  + ($4::bigint / 1000000) * INTERVAL '1 second' \
			  + ($4::bigint % 1000000) * INTERVAL '1 microsecond' \
			 FROM decodex.quota_windows WHERE account_id=$1::text::uuid \
			 AND window_class=$2::text::decodex.quota_window_class AND duration_minutes=$3",
			&[
				&mutation.account_id.as_str(),
				&response::window_class_sql(mutation.window),
				&i16::try_from(mutation.window.duration_minutes())
					.expect("closed quota durations fit PostgreSQL smallint"),
				&mutation.observed_at.get(),
			],
		)
		.await?;

	match (row, mutation.expected_revision) {
		(None, None) => Ok(()),
		(Some(row), Some(expected)) if row.get::<_, i64>(0) == expected => {
			if row.get::<_, bool>(1) {
				Err(StoreError::InvalidInput("quota observation cannot move backward in time"))
			} else {
				Ok(())
			}
		},
		(row, expected) => Err(StoreError::RevisionConflict {
			entity: format!(
				"quota_window/{}:{}",
				mutation.account_id,
				response::window_class_sql(mutation.window)
			),
			expected,
			actual: row.map(|row| row.get(0)),
		}),
	}
}

async fn persist_window(
	transaction: &Transaction<'_>,
	mutation: &QuotaWindowMutation,
) -> Result<QuotaWindow, StoreError> {
	let duration = i16::try_from(mutation.window.duration_minutes())
		.expect("closed quota durations fit PostgreSQL smallint");
	let remaining = mutation.remaining_percent.map(|value| i16::from(value.get()));
	let resets_at = mutation.resets_at.map(QuotaTimestampMicros::get);
	let row = match mutation.expected_revision {
		None =>
			transaction
				.query_opt(
					"INSERT INTO decodex.quota_windows \
					 (account_id,window_class,duration_minutes,remaining_percent,resets_at, \
					  observed_at,confidence,metadata) \
					 VALUES ($1::text::uuid,$2::text::decodex.quota_window_class,$3,$4, \
					  CASE WHEN $5::bigint IS NULL THEN NULL ELSE \
					   TIMESTAMPTZ '1970-01-01 00:00:00+00' + ($5::bigint / 1000000) * INTERVAL '1 second' \
					   + ($5::bigint % 1000000) * INTERVAL '1 microsecond' END, \
					  TIMESTAMPTZ '1970-01-01 00:00:00+00' + ($6::bigint / 1000000) * INTERVAL '1 second' \
					   + ($6::bigint % 1000000) * INTERVAL '1 microsecond', \
					  $7::text::decodex.observation_confidence,$8) \
					 ON CONFLICT DO NOTHING RETURNING revision",
					&[
						&mutation.account_id.as_str(),
						&response::window_class_sql(mutation.window),
						&duration,
						&remaining,
						&resets_at,
						&mutation.observed_at.get(),
						&response::confidence_sql(mutation.confidence),
						&mutation.metadata,
					],
				)
				.await?,
		Some(expected) =>
			transaction
				.query_opt(
					"UPDATE decodex.quota_windows SET remaining_percent=$4, \
					 resets_at=CASE WHEN $5::bigint IS NULL THEN NULL ELSE \
					  TIMESTAMPTZ '1970-01-01 00:00:00+00' + ($5::bigint / 1000000) * INTERVAL '1 second' \
					  + ($5::bigint % 1000000) * INTERVAL '1 microsecond' END, \
					 observed_at=TIMESTAMPTZ '1970-01-01 00:00:00+00' \
					  + ($6::bigint / 1000000) * INTERVAL '1 second' \
					  + ($6::bigint % 1000000) * INTERVAL '1 microsecond', \
					 confidence=$7::text::decodex.observation_confidence, \
					 metadata=$8,revision=revision+1,updated_at=clock_timestamp() \
					 WHERE account_id=$1::text::uuid \
					 AND window_class=$2::text::decodex.quota_window_class \
					 AND duration_minutes=$3 AND revision=$9 \
					 AND observed_at <= TIMESTAMPTZ '1970-01-01 00:00:00+00' \
					  + ($6::bigint / 1000000) * INTERVAL '1 second' \
					  + ($6::bigint % 1000000) * INTERVAL '1 microsecond' \
					 RETURNING revision",
					&[
						&mutation.account_id.as_str(),
						&response::window_class_sql(mutation.window),
						&duration,
						&remaining,
						&resets_at,
						&mutation.observed_at.get(),
						&response::confidence_sql(mutation.confidence),
						&mutation.metadata,
						&expected,
					],
				)
				.await?,
	};
	let Some(row) = row else {
		return Err(window_write_error(transaction, mutation).await?);
	};

	Ok(QuotaWindow {
		account_id: mutation.account_id.clone(),
		window: mutation.window,
		remaining_percent: mutation.remaining_percent,
		resets_at: mutation.resets_at,
		observed_at: mutation.observed_at,
		confidence: mutation.confidence,
		metadata: mutation.metadata.clone(),
		revision: row.get(0),
	})
}

async fn window_write_error(
	transaction: &Transaction<'_>,
	mutation: &QuotaWindowMutation,
) -> Result<StoreError, StoreError> {
	let row = transaction
		.query_opt(
			"SELECT revision, observed_at > TIMESTAMPTZ '1970-01-01 00:00:00+00' \
			  + ($4::bigint / 1000000) * INTERVAL '1 second' \
			  + ($4::bigint % 1000000) * INTERVAL '1 microsecond' \
			 FROM decodex.quota_windows \
			 WHERE account_id=$1::text::uuid \
			 AND window_class=$2::text::decodex.quota_window_class AND duration_minutes=$3",
			&[
				&mutation.account_id.as_str(),
				&response::window_class_sql(mutation.window),
				&i16::try_from(mutation.window.duration_minutes())
					.expect("closed quota durations fit PostgreSQL smallint"),
				&mutation.observed_at.get(),
			],
		)
		.await?;

	if let Some(row) = &row
		&& mutation.expected_revision == Some(row.get(0))
		&& row.get::<_, bool>(1)
	{
		return Ok(StoreError::InvalidInput("quota observation cannot move backward in time"));
	}

	Ok(StoreError::RevisionConflict {
		entity: format!(
			"quota_window/{}:{}",
			mutation.account_id,
			response::window_class_sql(mutation.window)
		),
		expected: mutation.expected_revision,
		actual: row.map(|row| row.get(0)),
	})
}

async fn insert_exclusion(
	transaction: &Transaction<'_>,
	mutation: &QuotaExclusionMutation,
	window: &QuotaWindow,
	identity: &CanonicalMutationIdentity,
) -> Result<bool, StoreError> {
	let inserted = transaction
		.execute(
			"INSERT INTO decodex.quota_exclusions \
			 (account_id,window_class,duration_minutes,observation_revision,remaining_percent, \
			  confidence,observation_metadata,observed_at_micros,resets_at_micros, \
			  excluded_at_micros,maximum_age_micros,mutation_sha256,mutation_length) \
			 VALUES ($1::text::uuid,$2::text::decodex.quota_window_class,$3,$4,$5, \
			  $6::text::decodex.observation_confidence,$7,$8,$9,$10,$11,$12,$13) \
			 ON CONFLICT DO NOTHING",
			&[
				&mutation.observation.account_id.as_str(),
				&response::window_class_sql(mutation.observation.window),
				&i16::try_from(mutation.observation.window.duration_minutes())
					.expect("closed quota durations fit PostgreSQL smallint"),
				&window.revision,
				&i16::from(
					mutation
						.observation
						.remaining_percent
						.expect("validated exclusion has remaining evidence")
						.get(),
				),
				&response::confidence_sql(mutation.observation.confidence),
				&mutation.observation.metadata,
				&mutation.observation.observed_at.get(),
				&mutation
					.observation
					.resets_at
					.expect("validated exclusion has reset evidence")
					.get(),
				&mutation.excluded_at.get(),
				&i64::try_from(MAXIMUM_OBSERVATION_AGE_MICROS)
					.expect("maximum age fits PostgreSQL bigint"),
				&identity.sha256,
				&identity.length,
			],
		)
		.await?;

	if inserted == 1 {
		return Ok(true);
	}

	let matching: bool = transaction
		.query_one(
			"SELECT mutation_sha256=$5 AND mutation_length=$6 \
			 FROM decodex.quota_exclusions WHERE account_id=$1::text::uuid \
			 AND window_class=$2::text::decodex.quota_window_class \
			 AND duration_minutes=$3 AND observation_revision=$4",
			&[
				&mutation.observation.account_id.as_str(),
				&response::window_class_sql(mutation.observation.window),
				&i16::try_from(mutation.observation.window.duration_minutes())
					.expect("closed quota durations fit PostgreSQL smallint"),
				&window.revision,
				&identity.sha256,
				&identity.length,
			],
		)
		.await?
		.get(0);

	if matching { Ok(false) } else { Err(StoreError::IdempotencyConflict) }
}

#[cfg(test)] mod tests;
