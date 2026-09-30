//! Allocate display names in the same transaction that installs an account.
use std::collections::HashSet;

use decodex_core::{AccountProvider, ProviderIdentity, account_alias_candidate};
use rusqlite::{Connection, params};

use crate::{DatabaseError, error::sqlite_error};

fn allocate(provider: &ProviderIdentity, used: &HashSet<String>) -> String {
	for attempt in 0.. {
		let name = account_alias_candidate(provider, attempt);

		if !used.contains(&name) {
			return name;
		}
	}

	unreachable!("account alias candidate space exhausted")
}

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

pub(crate) fn migrate_names(connection: &Connection) -> Result<(), DatabaseError> {
	let mut statement = connection.prepare("SELECT account_id, provider_account_id FROM accounts ORDER BY provider, provider_account_id, account_id").map_err(sqlite_error)?;
	let accounts = statement
		.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
		.map_err(sqlite_error)?
		.collect::<Result<Vec<_>, _>>()
		.map_err(sqlite_error)?;
	let mut used = HashSet::new();

	for (id, provider_id) in accounts {
		let provider = ProviderIdentity::new(AccountProvider::Chatgpt, provider_id)
			.map_err(|_| DatabaseError::Corrupt)?;
		let name = allocate(&provider, &used);

		connection
			.execute(
				"UPDATE accounts SET display_label = ?1 WHERE account_id = ?2",
				params![name, id],
			)
			.map_err(sqlite_error)?;
		used.insert(name);
	}

	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn aliases_resolve_collisions_and_dictionary_exhaustion() {
		let mut used = HashSet::new();

		for number in 0..512 {
			let provider =
				ProviderIdentity::new(AccountProvider::Chatgpt, format!("provider-{number}"))
					.unwrap();
			let name = allocate(&provider, &used);

			assert!((2..=16).contains(&name.len()));
			assert!(name.as_bytes()[0].is_ascii_uppercase());
			assert!(name.as_bytes()[1..].iter().all(u8::is_ascii_lowercase));
			assert_eq!(name, allocate(&provider, &used));
			assert!(used.insert(name));
		}
	}

	#[test]
	fn aliases_migrate_duplicates_and_survive_new_accounts_and_restore() {
		let connection = Connection::open_in_memory().unwrap();

		connection.execute_batch("CREATE TABLE accounts(account_id TEXT, provider TEXT, provider_account_id TEXT, display_label TEXT); INSERT INTO accounts VALUES ('local-a', 'chatgpt', 'provider-a', 'Val'), ('local-b', 'chatgpt', 'provider-b', 'Val');").unwrap();

		migrate_names(&connection).unwrap();

		let a = ProviderIdentity::new(AccountProvider::Chatgpt, "provider-a").unwrap();
		let b = ProviderIdentity::new(AccountProvider::Chatgpt, "provider-b").unwrap();
		let original = for_enrollment(&connection, &a).unwrap();

		assert_ne!(original, for_enrollment(&connection, &b).unwrap());

		connection
			.execute(
				"INSERT INTO accounts VALUES ('local-c', 'chatgpt', 'provider-c', 'Other')",
				[],
			)
			.unwrap();

		assert_eq!(original, for_enrollment(&connection, &a).unwrap());
		// Replaying the deterministic backfill retains names for the same population.
		connection.execute("DELETE FROM accounts WHERE account_id = 'local-c'", []).unwrap();

		migrate_names(&connection).unwrap();

		assert_eq!(original, for_enrollment(&connection, &a).unwrap());
	}
}
