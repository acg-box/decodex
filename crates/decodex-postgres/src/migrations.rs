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
			"SELECT current_setting('server_version_num')::integer / 10000, \
			 current_setting('data_checksums'), extversion \
			 FROM pg_extension WHERE extname = 'pgcrypto'",
			&[],
		)
		.await?;
	let major: i32 = row.get(0);
	let checksums: String = row.get(1);
	let pgcrypto: String = row.get(2);
	let history: i64 =
		client.query_one("SELECT count(*) FROM refinery_schema_history", &[]).await?.get(0);

	if major != i32::try_from(REQUIRED_POSTGRES_MAJOR).expect("major fits i32") {
		return Err(StoreError::Incompatible(format!("PostgreSQL major {major}, expected 18")));
	}
	if checksums != "on" {
		return Err(StoreError::Incompatible("data checksums are not enabled".into()));
	}
	if pgcrypto != "1.4" {
		return Err(StoreError::Incompatible(format!("pgcrypto version {pgcrypto}, expected 1.4")));
	}
	if history != 2 {
		return Err(StoreError::Incompatible(format!("expected 2 migrations, found {history}")));
	}

	Ok(())
}
