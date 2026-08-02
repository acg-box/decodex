use std::{ffi::CString, fs, os::unix::ffi::OsStrExt as _, sync::mpsc, thread};

use serde_json::Value;

use crate::{
	RadarRenderSignalRequest, RadarValidateRequest,
	tests::{env::TestEnvVars, fixtures},
};

#[test]
fn default_github_token_falls_back_to_workflow_token() {
	let _env = TestEnvVars::set(&[
		("GITHUB_PAT_X", Some("")),
		("GITHUB_PAT_Y", Some("")),
		("GH_TOKEN", None),
		("GITHUB_TOKEN", Some("workflow-token")),
	]);

	assert_eq!(
		crate::github_token(None).expect("default token lookup should succeed").as_deref(),
		Some("workflow-token")
	);
}

#[test]
fn default_github_token_prefers_gh_token() {
	let _env = TestEnvVars::set(&[
		("GITHUB_PAT_X", Some("")),
		("GITHUB_PAT_Y", Some("")),
		("GH_TOKEN", Some("cli-token")),
		("GITHUB_TOKEN", Some("workflow-token")),
	]);

	assert_eq!(
		crate::github_token(None).expect("default token lookup should succeed").as_deref(),
		Some("cli-token")
	);
}

#[test]
fn explicit_github_token_env_must_exist() {
	let _env = TestEnvVars::set(&[
		("DECODEX_TEST_MISSING_RADAR_TOKEN", None),
		("GITHUB_TOKEN", Some("workflow-token")),
	]);

	let error = crate::github_token(Some("DECODEX_TEST_MISSING_RADAR_TOKEN"))
		.expect_err("missing explicit token environment must fail");

	assert!(error.to_string().contains("DECODEX_TEST_MISSING_RADAR_TOKEN"));
}

#[test]
fn explicit_github_token_env_wins() {
	let _env = TestEnvVars::set(&[
		("DECODEX_TEST_RADAR_TOKEN", Some("explicit-token")),
		("GH_TOKEN", Some("cli-token")),
		("GITHUB_TOKEN", Some("workflow-token")),
	]);

	assert_eq!(
		crate::github_token(Some("DECODEX_TEST_RADAR_TOKEN"))
			.expect("explicit token lookup should succeed")
			.as_deref(),
		Some("explicit-token")
	);
}

#[test]
fn validates_json_files_from_directory() {
	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let path = temp_dir.path().join("bundle.json");

	fs::write(&path, fixtures::valid_bundle().to_string()).expect("fixture should be written");

	let report = crate::validate(&RadarValidateRequest {
		paths: vec![temp_dir.path().to_path_buf()],
		max_age_hours: None,
		bootstrap: false,
	})
	.expect("valid temporary bundle should pass");

	assert_eq!(report.checked_files, 1);
}

#[test]
fn validates_explicit_private_cache_files_by_absolute_path() {
	let temp_dir = crate::test_support::private_tempdir();
	let pair = temp_dir
		.path()
		.join(crate::DEFAULT_CACHE_ROOT)
		.join("github/content-review-pairs/test-pair");
	let review = pair.join("review.json");
	let impact = pair.join("impact.json");

	crate::write_json(&review, &fixtures::valid_upstream_review())
		.expect("private review should be written");
	crate::write_json(&impact, &fixtures::valid_upstream_impact())
		.expect("private impact should be written");
	assert!(review.is_absolute());
	assert!(impact.is_absolute());

	let report = crate::validate(&RadarValidateRequest {
		paths: vec![review, impact],
		max_age_hours: None,
		bootstrap: false,
	})
	.expect("exact private review and impact files should pass");

	assert_eq!(report.checked_files, 2);
}

