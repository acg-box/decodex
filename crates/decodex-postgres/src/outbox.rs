use std::time::Duration;

use serde_json::Value;

use crate::{OutboxClaim, OutboxReconciliation, PostgresStore, ReconciliationOutcome, StoreError};

impl PostgresStore {
	/// Claim at most `limit` available rows through `FOR UPDATE SKIP LOCKED`. Expired claims
	/// are reclaimable; a row whose prior effect began is returned as reconciliation-required.
	pub async fn claim_outbox(
		&self,
		worker_id: &str,
		limit: u32,
		lease: Duration,
	) -> Result<Vec<OutboxClaim>, StoreError> {
		if limit == 0 || limit > 1_000 {
			return Err(StoreError::InvalidInput("outbox claim limit must be within 1..=1000"));
		}

		let limit = i64::from(limit);
		let lease_millis = crate::exact_milliseconds(lease)?;
		let mut client = self.pool().get().await?;
		let transaction = client.transaction().await?;
		let rows = transaction
			.query(
				"WITH write_time AS MATERIALIZED (SELECT clock_timestamp() AS value), \
				 exhausted AS ( \
				   UPDATE decodex.outbox SET state = 'dead_letter', lease_holder = NULL, \
				     claim_token = NULL, lease_acquired_at = NULL, lease_expires_at = NULL, \
				     dead_lettered_at = write_time.value \
				   FROM write_time \
				   WHERE state = 'in_flight' AND lease_expires_at <= write_time.value \
				     AND attempt_count >= max_attempts AND effect_state = 'not_started' \
				   RETURNING id \
				 ), candidates AS ( \
				   SELECT id, effect_state <> 'not_started' AS requires_reconciliation \
				   FROM decodex.outbox CROSS JOIN write_time \
				   WHERE available_at <= write_time.value \
				     AND (attempt_count < max_attempts OR effect_state <> 'not_started') \
				     AND (state = 'pending' OR (state = 'in_flight' AND lease_expires_at <= write_time.value)) \
				   ORDER BY available_at, id FOR UPDATE SKIP LOCKED LIMIT $2 \
				 ) \
				 UPDATE decodex.outbox AS work SET state = 'in_flight', \
				   attempt_count = CASE WHEN work.attempt_count < work.max_attempts \
				     THEN work.attempt_count + 1 ELSE work.attempt_count END, \
				   lease_holder = $1::text::uuid, \
				   claim_token = gen_random_uuid(), \
				   lease_acquired_at = write_time.value, \
				   lease_expires_at = write_time.value + $3::bigint * interval '1 millisecond' \
				 FROM candidates CROSS JOIN write_time WHERE work.id = candidates.id \
				 RETURNING work.id, work.claim_token::text, work.effect_key, work.payload, work.attempt_count, \
				   candidates.requires_reconciliation, work.receipt",
				&[&worker_id, &limit, &lease_millis],
			)
			.await?;

		transaction.commit().await?;

		Ok(rows
			.into_iter()
			.map(|row| OutboxClaim {
				id: row.get(0),
				claim_token: row.get(1),
				effect_key: row.get(2),
				payload: row.get(3),
				attempt_count: row.get(4),
				requires_reconciliation: row.get(5),
				receipt: row.get(6),
			})
			.collect())
	}

	/// Renew only the live claim identified by both worker and per-claim fencing token.
	pub async fn renew_outbox_claim(
		&self,
		id: i64,
		worker_id: &str,
		claim_token: &str,
		lease: Duration,
	) -> Result<(), StoreError> {
		let lease_millis = crate::exact_milliseconds(lease)?;
		let updated = self
			.pool()
			.get()
			.await?
			.execute(
				"WITH write_time AS (SELECT clock_timestamp() AS value) \
				 UPDATE decodex.outbox \
				 SET lease_acquired_at = write_time.value, \
				   lease_expires_at = write_time.value + $4::bigint * interval '1 millisecond' \
				 FROM write_time \
				 WHERE id = $1 AND state = 'in_flight' AND lease_holder = $2::text::uuid \
				   AND claim_token = $3::text::uuid AND lease_expires_at > write_time.value",
				&[&id, &worker_id, &claim_token, &lease_millis],
			)
			.await?;

		if updated == 1 { Ok(()) } else { Err(StoreError::OwnershipLost("outbox claim")) }
	}

