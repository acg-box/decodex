use std::{fs, os::unix::fs::PermissionsExt as _, sync::mpsc, thread, time::Duration};

use rusqlite::Connection;

use crate::{RadarLedgerIngestExistingRequest, tests::fixtures};

#[test]
fn ledger_ingests_existing_bundle_analysis_and_signal_artifacts() {
	let temp_dir = crate::test_support::private_tempdir();
	fs::set_permissions(temp_dir.path(), fs::Permissions::from_mode(0o700))
		.expect("ledger directory should be private");
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

#[test]
fn ledger_ingest_existing_completes_with_the_canonical_cache_layout() {
	let temp_dir = crate::test_support::private_tempdir();
	let cache = temp_dir.path().join(crate::DEFAULT_CACHE_ROOT);
	let bundles_dir = cache.join("github/bundles");
	let analysis_dir = cache.join("generated/analysis");
	let signals_dir = cache.join("site-content/signals");
	let db_path = cache.join("github/radar.sqlite3");

	crate::write_json(&bundles_dir.join("openai-codex-pr-22414.json"), &fixtures::valid_bundle())
		.expect("private bundle fixture should be written");
	crate::write_json(
		&analysis_dir.join("openai-codex-pr-22414.analysis.json"),
		&serde_json::json!({"kind": "capability"}),
	)
	.expect("private analysis fixture should be written");
	crate::write_json(&signals_dir.join("openai-codex-pr-22414.json"), &fixtures::valid_signal())
		.expect("private signal fixture should be written");

	let (sender, receiver) = mpsc::channel();
	let handle = thread::spawn(move || {
		let result = crate::ledger_ingest_existing(&RadarLedgerIngestExistingRequest {
			db_path,
			bundles_dir,
			analysis_dir,
			signals_dir,
		})
		.map(|summary| summary.get("artifact_links").copied());

		sender.send(result).expect("ingest result should be delivered");
	});
	let result = receiver
		.recv_timeout(Duration::from_secs(3))
		.expect("canonical-cache ingest must not self-deadlock")
		.expect("canonical-cache ingest should succeed");

	assert_eq!(result, Some(3));
	handle.join().expect("canonical-cache ingest thread should finish");
}