#[test]
fn validates_json_files_from_explicit_private_cache_directory() {
	let temp_dir = crate::test_support::private_tempdir();
	let directory = temp_dir
		.path()
		.join(crate::DEFAULT_CACHE_ROOT)
		.join("github/test-validation-directory.json");

	crate::write_json(&directory.join("bundle.json"), &fixtures::valid_bundle())
		.expect("private bundle should be written");

	let report = crate::validate(&RadarValidateRequest {
		paths: vec![directory],
		max_age_hours: None,
		bootstrap: false,
	})
	.expect("an explicit private cache directory should still be traversed");

	assert_eq!(report.checked_files, 1);
}

#[test]
fn explicit_private_cache_regular_file_applies_json_extension_filter_after_classification() {
	let temp_dir = crate::test_support::private_tempdir();
	let non_json = temp_dir
		.path()
		.join(crate::DEFAULT_CACHE_ROOT)
		.join("github/content-review-pairs/test-pair/review.txt");

	crate::write_private_file_atomic(&non_json, b"not a JSON artifact")
		.expect("private non-JSON file should be written safely");

	let report = crate::validate(&RadarValidateRequest {
		paths: vec![non_json],
		max_age_hours: None,
		bootstrap: false,
	})
	.expect("an actual non-JSON private file should be filtered out");

	assert_eq!(report.checked_files, 0);
}

#[test]
fn explicit_private_cache_file_validation_rejects_symlink_leaf() {
	let temp_dir = crate::test_support::private_tempdir();
	let pair = temp_dir
		.path()
		.join(crate::DEFAULT_CACHE_ROOT)
		.join("github/content-review-pairs/test-pair");
	let review = pair.join("review.json");
	let linked = pair.join("linked-review.json");

	crate::write_json(&review, &fixtures::valid_upstream_review())
		.expect("private review should be written");
	std::os::unix::fs::symlink(&review, &linked).expect("symlink fixture should be created");

	let error = crate::validate(&RadarValidateRequest {
		paths: vec![linked],
		max_age_hours: None,
		bootstrap: false,
	})
	.expect_err("a symlinked private validation file must fail closed");

	assert!(error.to_string().contains("symlink"), "unexpected symlink error: {error:?}");
}

#[test]
fn explicit_private_cache_file_validation_rejects_unexpected_entry_type() {
	let temp_dir = crate::test_support::private_tempdir();
	let fifo = temp_dir
		.path()
		.join(crate::DEFAULT_CACHE_ROOT)
		.join("github/content-review-pairs/test-pair/review.json");

	crate::ensure_private_directory(fifo.parent().expect("FIFO parent should exist"))
		.expect("private FIFO parent should be created");
	let fifo_path =
		CString::new(fifo.as_os_str().as_bytes()).expect("FIFO path should not contain NUL");

	assert_eq!(
		unsafe { libc::mkfifo(fifo_path.as_ptr(), 0o600) },
		0,
		"FIFO fixture should be created"
	);

	let error = crate::validate(&RadarValidateRequest {
		paths: vec![fifo],
		max_age_hours: None,
		bootstrap: false,
	})
	.expect_err("an unexpected private validation entry type must fail closed");

	assert!(
		error.to_string().contains("regular") || error.to_string().contains("unsupported"),
		"unexpected entry-type error: {error:?}"
	);
}

#[test]
fn explicit_private_cache_file_validation_rejects_malformed_json() {
	let temp_dir = crate::test_support::private_tempdir();
	let malformed = temp_dir
		.path()
		.join(crate::DEFAULT_CACHE_ROOT)
		.join("github/content-review-pairs/test-pair/review.json");

	crate::write_private_file_atomic(&malformed, b"{not-json")
		.expect("malformed private JSON should be written safely");

	let error = crate::validate(&RadarValidateRequest {
		paths: vec![malformed],
		max_age_hours: None,
		bootstrap: false,
	})
	.expect_err("malformed private validation JSON must fail closed");

	assert!(
		error.to_string().contains("Failed to parse JSON"),
		"unexpected parse error: {error:?}"
	);
}

