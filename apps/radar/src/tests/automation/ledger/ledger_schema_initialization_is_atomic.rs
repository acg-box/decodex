use std::os::unix::fs::PermissionsExt as _;

use rusqlite::Connection;

#[test]
fn first_initialization_rolls_back_at_every_precommit_boundary_and_restarts_cleanly() {
	for boundary in ["after_inventory", "after_objects", "after_version", "before_commit"] {
		let temp_dir = crate::test_support::private_tempdir();

		std::fs::set_permissions(temp_dir.path(), std::fs::Permissions::from_mode(0o700))
			.expect("ledger parent should be private");
		let path = temp_dir.path().join(format!("{boundary}.sqlite3"));
		drop(crate::create_private_file(&path).expect("ledger file should be created"));

		let connection = Connection::open(&path).expect("empty ledger should open");
		let error = crate::ledger::initialize_ledger_with_failure(&connection, boundary)
			.expect_err("injected initialization must fail");

		assert!(error.to_string().contains("injected Radar ledger initialization failure"));
		let user_tables: i64 = connection
			.query_row(
				"
				SELECT COUNT(*)
				FROM sqlite_master
				WHERE type = 'table' AND name NOT LIKE 'sqlite_%'
				",
				[],
				|row| row.get(0),
			)
			.expect("rolled-back table inventory should be readable");

		assert_eq!(user_tables, 0, "{boundary} left a partial schema");
		drop(connection);

		let restarted =
			crate::ledger::open_ledger(&path).expect("restart should initialize schema 6 cleanly");
		let version: String = restarted
			.query_row("SELECT value FROM metadata WHERE key = 'schema_version'", [], |row| {
				row.get(0)
			})
			.expect("schema version should be stored");

		assert_eq!(version, crate::SCHEMA_VERSION.to_string());
		restarted.close().expect("restarted ledger should persist atomically");
	}
}
