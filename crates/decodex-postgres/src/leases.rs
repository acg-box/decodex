use std::time::Duration;

use crate::{LeaseClaim, PostgresStore, StoreError};

impl PostgresStore {
	/// Atomically acquire, reclaim, or renew a lease. A different live holder receives a
	/// non-owning result; expiry rotates the fencing token and increments the revision.
	pub async fn try_acquire_lease(
		&self,
		resource_key: &str,
		holder_id: &str,
		ttl: Duration,
	) -> Result<LeaseClaim, StoreError> {
		validate_lease_resource_key(resource_key)?;

		let ttl_millis = crate::exact_milliseconds(ttl)?;
		let row = self
			.pool()
			.get()
			.await?
			.query_one(
				"SELECT acquired, lease_token::text, revision \
				 FROM decodex.try_acquire_lease($1, $2::text::uuid, $3::bigint * interval '1 millisecond')",
				&[&resource_key, &holder_id, &ttl_millis],
			)
			.await?;

		Ok(LeaseClaim { acquired: row.get(0), token: row.get(1), revision: row.get(2) })
	}

	/// Renew only a live lease held through the exact fencing token.
	pub async fn renew_lease(
		&self,
		resource_key: &str,
		holder_id: &str,
		token: &str,
		ttl: Duration,
	) -> Result<(), StoreError> {
		validate_lease_resource_key(resource_key)?;

		let ttl_millis = crate::exact_milliseconds(ttl)?;
		let renewed: Option<bool> = self
			.pool()
			.get()
			.await?
			.query_one(
				"SELECT decodex.renew_lease($1, $2::text::uuid, $3::text::uuid, $4::bigint * interval '1 millisecond')",
				&[&resource_key, &holder_id, &token, &ttl_millis],
			)
			.await?
			.get(0);

		if renewed == Some(true) { Ok(()) } else { Err(StoreError::OwnershipLost("lease")) }
	}

	/// Release only the exact holder and fencing token.
	pub async fn release_lease(
		&self,
		resource_key: &str,
		holder_id: &str,
		token: &str,
	) -> Result<(), StoreError> {
		validate_lease_resource_key(resource_key)?;

		let released: Option<bool> = self
			.pool()
			.get()
			.await?
			.query_one(
				"SELECT decodex.release_lease($1, $2::text::uuid, $3::text::uuid)",
				&[&resource_key, &holder_id, &token],
			)
			.await?
			.get(0);

		if released == Some(true) { Ok(()) } else { Err(StoreError::OwnershipLost("lease")) }
	}
}

fn validate_lease_resource_key(resource_key: &str) -> Result<(), StoreError> {
	if resource_key.is_empty() || resource_key.len() > 256 {
		return Err(StoreError::InvalidInput("lease resource key must contain 1..=256 bytes"));
	}

	crate::ensure_credential_negative_text(resource_key)?;

	Ok(())
}