#[test]
fn default_collection_traversal_skips_missing_roots_but_explicit_traversal_fails() {
	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let missing = temp_dir.path().join("missing");

	assert!(
		crate::collect_json_files(std::slice::from_ref(&missing), true)
			.expect("missing default collection should be empty")
			.is_empty()
	);

	let error = crate::validate(&RadarValidateRequest {
		paths: vec![missing],
		max_age_hours: None,
		bootstrap: false,
	})
	.expect_err("missing explicit validation path must fail");

	assert!(error.to_string().contains("Radar validation path does not exist"));
}

#[test]
fn source_freshness_gate_rejects_stale_queue_and_accepts_current_queue() {
	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let path = temp_dir.path().join("queue.json");
	let mut queue = fixtures::valid_review_queue();

	fs::write(&path, queue.to_string()).expect("stale queue should be written");

	let error = crate::validate(&RadarValidateRequest {
		paths: vec![path.clone()],
		max_age_hours: Some(12),
		bootstrap: false,
	})
	.expect_err("stale queue must fail freshness");

	assert!(error.to_string().contains("older than the 12-hour source freshness limit"));

	queue["generated_at"] =
		serde_json::json!(crate::utc_now_iso().expect("current timestamp should format"));
	fs::write(&path, queue.to_string()).expect("current queue should be written");

	let report = crate::validate(&RadarValidateRequest {
		paths: vec![path],
		max_age_hours: Some(12),
		bootstrap: false,
	})
	.expect("current queue should pass freshness");

	assert_eq!(report.checked_files, 1);
}

#[test]
fn successful_equal_refresh_rewrites_the_observation_timestamp() {
	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let path = temp_dir.path().join("queue.json");
	let mut old_queue = fixtures::valid_review_queue();
	let mut refreshed_queue = old_queue.clone();

	old_queue["generated_at"] = serde_json::json!("2026-06-01T00:00:00Z");
	refreshed_queue["generated_at"] = serde_json::json!("2026-06-02T00:00:00Z");
	crate::write_json(&path, &old_queue).expect("old queue should be written");

	let refresh = crate::core_io::refresh_json(&path, &refreshed_queue, crate::RefreshKind::Queue)
		.expect("equal refresh should succeed");
	let stored = crate::load_json(&path).expect("refreshed queue should be readable");

	assert!(!refresh.material_changed);
	assert!(refresh.written);
	assert_eq!(refresh.refreshed_at, "2026-06-02T00:00:00Z");
	assert_eq!(stored["generated_at"], "2026-06-02T00:00:00Z");
}

#[test]
fn concurrent_refresh_compare_and_replace_is_one_monotonic_cache_operation() {
	let temp_dir = crate::test_support::private_tempdir();
	let path = temp_dir.path().join(crate::DEFAULT_QUEUE_OUT);
	let mut old_queue = fixtures::valid_review_queue();
	let mut newest_queue = old_queue.clone();
	let mut stale_queue = old_queue.clone();

	old_queue["generated_at"] = serde_json::json!("2026-06-01T00:00:00Z");
	newest_queue["generated_at"] = serde_json::json!("2026-06-03T00:00:00Z");
	stale_queue["generated_at"] = serde_json::json!("2026-06-02T00:00:00Z");
	crate::write_json(&path, &old_queue).expect("old private queue should be written");

	let (entered_sender, entered_receiver) = mpsc::channel();
	let (release_sender, release_receiver) = mpsc::channel();
	let newest_path = path.clone();
	let newest = thread::spawn(move || {
		crate::core_io::refresh_json_after_comparison(
			&newest_path,
			&newest_queue,
			crate::RefreshKind::Queue,
			move || {
				entered_sender.send(()).expect("newest refresh should announce its lock");
				release_receiver.recv().expect("newest refresh should be released");
			},
		)
		.map_err(|error| error.to_string())
	});

	entered_receiver.recv().expect("newest refresh should hold the cache lock");
	let (stale_started_sender, stale_started_receiver) = mpsc::channel();
	let stale_path = path.clone();
	let stale = thread::spawn(move || {
		stale_started_sender.send(()).expect("stale refresh should announce its attempt");
		crate::core_io::refresh_json(&stale_path, &stale_queue, crate::RefreshKind::Queue)
			.map_err(|error| error.to_string())
	});

	stale_started_receiver.recv().expect("stale refresh should attempt the cache lock");
	release_sender.send(()).expect("newest refresh should be released");
	newest.join().expect("newest refresh thread should finish").expect("newest refresh should win");
	let stale_error = stale
		.join()
		.expect("stale refresh thread should finish")
		.expect_err("stale refresh must not overwrite a newer observation");
	let stored = crate::load_json(&path).expect("newest queue should remain readable");

	assert!(stale_error.contains("older than the currently stored observation"));
	assert_eq!(stored["generated_at"], "2026-06-03T00:00:00Z");
}

