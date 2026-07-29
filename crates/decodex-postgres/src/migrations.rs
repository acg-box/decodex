mod embedded {
	refinery::embed_migrations!("migrations");
}

use deadpool_postgres::Client;
#[cfg(feature = "test-support")] use refinery::Target;

use crate::{REQUIRED_POSTGRES_MAJOR, StoreError};
use embedded::migrations;

const EXPECTED_LATEST_MIGRATION_VERSION: i32 = 30;
#[cfg(test)]
const CONSTRAINT_RESTORE_CANONICALIZATION_MIGRATION: &str =
	include_str!("../migrations/V20__constraint_restore_canonicalization.sql");
#[cfg(test)]
const MAC_ACCOUNT_LIFECYCLE_MIGRATION: &str =
	include_str!("../migrations/V27__mac_account_lifecycle.sql");
#[cfg(test)]
const ACCOUNT_PROFILE_OBSERVATIONS_MIGRATION: &str =
	include_str!("../migrations/V28__account_profile_observations.sql");
#[cfg(test)]
const ACCOUNT_PROFILE_ARRAY_ZIP_MIGRATION: &str =
	include_str!("../migrations/V29__account_profile_array_zip.sql");
#[cfg(test)]
const CURRENT_CODEX_ACCOUNT_CAPABILITY_MIGRATION: &str =
	include_str!("../migrations/V30__current_codex_account_capability.sql");

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
			"embedded migration inventory does not end at the canonical V30 ledger".into(),
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
		|| expected.iter().enumerate().any(|(index, migration)| {
			migration.version()
				!= i32::try_from(index + 1).expect("migration prefix index fits i32")
		}) {
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
	use super::{
		ACCOUNT_PROFILE_ARRAY_ZIP_MIGRATION, ACCOUNT_PROFILE_OBSERVATIONS_MIGRATION,
		CONSTRAINT_RESTORE_CANONICALIZATION_MIGRATION, CURRENT_CODEX_ACCOUNT_CAPABILITY_MIGRATION,
		MAC_ACCOUNT_LIFECYCLE_MIGRATION, migrations,
	};

	const POSTGRESQL_SYNTAX_CONSTRUCTS: [&str; 7] =
		["coalesce", "nullif", "greatest", "least", "extract", "substring", "position"];

	#[derive(Clone, Copy)]
	enum SqlToken<'a> {
		Identifier(&'a str),
		Dot,
		LeftParen,
		Other,
	}

	fn sql_tokens(sql: &str) -> Vec<SqlToken<'_>> {
		let bytes = sql.as_bytes();
		let mut tokens = Vec::new();
		let mut offset = 0;

		while offset < bytes.len() {
			match bytes[offset] {
				byte if byte.is_ascii_whitespace() => offset += 1,
				b'-' if bytes.get(offset + 1) == Some(&b'-') => {
					offset += 2;
					while offset < bytes.len() && bytes[offset] != b'\n' {
						offset += 1;
					}
				},
				b'/' if bytes.get(offset + 1) == Some(&b'*') => {
					offset += 2;
					let mut depth = 1;
					while offset < bytes.len() && depth > 0 {
						if bytes[offset..].starts_with(b"/*") {
							depth += 1;
							offset += 2;
						} else if bytes[offset..].starts_with(b"*/") {
							depth -= 1;
							offset += 2;
						} else {
							offset += 1;
						}
					}
				},
				b'\'' => {
					offset += 1;
					while offset < bytes.len() {
						if bytes[offset] != b'\'' {
							offset += 1;
						} else if bytes.get(offset + 1) == Some(&b'\'') {
							offset += 2;
						} else {
							offset += 1;
							break;
						}
					}
				},
				b'.' => {
					tokens.push(SqlToken::Dot);
					offset += 1;
				},
				b'(' => {
					tokens.push(SqlToken::LeftParen);
					offset += 1;
				},
				byte if byte.is_ascii_alphabetic() || byte == b'_' => {
					let start = offset;
					offset += 1;
					while offset < bytes.len()
						&& (bytes[offset].is_ascii_alphanumeric()
							|| matches!(bytes[offset], b'_' | b'$'))
					{
						offset += 1;
					}
					tokens.push(SqlToken::Identifier(&sql[start..offset]));
				},
				_ => {
					tokens.push(SqlToken::Other);
					offset += 1;
				},
			}
		}

		tokens
	}

	fn schema_qualified_syntax_constructs(sql: &str) -> Vec<&str> {
		sql_tokens(sql)
			.windows(4)
			.filter_map(|tokens| match tokens {
				[
					SqlToken::Identifier(schema),
					SqlToken::Dot,
					SqlToken::Identifier(name),
					SqlToken::LeftParen,
				] if schema.eq_ignore_ascii_case("pg_catalog")
					&& POSTGRESQL_SYNTAX_CONSTRUCTS
						.iter()
						.any(|construct| name.eq_ignore_ascii_case(construct)) =>
					Some(*name),
				_ => None,
			})
			.collect()
	}

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
	fn embedded_migrations_do_not_schema_qualify_postgresql_syntax_constructs() {
		for migration in migrations::runner().get_migrations() {
			let sql = migration.sql().expect("embedded migrations retain their SQL");
			let invalid = schema_qualified_syntax_constructs(sql);

			assert!(
				invalid.is_empty(),
				"V{} schema-qualifies PostgreSQL syntax constructs: {invalid:?}",
				migration.version(),
			);
		}
	}

	#[test]
	fn syntax_construct_guard_covers_the_category_without_rejecting_catalog_functions() {
		for construct in POSTGRESQL_SYNTAX_CONSTRUCTS {
			let sql = format!("SELECT pg_catalog /* qualification */ . {construct} (value)");
			assert_eq!(schema_qualified_syntax_constructs(&sql), [construct]);
		}
		assert!(
			schema_qualified_syntax_constructs(
				"SELECT pg_catalog.jsonb_build_object('value', pg_catalog.max(value))",
			)
			.is_empty()
		);
	}

	#[test]
	fn v20_recreates_only_the_nine_restore_canonical_constraints() {
		let migration = CONSTRAINT_RESTORE_CANONICALIZATION_MIGRATION;

		assert_eq!(migration.matches("DROP CONSTRAINT").count(), CONSTRAINTS.len());
		assert_eq!(migration.matches("ADD CONSTRAINT").count(), CONSTRAINTS.len());
		for constraint in CONSTRAINTS {
			assert_eq!(migration.matches(&format!("DROP CONSTRAINT {constraint};")).count(), 1);
			assert_eq!(migration.matches(&format!("ADD CONSTRAINT {constraint} CHECK")).count(), 1);
		}
		assert!(
			!migration
				.lines()
				.any(|line| !line.trim_start().starts_with("--") && line.contains("BETWEEN"))
		);
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

	#[test]
	fn v27_is_an_empty_registry_clean_break_without_account_migration_authority() {
		let migration = MAC_ACCOUNT_LIFECYCLE_MIGRATION;

		assert!(migration.contains("IF EXISTS (SELECT 1 FROM decodex.accounts) THEN"));
		assert!(migration.contains("V27 requires an empty pre-V27 account registry"));
		for retired in [
			"account_migration_receipts",
			"decodex_account_migration_handoff",
			"record_account_migration_receipt_exact",
			"replace_account_routing_control_for_migration_exact",
			"p_require_complete",
		] {
			assert!(!migration.contains(retired), "{retired}");
		}
		assert!(
			migration.contains("CREATE FUNCTION decodex.lock_account_routing_universe_exact()")
		);
	}

	#[test]
	fn v28_profile_storage_is_bounded_credential_negative_and_runtime_function_only() {
		let migration = ACCOUNT_PROFILE_OBSERVATIONS_MIGRATION;

		for required in [
			"CREATE TABLE decodex.account_profile_snapshots",
			"CREATE TABLE decodex.account_profile_daily_usage",
			"CREATE FUNCTION decodex.observe_account_profile_exact",
			"CREATE FUNCTION decodex.read_account_profile_exact",
			"daily_count>36",
			"WHERE account_id=p_account_id FOR UPDATE",
			"account.revision=snapshot.account_revision",
		] {
			assert!(migration.contains(required), "{required}");
		}
		for forbidden in [
			"access_token",
			"refresh_token",
			"id_token",
			"provider_email",
			"plan_type",
			"GRANT SELECT",
			"GRANT INSERT",
			"GRANT UPDATE",
			"GRANT DELETE",
			"GRANT EXECUTE",
			"prepare_provider_attempt_exact",
			"runtime_role",
		] {
			assert!(!migration.contains(forbidden), "{forbidden}");
		}
	}

	#[test]
	fn v29_repairs_only_the_profile_array_zip() {
		let migration = ACCOUNT_PROFILE_ARRAY_ZIP_MIGRATION;

		assert!(
			migration.contains("CREATE OR REPLACE FUNCTION decodex.observe_account_profile_exact")
		);
		assert_eq!(migration.matches("FROM ROWS FROM (").count(), 2);
		assert!(!migration.contains("pg_catalog.unnest(p_daily_start_dates,p_daily_tokens)"));
		for forbidden in
			["CREATE TABLE", "CREATE TYPE", "GRANT ", "REVOKE ", "account_migration", "legacy"]
		{
			assert!(!migration.contains(forbidden), "{forbidden}");
		}
	}

	#[test]
	fn v30_rebinds_only_the_current_account_capability_and_profile_function() {
		let migration = CURRENT_CODEX_ACCOUNT_CAPABILITY_MIGRATION;

		for function in [
			"read_account_registry_exact",
			"read_reset_card_account_admission_exact",
			"attest_codex_account_capability_exact",
			"prepare_process_generation_exact",
			"observe_account_profile_exact",
		] {
			assert!(
				migration.contains(&format!("CREATE OR REPLACE FUNCTION decodex.{function}(")),
				"{function}"
			);
		}
		assert_eq!(
			migration
				.matches("fb2b6b35789e59c885cf4d2aee12475809dd67b2c10df580e638122fd6b3438e")
				.count(),
			4
		);
		assert!(
			!migration.contains("6d8be49e49751554df16572369e636cbe02c84b208cad3dc35528c846eeca223")
		);
		assert!(migration.contains("AS duplicate_date(value)"));
		assert!(migration.contains("GROUP BY duplicate_date.value"));
		for forbidden in ["CREATE TABLE", "CREATE TYPE", "account_migration", "legacy"] {
			assert!(!migration.contains(forbidden), "{forbidden}");
		}
	}
}
