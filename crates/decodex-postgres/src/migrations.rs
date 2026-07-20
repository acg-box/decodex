mod embedded {
	refinery::embed_migrations!("migrations");
}

use deadpool_postgres::Client;
#[cfg(feature = "test-support")] use refinery::Target;

use crate::{REQUIRED_POSTGRES_MAJOR, StoreError};
use embedded::migrations;

const EXPECTED_LATEST_MIGRATION_VERSION: i32 = 21;
#[cfg(test)]
const CONSTRAINT_RESTORE_CANONICALIZATION_MIGRATION: &str =
	include_str!("../migrations/V20__constraint_restore_canonicalization.sql");

pub(crate) async fn run(client: &mut Client) -> Result<(), StoreError> {
	migrations::runner().run_async(&mut ***client).await?;

	Ok(())
}

#[cfg(feature = "test-support")]
pub(crate) async fn run_through_v7(client: &mut Client) -> Result<(), StoreError> {
	migrations::runner().set_target(Target::Version(7)).run_async(&mut ***client).await?;

	Ok(())
}

#[cfg(feature = "test-support")]
pub(crate) async fn run_through_v8(client: &mut Client) -> Result<(), StoreError> {
	migrations::runner().set_target(Target::Version(8)).run_async(&mut ***client).await?;

	Ok(())
}

#[cfg(feature = "test-support")]
pub(crate) async fn run_through_v9(client: &mut Client) -> Result<(), StoreError> {
	migrations::runner().set_target(Target::Version(9)).run_async(&mut ***client).await?;

	Ok(())
}

#[cfg(feature = "test-support")]
pub(crate) async fn run_through_v10(client: &mut Client) -> Result<(), StoreError> {
	migrations::runner().set_target(Target::Version(10)).run_async(&mut ***client).await?;

	verify_exact_ledger(client, 10, false).await
}

#[cfg(feature = "test-support")]
pub(crate) async fn run_through_v13(client: &mut Client) -> Result<(), StoreError> {
	migrations::runner().set_target(Target::Version(13)).run_async(&mut ***client).await?;

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

	verify_exact_ledger(client, EXPECTED_LATEST_MIGRATION_VERSION, true).await
}

async fn verify_exact_ledger(
	client: &Client,
	terminal_version: i32,
	require_embedded_terminal: bool,
) -> Result<(), StoreError> {
	let actual = client
		.query(
			"SELECT version, name, checksum FROM public.refinery_schema_history ORDER BY version",
			&[],
		)
		.await?;
	let runner = migrations::runner();
	let mut expected = runner.get_migrations().iter().collect::<Vec<_>>();

	expected.sort_by_key(|migration| migration.version());

	if require_embedded_terminal
		&& expected.last().map(|migration| migration.version()) != Some(terminal_version)
	{
		return Err(StoreError::Incompatible(
			"embedded migration inventory does not end at the canonical V21 ledger".into(),
		));
	}
	expected.retain(|migration| migration.version() <= terminal_version);
	if expected.last().map(|migration| migration.version()) != Some(terminal_version) {
		return Err(StoreError::Incompatible(format!(
			"embedded migration inventory does not contain terminal V{terminal_version}"
		)));
	}
	let expected_len = usize::try_from(terminal_version).map_err(|_| {
		StoreError::Incompatible(format!("invalid terminal migration V{terminal_version}"))
	})?;
	if expected.len() != expected_len
		|| expected
			.iter()
			.enumerate()
			.any(|(index, migration)| {
				migration.version()
					!= i32::try_from(index + 1).expect("migration prefix index fits i32")
			})
	{
		return Err(StoreError::Incompatible(format!(
			"embedded migration inventory is not the contiguous V1-V{terminal_version} prefix"
		)));
	}
	if actual.len() != expected.len() {
		return Err(StoreError::Incompatible(format!(
			"expected {} migration history entries through V{terminal_version}, found {}",
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
				"migration history entry {} through V{terminal_version} does not match the embedded migration",
				index + 1,
			)));
		}
	}

	Ok(())
}

#[cfg(test)]
mod tests {
	use super::CONSTRAINT_RESTORE_CANONICALIZATION_MIGRATION;

	const CONSTRAINTS: [&str; 9] = [
		"repository_admissions_identity_bounded",
		"repository_admissions_base_bounded",
		"repository_admissions_path_bounded",
		"repository_operations_descriptor_bounded",
		"repository_authority_transitions_head_bounded",
		"managed_repositories_worktree_path_bounded",
		"managed_repositories_head_bounded",
		"routing_policy_revisions_build",
		"routing_decision_exclusion_range",
	];

	#[test]
	fn v20_recreates_only_the_nine_restore_canonical_constraints() {
		let migration = CONSTRAINT_RESTORE_CANONICALIZATION_MIGRATION;

		assert_eq!(migration.matches("DROP CONSTRAINT").count(), CONSTRAINTS.len());
		assert_eq!(migration.matches("ADD CONSTRAINT").count(), CONSTRAINTS.len());
		for constraint in CONSTRAINTS {
			assert_eq!(
				migration.matches(&format!("DROP CONSTRAINT {constraint};")).count(),
				1
			);
			assert_eq!(
				migration.matches(&format!("ADD CONSTRAINT {constraint} CHECK")).count(),
				1
			);
		}
		assert!(!migration.lines().any(
			|line| !line.trim_start().starts_with("--") && line.contains("BETWEEN")
		));
		assert!(!migration.contains("CASCADE"));
	}

	#[test]
	fn v20_uses_explicit_restore_canonical_range_predicates() {
		let migration = CONSTRAINT_RESTORE_CANONICALIZATION_MIGRATION;

		for required in [
			"pg_catalog.octet_length(admitted_identity) >= 1",
			"pg_catalog.octet_length(admitted_identity) <= 256",
			"pg_catalog.octet_length(admitted_base) >= 1",
			"pg_catalog.octet_length(admitted_base) <= 256",
			"pg_catalog.octet_length(repository_absolute_path) >= 2",
			"pg_catalog.octet_length(repository_absolute_path) <= 4096",
			"pg_catalog.octet_length(descriptor::text) >= 2",
			"pg_catalog.octet_length(descriptor::text) <= 1048576",
			"pg_catalog.octet_length(payload::text) >= 2",
			"pg_catalog.octet_length(payload::text) <= 262144",
			"pg_catalog.octet_length(head) >= 1",
			"pg_catalog.octet_length(head) <= 256",
			"pg_catalog.octet_length(worktree_absolute_path) >= 2",
			"pg_catalog.octet_length(worktree_absolute_path) <= 4096",
			"pg_catalog.octet_length(required_build_id) >= 1",
			"pg_catalog.octet_length(required_build_id) <= 256",
			"observed_at_micros >= 0",
			"observed_at_micros <= 253402300799999999",
			"resets_at_micros >= observed_at_micros + 1",
			"resets_at_micros <= 253402300799999999",
		] {
			assert!(migration.contains(required), "{required}");
		}
	}
}