#[test]
fn concurrent_refresh_reports_material_change_against_the_artifact_it_replaces() {
	let temp_dir = crate::test_support::private_tempdir();
	let path = temp_dir.path().join(crate::DEFAULT_QUEUE_OUT);
	let mut old_queue = fixtures::valid_review_queue();
	let mut first_queue = old_queue.clone();

	old_queue["generated_at"] = serde_json::json!("2026-06-01T00:00:00Z");
	first_queue["generated_at"] = serde_json::json!("2026-06-02T00:00:00Z");
	first_queue["source"]["upstream_head"] =
		serde_json::json!("cccccccccccccccccccccccccccccccccccccccc");
	let mut second_queue = first_queue.clone();

	second_queue["generated_at"] = serde_json::json!("2026-06-03T00:00:00Z");
	crate::write_json(&path, &old_queue).expect("old private queue should be written");

	let (entered_sender, entered_receiver) = mpsc::channel();
	let (release_sender, release_receiver) = mpsc::channel();
	let first_path = path.clone();
	let first = thread::spawn(move || {
		crate::core_io::refresh_json_after_comparison(
			&first_path,
			&first_queue,
			crate::RefreshKind::Queue,
			move || {
				entered_sender.send(()).expect("first refresh should announce its lock");
				release_receiver.recv().expect("first refresh should be released");
			},
		)
		.map_err(|error| error.to_string())
	});

	entered_receiver.recv().expect("first refresh should hold the cache lock");
	let second_path = path.clone();
	let second = thread::spawn(move || {
		crate::core_io::refresh_json(&second_path, &second_queue, crate::RefreshKind::Queue)
			.map_err(|error| error.to_string())
	});

	release_sender.send(()).expect("first refresh should be released");
	let first_report = first
		.join()
		.expect("first refresh thread should finish")
		.expect("first refresh should win");
	let second_report = second
		.join()
		.expect("second refresh thread should finish")
		.expect("second refresh should follow");

	assert!(first_report.material_changed);
	assert!(!second_report.material_changed);
	assert_eq!(second_report.refreshed_at, "2026-06-03T00:00:00Z");
}

#[test]
fn refresh_rejects_corrupt_existing_json_without_replacing_it() {
	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let path = temp_dir.path().join("queue.json");
	let corrupt = b"{not-json";

	fs::write(&path, corrupt).expect("corrupt fixture should be written");
	let error = crate::core_io::refresh_json(
		&path,
		&fixtures::valid_review_queue(),
		crate::RefreshKind::Queue,
	)
	.expect_err("refresh must not treat corrupt existing JSON as absent");

	assert!(error.to_string().contains("parse"));
	assert_eq!(fs::read(&path).expect("fixture should remain readable"), corrupt);
}

#[test]
fn daily_default_presence_fails_closed_but_explicit_bootstrap_accepts_empty_cache() {
	let temp_dir = crate::test_support::private_tempdir();
	let error = crate::validate_default_cache_presence(temp_dir.path(), false)
		.expect_err("daily validation must reject an empty cache");

	assert!(error.to_string().contains("requires current source snapshots"));
	crate::validate_default_cache_presence(temp_dir.path(), true)
		.expect("explicit bootstrap may accept an empty cache");
}