	/// Durably mark that a worker is about to attempt the external effect. A crash after
	/// this commit makes every future claimant reconcile instead of blindly replaying.
	pub async fn begin_outbox_effect(
		&self,
		id: i64,
		worker_id: &str,
		claim_token: &str,
	) -> Result<(), StoreError> {
		self.owner_transition(
			"UPDATE decodex.outbox SET effect_state = 'ambiguous' \
			 WHERE id = $1 AND state = 'in_flight' AND lease_holder = $2::text::uuid \
			   AND claim_token = $3::text::uuid \
			   AND lease_expires_at > clock_timestamp() AND effect_state = 'not_started'",
			id,
			worker_id,
			claim_token,
			None,
		)
		.await
	}

	/// Record a provider receipt while retaining the requirement for authoritative readback.
	pub async fn record_outbox_receipt(
		&self,
		id: i64,
		worker_id: &str,
		claim_token: &str,
		receipt: &Value,
	) -> Result<(), StoreError> {
		crate::ensure_meaningful_evidence(receipt)?;
		crate::ensure_credential_negative_json(receipt)?;

		self.owner_transition(
			"UPDATE decodex.outbox SET effect_state = 'receipt_recorded', receipt = $4 \
			 WHERE id = $1 AND state = 'in_flight' AND lease_holder = $2::text::uuid \
			   AND claim_token = $3::text::uuid \
			   AND lease_expires_at > clock_timestamp() AND effect_state = 'ambiguous'",
			id,
			worker_id,
			claim_token,
			Some(receipt),
		)
		.await
	}

	/// Schedule an ordinary retry only when the worker can prove no external effect began.
	pub async fn retry_outbox_before_effect(
		&self,
		id: i64,
		worker_id: &str,
		claim_token: &str,
		failure_code: &str,
		delay: Duration,
	) -> Result<(), StoreError> {
		validate_failure_code(failure_code)?;

		let delay_millis = crate::exact_milliseconds(delay)?;
		let updated = self
			.pool()
			.get()
			.await?
			.execute(
				"WITH write_time AS (SELECT clock_timestamp() AS value) \
				 UPDATE decodex.outbox SET \
				 state = CASE WHEN attempt_count >= max_attempts THEN 'dead_letter'::decodex.outbox_state \
				              ELSE 'pending'::decodex.outbox_state END, \
				 available_at = write_time.value + $5::bigint * interval '1 millisecond', \
				 lease_holder = NULL, claim_token = NULL, lease_acquired_at = NULL, \
				 lease_expires_at = NULL, last_failure_code = $4, \
				 dead_lettered_at = CASE WHEN attempt_count >= max_attempts THEN write_time.value ELSE NULL END \
				 FROM write_time \
				 WHERE id = $1 AND state = 'in_flight' AND lease_holder = $2::text::uuid \
				   AND claim_token = $3::text::uuid \
				   AND lease_expires_at > write_time.value AND effect_state = 'not_started'",
				&[&id, &worker_id, &claim_token, &failure_code, &delay_millis],
			)
			.await?;

		if updated == 1 { Ok(()) } else { Err(StoreError::OwnershipLost("outbox claim")) }
	}

