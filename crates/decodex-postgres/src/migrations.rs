mod embedded {
	refinery::embed_migrations!("migrations");
}

use deadpool_postgres::Client;

use crate::{REQUIRED_POSTGRES_MAJOR, StoreError};
use embedded::migrations;

pub(crate) async fn run(client: &mut Client) -> Result<(), StoreError> {
	migrations::runner().run_async(&mut ***client).await?;

	Ok(())
}

pub(crate) async fn verify(client: &Client) -> Result<(), StoreError> {
	let row = client
		.query_one(
			"SELECT pg_catalog.current_setting('server_version_num')::integer / 10000, \
			 pg_catalog.current_setting('data_checksums')",
			&[],
		)
		.await?;
	let major: i32 = row.get(0);
	let checksums: String = row.get(1);

	if major != i32::try_from(REQUIRED_POSTGRES_MAJOR).expect("major fits i32") {
		return Err(StoreError::Incompatible(format!("PostgreSQL major {major}, expected 18")));
	}
	if checksums != "on" {
		return Err(StoreError::Incompatible("data checksums are not enabled".into()));
	}

	let pgcrypto = client
		.query_opt("SELECT extversion FROM pg_catalog.pg_extension WHERE extname = 'pgcrypto'", &[])
		.await?
		.ok_or_else(|| StoreError::Incompatible("required pgcrypto extension is absent".into()))?
		.get::<_, String>(0);

	if pgcrypto != "1.4" {
		return Err(StoreError::Incompatible(format!("pgcrypto version {pgcrypto}, expected 1.4")));
	}

	let actual = client
		.query(
			"SELECT version, name, checksum FROM public.refinery_schema_history ORDER BY version",
			&[],
		)
		.await?;
	let runner = migrations::runner();
	let expected = runner.get_migrations();

	if actual.len() != expected.len() {
		return Err(StoreError::Incompatible(format!(
			"expected {} migration history entries, found {}",
			expected.len(),
			actual.len()
		)));
	}

	for (index, (actual, expected)) in actual.iter().zip(expected).enumerate() {
		let version = actual.get::<_, i32>(0);
		let name = actual.get::<_, String>(1);
		let checksum = actual.get::<_, String>(2);
		let expected_checksum = expected.checksum().to_string();

		if version != expected.version() || name != expected.name() || checksum != expected_checksum
		{
			return Err(StoreError::Incompatible(format!(
				"migration history entry {} does not match the embedded migration",
				index + 1
			)));
		}
	}

	Ok(())
}
