//! Allocate display names in the same transaction that installs an account.
use std::collections::HashSet;

use rusqlite::{self, Connection};

use crate::{DatabaseError, error::sqlite_error};
use decodex_core::{self, ProviderIdentity};

pub(crate) fn for_enrollment(
	connection: &Connection,
	provider: &ProviderIdentity,
) -> Result<String, DatabaseError> {
	let mut statement = connection
		.prepare(
			"SELECT provider_account_id, display_label FROM accounts WHERE provider = 'chatgpt'",
		)
		.map_err(sqlite_error)?;
	let rows = statement
		.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
		.map_err(sqlite_error)?;
	let mut used = HashSet::new();

	for row in rows {
		let (id, label) = row.map_err(sqlite_error)?;

		if id == provider.account_id() {
			return Ok(label);
		}

		used.insert(label);
	}

	Ok(allocate(provider, &used))
}

fn allocate(provider: &ProviderIdentity, used: &HashSet<String>) -> String {
	for attempt in 0.. {
		let name = decodex_core::account_alias_candidate(provider, attempt);

		if !used.contains(&name) {
			return name;
		}
	}

	unreachable!("account alias candidate space exhausted")
}

#[cfg(test)]
mod tests {
	use crate::account_alias::{self, HashSet, ProviderIdentity};
	use decodex_core::AccountProvider;

	#[test]
	fn aliases_resolve_collisions_and_dictionary_exhaustion() {
		let mut used = HashSet::new();

		for number in 0..512 {
			let provider =
				ProviderIdentity::new(AccountProvider::Chatgpt, format!("provider-{number}"))
					.unwrap();
			let name = account_alias::allocate(&provider, &used);

			assert!((2..=16).contains(&name.len()));
			assert!(name.as_bytes()[0].is_ascii_uppercase());
			assert!(name.as_bytes()[1..].iter().all(u8::is_ascii_lowercase));
			assert_eq!(name, account_alias::allocate(&provider, &used));
			assert!(used.insert(name));
		}
	}
}