	/// Reconcile an ambiguous effect. Readback proving presence completes it without replay;
	/// readback proving absence resets the ambiguity marker and schedules a bounded retry.
	pub async fn reconcile_outbox(
		&self,
		id: i64,
		worker_id: &str,
		claim_token: &str,
		reconciliation: &OutboxReconciliation,
		delay: Duration,
		retention: Duration,
	) -> Result<(), StoreError> {
		crate::ensure_meaningful_evidence(&reconciliation.readback)?;
		crate::ensure_credential_negative_json(&reconciliation.readback)?;

		let delay_millis = crate::exact_milliseconds(delay)?;
		let retention_millis = crate::exact_milliseconds(retention)?;
		let outcome = match reconciliation.outcome {
			ReconciliationOutcome::EffectPresent => "present",
			ReconciliationOutcome::EffectAbsent => "absent",
		};
		let updated = self
			.pool()
			.get()
			.await?
			.execute(
				"WITH write_time AS (SELECT clock_timestamp() AS value) \
				 UPDATE decodex.outbox SET \
				 state = CASE \
				   WHEN $5 = 'present' THEN 'delivered'::decodex.outbox_state \
				   WHEN attempt_count >= max_attempts THEN 'dead_letter'::decodex.outbox_state \
				   ELSE 'pending'::decodex.outbox_state END, \
				 effect_state = CASE WHEN $5 = 'absent' THEN 'not_started'::decodex.effect_state ELSE effect_state END, \
				 receipt = CASE WHEN $5 = 'absent' THEN NULL ELSE receipt END, \
				 reconciliation = $4, lease_holder = NULL, claim_token = NULL, \
				 lease_acquired_at = NULL, lease_expires_at = NULL, \
				 available_at = CASE WHEN $5 = 'absent' THEN write_time.value + $6::bigint * interval '1 millisecond' ELSE available_at END, \
				 delivered_at = CASE WHEN $5 = 'present' THEN write_time.value ELSE NULL END, \
				 dead_lettered_at = CASE WHEN $5 = 'absent' AND attempt_count >= max_attempts THEN write_time.value ELSE NULL END, \
				 retain_until = CASE WHEN $5 = 'present' THEN write_time.value + $7::bigint * interval '1 millisecond' ELSE retain_until END \
				 FROM write_time \
				 WHERE id = $1 AND state = 'in_flight' AND lease_holder = $2::text::uuid \
				   AND claim_token = $3::text::uuid \
				   AND lease_expires_at > write_time.value \
				   AND (($5 = 'present' AND effect_state = 'receipt_recorded' AND receipt IS NOT NULL) \
				     OR ($5 = 'absent' AND effect_state <> 'not_started'))",
				&[
					&id,
					&worker_id,
					&claim_token,
					&reconciliation.readback,
					&outcome,
					&delay_millis,
					&retention_millis,
				],
			)
			.await?;

		if updated == 1 { Ok(()) } else { Err(StoreError::OwnershipLost("outbox claim")) }
	}

	/// Delete delivered transport rows only after retention is due. Activity remains append-only.
	pub async fn prune_delivered_outbox(&self, limit: u32) -> Result<u64, StoreError> {
		if limit == 0 || limit > 1_000 {
			return Err(StoreError::InvalidInput("outbox prune limit must be within 1..=1000"));
		}

		let limit = i64::from(limit);
		let deleted = self
			.pool()
			.get()
			.await?
			.execute(
				"DELETE FROM decodex.outbox WHERE id IN ( \
				 SELECT id FROM decodex.outbox WHERE state = 'delivered' \
				 AND retain_until <= clock_timestamp() ORDER BY id LIMIT $1 )",
				&[&limit],
			)
			.await?;

		Ok(deleted)
	}

	async fn owner_transition(
		&self,
		statement: &str,
		id: i64,
		worker_id: &str,
		claim_token: &str,
		payload: Option<&Value>,
	) -> Result<(), StoreError> {
		let client = self.pool().get().await?;
		let updated = if let Some(payload) = payload {
			client.execute(statement, &[&id, &worker_id, &claim_token, payload]).await?
		} else {
			client.execute(statement, &[&id, &worker_id, &claim_token]).await?
		};

		if updated == 1 { Ok(()) } else { Err(StoreError::OwnershipLost("outbox claim")) }
	}
}

fn validate_failure_code(failure_code: &str) -> Result<(), StoreError> {
	let valid = !failure_code.is_empty()
		&& failure_code.len() <= 128
		&& failure_code.bytes().enumerate().all(|(index, byte)| {
			byte.is_ascii_lowercase() || (index > 0 && (byte.is_ascii_digit() || byte == b'_'))
		});

	if valid {
		crate::ensure_credential_negative_text(failure_code)
	} else {
		Err(StoreError::InvalidInput("outbox failure code must be lower snake case"))
	}
}