#[test]
fn explicit_bootstrap_rejects_explicit_validation_paths() {
	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let path = temp_dir.path().join("bundle.json");

	fs::write(&path, fixtures::valid_bundle().to_string()).expect("bundle should be written");
	let error = crate::validate(&RadarValidateRequest {
		paths: vec![path],
		max_age_hours: None,
		bootstrap: true,
	})
	.expect_err("bootstrap must be limited to the fixed generated cache");

	assert!(error.to_string().contains("RADAR_BOOTSTRAP_SCOPE"));
}

#[test]
fn explicit_bootstrap_rejects_every_partial_generated_cache_shape() {
	for partial in ["directory", "ledger", "temp_file"] {
		let temp_dir = crate::test_support::private_tempdir();
		let cache_root = temp_dir.path().join(crate::DEFAULT_CACHE_ROOT);

		match partial {
			"directory" => {
				crate::ensure_private_directory(&cache_root.join("github"))
					.expect("partial directory should be created");
			},
			"ledger" => {
				let ledger = temp_dir.path().join(crate::DEFAULT_LEDGER_PATH);
				let connection =
					crate::ledger::open_ledger(&ledger).expect("partial ledger should open");

				connection.close().expect("partial ledger should be persisted");
			},
			"temp_file" => {
				let path = cache_root.join(".radar-tmp-interrupted");

				drop(
					crate::create_private_file(&path).expect("partial temp file should be created"),
				);
			},
			_ => unreachable!("test partial shape must be known"),
		}

		let error = crate::validate_default_cache_presence(temp_dir.path(), true)
			.expect_err("partial generated cache must not bootstrap");

		assert!(error.to_string().contains("RADAR_BOOTSTRAP_NONEMPTY"), "{partial}: {error}");
	}
}

#[test]
fn renders_signal_from_bundle_and_analysis_fixture() {
	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let bundle_path = temp_dir.path().join("bundle.json");
	let analysis_path = temp_dir.path().join("analysis.json");
	let signal_path = temp_dir.path().join("signal.json");
	let analysis = serde_json::json!({
		"kind": "capability",
		"title": "Unix sockets for remote Codex",
		"summary": "Remote Codex can use Unix socket endpoints.",
		"why_it_matters": "Operators can use local socket transports.",
		"confidence": "confirmed",
		"impact": "medium",
		"proof_points": ["PR #22414 adds endpoint handling."],
		"slug": null,
		"config_flags": [],
		"how_to_try": null,
		"expected_effect": null,
		"caveats": null,
		"watch_state": null
	});

	fs::write(&bundle_path, fixtures::valid_bundle().to_string())
		.expect("bundle should be written");
	fs::write(&analysis_path, analysis.to_string()).expect("analysis should be written");

	let report = crate::render_signal(&RadarRenderSignalRequest {
		bundle: bundle_path,
		analysis: analysis_path,
		out: signal_path.clone(),
		published_at: None,
	})
	.expect("rendered signal should pass validation");
	let rendered: Value = serde_json::from_str(
		&fs::read_to_string(&signal_path).expect("rendered signal should be readable"),
	)
	.expect("rendered signal should parse");

	assert_eq!(report.out, signal_path);
	assert_eq!(rendered["schema"], "signal_entry/v1");
	assert_eq!(rendered["slug"], "unix-sockets-for-remote-codex");
	assert_eq!(rendered["published_at"], "2026-06-01T00:00:00Z");
	assert_eq!(rendered["source_refs"]["items"][0]["meta"], serde_json::json!("#22414"));
	assert_eq!(rendered["source_refs"]["items"][1]["meta"], "abc123");
	assert!(rendered.get("how_to_try").is_none());
}
