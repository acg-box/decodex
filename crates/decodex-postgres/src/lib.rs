//! PostgreSQL product-state adapter boundary.
//!
//! XY-1265 deliberately supplies no connection, schema, migration, or fallback store.

use decodex_core::{Availability, ProductState};

/// Product-state authority selected by this infrastructure owner.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProductStateAuthority {
	/// PostgreSQL domain tables and their transactional mechanisms.
	Postgres,
}

/// Stable unavailable reason while production persistence remains outside XY-1265.
pub const NOT_IMPLEMENTED: &str = "PostgreSQL store is unavailable until XY-1267";

/// The sole product-state adapter selected by the vNext composition root.
#[derive(Clone, Copy, Debug, Default)]
pub struct PostgresStore;

impl PostgresStore {
	/// Construct the explicit XY-1265 unavailable adapter.
	pub const fn unavailable() -> Self {
		Self
	}

	/// Report the concrete authority owned by this adapter.
	pub const fn authority(self) -> ProductStateAuthority {
		ProductStateAuthority::Postgres
	}
}

impl ProductState for PostgresStore {
	fn availability(&self) -> Availability {
		Availability::Unavailable { reason: NOT_IMPLEMENTED }
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn adapter_is_postgres_owned_and_explicitly_unavailable() {
		let store = PostgresStore::unavailable();

		assert_eq!(store.authority(), ProductStateAuthority::Postgres);
		assert_eq!(store.availability(), Availability::Unavailable { reason: NOT_IMPLEMENTED });
	}
}
