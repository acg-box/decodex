use std::os::unix::fs::PermissionsExt as _;

#[test]
fn ledger_rejects_oversized_fields_without_persisting_them() {
	let temp_dir = private_temp_dir();
	let path = temp_dir.path().join("radar.sqlite3");
	let mut ledger = crate::RadarLedger::open(&path).expect("ledger should open");
	let error = ledger
		.record_review("openai/codex", "pr", "22414", "watch", &"x".repeat(2049), Some("confirmed"))
		.expect_err("oversized review reason must fail");

	assert!(error.to_string().contains("reason must not exceed"));
	drop(ledger);

	let connection = crate::ledger::open_ledger(&path).expect("ledger should reopen");
	let rows: i64 = connection
		.query_row("SELECT COUNT(*) FROM radar_review", [], |row| row.get(0))
		.expect("review count should be readable");

	assert_eq!(rows, 0);
	connection.close().expect("empty ledger should close");
}

#[test]
fn ledger_writer_prunes_oldest_rows_before_commit() {
	let temp_dir = private_temp_dir();
	let path = temp_dir.path().join("radar.sqlite3");
	let connection = crate::ledger::open_ledger(&path).expect("ledger should open");

	connection.close().expect("initial ledger should persist");
	let raw = rusqlite::Connection::open(&path).expect("fixture ledger should open directly");

	raw.execute_batch(
		"
			WITH RECURSIVE sequence(value) AS (
			  SELECT 1
			  UNION ALL
			  SELECT value + 1 FROM sequence WHERE value < 10001
			)
			INSERT INTO radar_review (
			  repo,
			  subject_kind,
			  subject_id,
			  status,
			  reason,
			  confidence,
			  reviewed_at,
			  updated_at
			)
			SELECT
			  'openai/codex',
			  'pr',
			  printf('%d', value),
			  'watch',
			  '',
			  'likely',
			  printf('2026-01-01T00:00:%02dZ', value % 60),
			  printf('2026-01-01T00:00:%02dZ', value % 60)
			FROM sequence;
			",
	)
	.expect("oversized fixture row set should be inserted");
	drop(raw);

	let open_error = crate::RadarLedger::open(&path)
		.expect_err("opening a pre-existing over-limit ledger must fail before another write");

	assert!(open_error.to_string().contains("RADAR_LEDGER_ROW_LIMIT"));

	let raw = rusqlite::Connection::open(&path).expect("fixture ledger should open directly");

	raw.execute(
		"DELETE FROM radar_review WHERE rowid IN (SELECT rowid FROM radar_review LIMIT 1)",
		[],
	)
	.expect("fixture should return to the write boundary");
	drop(raw);

	let mut ledger = crate::RadarLedger::open(&path).expect("bounded ledger should open");

	ledger
		.record_review(
			"openai/codex",
			"pr",
			"new-subject",
			"watch",
			"Newest review.",
			Some("confirmed"),
		)
		.expect("bounded writer should accept and prune");
	ledger.commit().expect("bounded write should commit");

	let connection = crate::ledger::open_ledger(&path).expect("ledger should reopen");
	let rows: i64 = connection
		.query_row("SELECT COUNT(*) FROM radar_review", [], |row| row.get(0))
		.expect("review count should be readable");

	assert_eq!(rows, crate::LEDGER_MAX_ROWS_PER_TABLE as i64);
	connection.close().expect("bounded ledger should close");
}

fn private_temp_dir() -> tempfile::TempDir {
	let temp_dir = crate::test_support::private_tempdir();

	std::fs::set_permissions(temp_dir.path(), std::fs::Permissions::from_mode(0o700))
		.expect("ledger parent should be private");

	temp_dir
}
