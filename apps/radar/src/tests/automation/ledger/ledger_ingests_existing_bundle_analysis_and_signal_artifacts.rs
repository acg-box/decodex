use std::fs;

use rusqlite::Connection;

use crate::{RadarLedgerIngestExistingRequest, tests::fixtures};

#[test]
fn ledger_ingests_existing_bundle_analysis_and_signal_artifacts() {
	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let bundles_dir = temp_dir.path().join("bundles");
	let analysis_dir = temp_dir.path().join("analysis");
	let signals_dir = temp_dir.path().join("signals");
	let db_path = temp_dir.path().join("radar.sqlite3");

	fs::create_dir_all(&bundles_dir).expect("bundles directory should be created");
	fs::create_dir_all(&analysis_dir).expect("analysis directory should be created");
	fs::create_dir_all(&signals_dir).expect("signals directory should be created");
	fs::write(bundles_dir.join("openai-codex-pr-22414.json"), fixtures::valid_bundle().to_string())
		.expect("bundle fixture should be written");
	fs::write(analysis_dir.join("openai-codex-pr-22414.analysis.json"), r#"{"kind":"capability"}"#)
		.expect("analysis fixture should be written");
	fs::write(signals_dir.join("openai-codex-pr-22414.json"), fixtures::valid_signal().to_string())
		.expect("signal fixture should be written");

	let summary = crate::ledger_ingest_existing(&RadarLedgerIngestExistingRequest {
		db_path: db_path.clone(),
		bundles_dir,
		analysis_dir,
		signals_dir,
	})
	.expect("existing artifacts should ingest");

	assert_eq!(summary.get("bundles_ingested"), Some(&1));
	assert_eq!(summary.get("upstream_commits"), Some(&1));
	assert_eq!(summary.get("radar_reviews"), Some(&1));
	assert_eq!(summary.get("artifact_links"), Some(&3));

	let connection = Connection::open(&db_path).expect("ingested ledger should open");
	let review: (String, String) = connection
		.query_row(
			"SELECT status, confidence FROM radar_review WHERE subject_kind = 'pr'",
			[],
			|row| Ok((row.get(0)?, row.get(1)?)),
		)
		.expect("review row should be readable");

	assert_eq!(review, ("signal".into(), "confirmed".into()));
}
