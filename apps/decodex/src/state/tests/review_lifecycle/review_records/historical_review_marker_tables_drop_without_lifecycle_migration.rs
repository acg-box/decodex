use rusqlite::Connection;
use tempfile::TempDir;

use crate::state::{StateStore, tests};

#[test]
fn historical_review_marker_tables_drop_without_lifecycle_migration() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");

	tests::seed_dropped_review_marker_tables(&state_path);

	let store = StateStore::open(&state_path).expect("state store should drop historical markers");

	assert!(
		store
			.review_handoff_marker("pubfi", "PUB-101", "x/decodex-pub-101")
			.expect("handoff projection should read")
			.is_none(),
		"historical review_handoffs rows must not become lifecycle records"
	);
	assert!(
		store
			.review_lifecycle_record("pubfi", "PUB-202", "x/decodex-pub-202")
			.expect("orchestration-only lifecycle should read")
			.is_none(),
		"historical review_orchestrations rows must not become lifecycle records"
	);
	assert!(
		store
			.review_lifecycle_record("pubfi", "PUB-303", "x/decodex-pub-303")
			.expect("stale historical lifecycle should read")
			.is_none(),
		"historical mixed review rows must not become lifecycle records"
	);

	drop(store);

	let connection = Connection::open(&state_path).expect("bootstrapped db should open");
	let legacy_table_count: i64 = connection
		.query_row(
			"SELECT COUNT(*) FROM sqlite_master \
			 WHERE type = 'table' AND name IN ('review_handoffs', 'review_orchestrations')",
			[],
			|row| row.get(0),
		)
		.expect("legacy marker tables should query");

	assert_eq!(legacy_table_count, 0);

	let lifecycle_count: i64 = connection
		.query_row("SELECT COUNT(*) FROM review_lifecycle_records", [], |row| row.get(0))
		.expect("review lifecycle rows should query");

	assert_eq!(lifecycle_count, 0);
}
