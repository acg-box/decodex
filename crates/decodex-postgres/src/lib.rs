//! PostgreSQL product-state authority for Decodex vNext.
//!
//! This crate owns only the XY-1267 persistence foundation: immutable migrations,
//! idempotent optimistic transactions, expiring leases, a transactional activity/outbox
//! boundary, and inert account/quota-window metadata. It does not select accounts, route
//! work, store credentials, or expose protocol/client behavior.

mod accounts;
mod error;
mod leases;
mod migrations;
mod outbox;
mod types;

pub use self::{
	error::StoreError,
	types::{
		AccountId, AccountMetadata, AccountMutation, AccountState, ActivityRecord, CommandIdentity,
		LeaseClaim, OutboxClaim, OutboxReconciliation, OutboxState, QuotaWindow,
		QuotaWindowMutation, ReconciliationOutcome,
	},
};

use std::{
	sync::{Arc, OnceLock},
	time::Duration,
};

use deadpool_postgres::{Manager, ManagerConfig, Pool, RecyclingMethod};
use regex::Regex;
use serde_json::Value;
#[cfg(test)]
use tokio as _;
use tokio_postgres::{Config, NoTls, config::Host};

use decodex_core::{Availability, ProductState};

/// PostgreSQL major accepted by the vNext storage authority.
pub const REQUIRED_POSTGRES_MAJOR: u32 = 18;
/// Stable reason returned by the composition seam before explicit verified configuration.
pub const NOT_CONFIGURED: &str = "PostgreSQL store requires explicit verified configuration";
/// Stable reason returned after the bounded connection pool is explicitly closed.
pub const CLOSED: &str = "PostgreSQL store connection pool is closed";

const INVALID_DURATION: &str = "duration must be a positive whole number of milliseconds";

/// Product-state authority selected by this infrastructure owner.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProductStateAuthority {
	/// PostgreSQL domain tables and their transactional mechanisms.
	Postgres,
}

/// Connected and migration-verified PostgreSQL product-state store.
#[derive(Clone)]
pub struct PostgresStore {
	pool: Arc<Pool>,
}
impl PostgresStore {
	/// Connect using explicit configuration, run embedded forward migrations, and prewarm
	/// two connections. Failure leaves no usable store and must be surfaced by the caller.
	pub async fn connect(config: Config) -> Result<Self, StoreError> {
		if config.get_hosts().len() != 1
			|| !matches!(config.get_hosts().first(), Some(Host::Unix(_)))
		{
			return Err(StoreError::Incompatible(
				"PostgreSQL must use one explicit Unix socket host".into(),
			));
		}

		let manager = Manager::from_config(
			config,
			NoTls,
			ManagerConfig { recycling_method: RecyclingMethod::Fast },
		);
		let pool = Pool::builder(manager).max_size(32).build()?;
		let mut client = pool.get().await?;

		migrations::run(&mut client).await?;
		migrations::verify(&client).await?;

		drop(client);

		let first = pool.get().await?;
		let second = pool.get().await?;

		drop((first, second));

		Ok(Self { pool: Arc::new(pool) })
	}

	/// Report the concrete authority owned by this adapter.
	pub const fn authority(&self) -> ProductStateAuthority {
		ProductStateAuthority::Postgres
	}

	/// Close the bounded pool. Existing checked-out connections finish before closure.
	pub fn close(&self) {
		self.pool.close();
	}

	pub(crate) fn pool(&self) -> &Pool {
		&self.pool
	}
}

impl ProductState for PostgresStore {
	fn availability(&self) -> Availability {
		if self.pool.is_closed() {
			Availability::Unavailable { reason: CLOSED }
		} else {
			// This synchronous port reports verified configuration and local pool lifecycle.
			// Individual operations remain authoritative for live PostgreSQL connectivity.
			Availability::Available
		}
	}
}

/// Unconfigured composition seam used until the path/bootstrap owner supplies explicit
/// connection configuration. It never opens a default or ambient PostgreSQL service.
#[derive(Clone, Copy, Debug, Default)]
pub struct UnavailablePostgresStore;
impl UnavailablePostgresStore {
	/// Construct the fail-closed, unconfigured adapter seam.
	pub const fn new() -> Self {
		Self
	}

	/// Report the concrete authority selected by the seam.
	pub const fn authority(self) -> ProductStateAuthority {
		ProductStateAuthority::Postgres
	}
}

impl ProductState for UnavailablePostgresStore {
	fn availability(&self) -> Availability {
		Availability::Unavailable { reason: NOT_CONFIGURED }
	}
}

pub(crate) fn exact_milliseconds(duration: Duration) -> Result<i64, StoreError> {
	if duration.is_zero() || !duration.subsec_nanos().is_multiple_of(1_000_000) {
		return Err(StoreError::InvalidInput(INVALID_DURATION));
	}

	i64::try_from(duration.as_millis()).map_err(|_| StoreError::InvalidInput(INVALID_DURATION))
}

pub(crate) fn ensure_credential_negative_text(value: &str) -> Result<(), StoreError> {
	if credential_value_pattern().is_match(value) {
		Err(StoreError::CredentialRejected)
	} else {
		Ok(())
	}
}

pub(crate) fn ensure_credential_negative_json(value: &Value) -> Result<(), StoreError> {
	match value {
		Value::Object(entries) => {
			for (key, value) in entries {
				if credential_key(key) {
					return Err(StoreError::CredentialRejected);
				}
				ensure_credential_negative_json(value)?;
			}
		},
		Value::Array(entries) => {
			for value in entries {
				ensure_credential_negative_json(value)?;
			}
		},
		Value::String(value) => ensure_credential_negative_text(value)?,
		_ => {},
	}

	Ok(())
}

fn credential_key(key: &str) -> bool {
	let normalized: String =
		key.chars().filter(char::is_ascii_alphanumeric).flat_map(char::to_lowercase).collect();

	[
		"credential",
		"credentials",
		"password",
		"passphrase",
		"privatekey",
		"secret",
		"authorization",
		"bearer",
		"apikey",
		"cookie",
		"token",
		"session",
	]
	.iter()
	.any(|suffix| normalized.ends_with(suffix))
}

fn credential_value_pattern() -> &'static Regex {
	static PATTERN: OnceLock<Regex> = OnceLock::new();

	PATTERN.get_or_init(|| {
		Regex::new(
			r"(?ix)
			(?:^|[\s[:punct:]])(?:bearer\s+[[:alnum:]_.~+/-]{8,}|basic\s+[[:alnum:]+/]{8,}={0,2})
			|(?:^|[^[:alnum:]])(?:sk-[[:alnum:]_-]{8,}|(?:sk|pk|rk)_(?:live|test|proj)?[[:alnum:]_-]{8,}|xox[baprs]-[[:alnum:]-]{8,}|glpat-[[:alnum:]_-]{8,}|npm_[[:alnum:]]{8,})
			|gh[pousr]_[[:alnum:]]{20,}
			|eyj[[:alnum:]_-]{8,}\.[[:alnum:]_-]{8,}\.[[:alnum:]_-]{8,}
			|-----begin[^-]*private\s+key-----
			|(?:password|passphrase|secret|token|authorization)\s*[:=]\s*[^\s]{4,}
			|[a-z][a-z0-9+.-]*://[^/:\s]+:[^@\s]+@
			|akia[0-9a-z]{16}",
		)
		.expect("credential material regex is valid")
	})
}
