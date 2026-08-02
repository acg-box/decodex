use std::{
	ffi::CString,
	fs::{self, FileTimes},
	io::Write as _,
	os::unix::{
		ffi::OsStrExt as _,
		fs::{MetadataExt as _, PermissionsExt as _},
	},
	sync::mpsc,
	thread,
	time::{Duration, SystemTime},
};

use crate::{RadarCacheGcRequest, requests::CacheRetentionPolicy};

fn policy() -> CacheRetentionPolicy {
	CacheRetentionPolicy {
		max_age_days: 10,
		max_files_per_collection: 2,
		max_bytes_per_collection: 8,
		ledger_max_rows_per_table: 2,
		ledger_max_bytes: 128 * 1024,
	}
}

fn private_file(path: &std::path::Path, bytes: &[u8], modified: SystemTime) {
	if let Some(parent) = path.parent() {
		crate::ensure_private_directory(parent).expect("private parent should be created");
	}
	let mut file = crate::create_private_file(path).expect("private file should be created");

	file.write_all(bytes).expect("fixture bytes should be written");
	file.set_times(FileTimes::new().set_modified(modified)).expect("fixture mtime should be set");
}

#[test]
fn cache_gc_enforces_age_count_and_byte_limits_across_all_collections() {
	let temp_dir = crate::test_support::private_tempdir();
	let root = temp_dir.path().join(crate::DEFAULT_CACHE_ROOT);
	let now = SystemTime::now();
	let recent = now - Duration::from_secs(60);
	let older = now - Duration::from_secs(120);
	let oldest = now - Duration::from_secs(180);
	let stale = now - Duration::from_secs(20 * 24 * 60 * 60);

	private_file(&root.join("github/bundles/stale.json"), b"1", stale);
	private_file(&root.join("github/review-queue/new.json"), b"12", recent);
	private_file(&root.join("github/review-queue/older.json"), b"34", older);
	private_file(&root.join("github/review-queue/oldest.json"), b"56", oldest);
	private_file(&root.join("site-content/signals/new.json"), b"123456", recent);
	private_file(&root.join("site-content/signals/older.json"), b"123456", older);
	private_file(&root.join("generated/analysis/keep.analysis.json"), b"1234", recent);

	let report =
		crate::cache_gc(&RadarCacheGcRequest { cache_root: root.clone(), policy: policy(), now })
			.expect("cache GC should succeed");

	assert_eq!(report.files_removed, 3);
	assert!(!root.join("github/bundles/stale.json").exists());
	assert!(root.join("github/review-queue/new.json").exists());
	assert!(root.join("github/review-queue/older.json").exists());
	assert!(!root.join("github/review-queue/oldest.json").exists());
	assert!(root.join("site-content/signals/new.json").exists());
	assert!(!root.join("site-content/signals/older.json").exists());
	assert!(root.join("generated/analysis/keep.analysis.json").exists());
}

#[test]
fn cache_gc_covers_every_writer_collection_and_recovers_crash_temporary_files() {
	let temp_dir = crate::test_support::private_tempdir();
	let root = temp_dir.path().join(crate::DEFAULT_CACHE_ROOT);
	let now = SystemTime::now();
	let recent = now - Duration::from_secs(60);
	let older = now - Duration::from_secs(120);
	let collections = [
		"github/bundles",
		"github/review-queue",
		"site-content/signals",
		"site-content/release-deltas",
	];
	let mut strict = policy();

	strict.max_files_per_collection = 1;
	for collection in collections {
		private_file(&root.join(collection).join("new.json"), b"1", recent);
		private_file(&root.join(collection).join("old.json"), b"1", older);
	}
	private_file(&root.join("generated/new.json"), b"1", recent);
	private_file(&root.join("generated/analysis/old.json"), b"1", older);
	private_file(&root.join(".radar-tmp-crashed-root"), b"partial", recent);
	private_file(&root.join("github/bundles/.radar-tmp-crashed-nested"), b"partial", recent);

	let report =
		crate::cache_gc(&RadarCacheGcRequest { cache_root: root.clone(), policy: strict, now })
			.expect("cache GC should recover and retain every writer collection");

	assert_eq!(report.files_removed, collections.len() + 3);
	for collection in collections {
		assert!(root.join(collection).join("new.json").exists());
		assert!(!root.join(collection).join("old.json").exists());
	}
	assert!(root.join("generated/new.json").exists());
	assert!(!root.join("generated/analysis/old.json").exists());
	assert!(!root.join(".radar-tmp-crashed-root").exists());
	assert!(!root.join("github/bundles/.radar-tmp-crashed-nested").exists());
}

#[test]
fn cache_gc_prunes_ledger_rows_with_atomic_persistence() {
	let temp_dir = crate::test_support::private_tempdir();
	let root = temp_dir.path().join(crate::DEFAULT_CACHE_ROOT);
	let ledger = root.join("github/radar.sqlite3");
	let connection = crate::ledger::open_ledger(&ledger).expect("ledger should open");

	connection.close().expect("ledger fixture should persist");
	let raw = rusqlite::Connection::open(&ledger).expect("raw oversized fixture should open");

	raw.execute(
		"
		WITH RECURSIVE rows(value) AS (
		  SELECT 1
		  UNION ALL
		  SELECT value + 1 FROM rows WHERE value < 10001
		)
		INSERT INTO source_cache (url, body_sha256, fetched_at, cache_path)
		SELECT 'https://example.com/' || value, ?1, ?2, NULL FROM rows
		",
		rusqlite::params![
			"x".repeat(64),
			crate::utc_now_iso().expect("fixture timestamp should be available")
		],
	)
	.expect("oversized source cache rows should be inserted");
	drop(raw);
	let ledger_mode = fs::metadata(&ledger).unwrap().permissions().mode() & 0o777;
	let before = fs::metadata(&ledger).expect("oversized ledger metadata should exist");

	assert_eq!(ledger_mode, 0o600);

	let report = crate::cache_gc(&RadarCacheGcRequest {
		cache_root: root,
		policy: policy(),
		now: SystemTime::now(),
	})
	.expect("ledger retention should succeed");

	assert!(report.ledger_rows_removed >= 9_999);
	let after = fs::metadata(&ledger).expect("retained ledger metadata should exist");

	assert_ne!(before.ino(), after.ino());
	for entry in fs::read_dir(ledger.parent().expect("ledger should have a parent"))
		.expect("ledger parent should be readable")
	{
		let name = entry
			.expect("ledger parent entry should be readable")
			.file_name()
			.to_string_lossy()
			.into_owned();

		assert!(
			!name.ends_with("-journal")
				&& !name.ends_with("-wal")
				&& !name.ends_with("-shm")
				&& !name.starts_with(".radar-tmp-"),
			"unexpected SQLite or replacement sidecar: {name}"
		);
	}
	let connection = crate::ledger::open_ledger(&ledger).expect("retained ledger should reopen");
	let source_rows: i64 = connection
		.query_row("SELECT COUNT(*) FROM source_cache", [], |row| row.get(0))
		.expect("source row count should be read");

	assert_eq!(source_rows, 2);
	connection.close().expect("retained ledger should close");
}

#[test]
fn cache_gc_fails_closed_without_resetting_an_oversized_ledger() {
	let temp_dir = crate::test_support::private_tempdir();
	let root = temp_dir.path().join(crate::DEFAULT_CACHE_ROOT);
	let ledger = root.join("github/radar.sqlite3");
	let connection = crate::ledger::open_ledger(&ledger).expect("ledger should open");

	connection
		.execute(
			"
			INSERT INTO source_cache (url, body_sha256, fetched_at, cache_path)
			VALUES ('https://example.com/source', ?1, '2026-07-27T00:00:00Z', NULL)
			",
			["x".repeat(64)],
		)
		.expect("source cache row should be inserted");
	connection.close().expect("oversized fixture should persist");

	let before = fs::metadata(&ledger).expect("ledger metadata should exist");
	let mut strict = policy();

	strict.ledger_max_bytes = 1;
	let error = crate::cache_gc(&RadarCacheGcRequest {
		cache_root: root,
		policy: strict,
		now: SystemTime::now(),
	})
	.expect_err("impossible ledger byte limit must fail closed");
	let after = fs::metadata(&ledger).expect("ledger must not be deleted");

	assert!(error.to_string().contains("RADAR_LEDGER_OVERSIZE"));
	assert_eq!(
		std::os::unix::fs::MetadataExt::ino(&before),
		std::os::unix::fs::MetadataExt::ino(&after)
	);
	let reopened = crate::ledger::open_ledger(&ledger).expect("ledger must remain valid");
	let rows: i64 = reopened
		.query_row("SELECT COUNT(*) FROM source_cache", [], |row| row.get(0))
		.expect("source cache rows should remain readable");

	assert_eq!(rows, 1);
	reopened.close().expect("preserved ledger should close");
}

#[test]
fn cache_io_is_owner_only_and_rejects_symlinks_and_wrong_modes() {
	let temp_dir = crate::test_support::private_tempdir();
	let root = temp_dir.path().join(".agent/automations/radar/cache");
	let path = root.join("github/test-files/review.json");

	crate::write_json(&path, &serde_json::json!({"schema": "test"}))
		.expect("private JSON should be written");
	let directory_mode =
		fs::metadata(path.parent().expect("parent should exist")).unwrap().permissions().mode()
			& 0o777;
	let file_mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;

	assert_eq!(directory_mode, 0o700);
	assert_eq!(file_mode, 0o600);

	fs::set_permissions(&path, fs::Permissions::from_mode(0o644))
		.expect("fixture mode should be weakened");
	let mode_error = crate::load_json(&path).expect_err("wrong file mode must fail");

	assert!(mode_error.to_string().contains("expected 0600"));
	let owner_error =
		crate::simulate_wrong_owner_error(&path).expect_err("wrong owner must fail validation");

	assert!(owner_error.to_string().contains("wrong owner"));

	let symlink_parent = root.join("github/other-test-files");

	crate::ensure_private_directory(root.join("github").as_path())
		.expect("GitHub cache root should exist");
	std::os::unix::fs::symlink(temp_dir.path(), &symlink_parent)
		.expect("symlink fixture should be created");
	let symlink_error = crate::write_json(
		&symlink_parent.join("impact.json"),
		&serde_json::json!({"schema": "test"}),
	)
	.expect_err("symlinked cache directory must fail");

	assert!(!symlink_error.to_string().is_empty());

	let bad_root = temp_dir.path().join("bad-cache");

	crate::ensure_private_directory(&bad_root).expect("private cache root should be created");
	fs::set_permissions(&bad_root, fs::Permissions::from_mode(0o755))
		.expect("fixture directory mode should be weakened");
	let directory_error = crate::cache_gc(&RadarCacheGcRequest {
		cache_root: bad_root,
		policy: policy(),
		now: SystemTime::now(),
	})
	.expect_err("wrong directory mode must fail");

	assert!(directory_error.to_string().contains("expected 0700"));
}

#[test]
fn cache_io_rejects_a_symlink_before_the_fixed_cache_root() {
	let temp_dir = crate::test_support::private_tempdir();
	let actual_agent = temp_dir.path().join("actual-agent");
	let linked_agent = temp_dir.path().join(".agent");

	crate::ensure_private_directory(&actual_agent).expect("private target should be created");
	std::os::unix::fs::symlink(&actual_agent, &linked_agent)
		.expect("cache ancestor symlink should be created");
	let path = linked_agent.join("automations/radar/cache/github/test-files/review.json");
	let error = crate::write_json(&path, &serde_json::json!({"schema": "test"}))
		.expect_err("cache ancestor symlink must fail closed");

	assert!(!error.to_string().is_empty());
	assert!(!actual_agent.join("automations").exists());
}

#[test]
fn cache_io_rejects_parent_traversal_hard_links_and_root_replacement() {
	let temp_dir = crate::test_support::private_tempdir();
	let root = temp_dir.path().join(crate::DEFAULT_CACHE_ROOT);
	let path = root.join("github/test-files/review.json");

	crate::write_json(&path, &serde_json::json!({"schema": "test"}))
		.expect("private JSON should be written");
	let traversal = root.join("github/../impact.json");
	let traversal_error = crate::write_json(&traversal, &serde_json::json!({"schema": "test"}))
		.expect_err("parent traversal must fail");

	assert!(traversal_error.to_string().contains("must not contain '..'"));

	let hard_link = root.join("github/test-files/review-copy.json");

	fs::hard_link(&path, &hard_link).expect("hard-link fixture should be created");
	let link_error = crate::load_json(&path).expect_err("multiply-linked cache file must fail");

	assert!(link_error.to_string().contains("invalid link count"));
	fs::remove_file(hard_link).expect("hard-link fixture should be removed");

	let cache =
		crate::private_fs::PrivateCache::open_existing(&root).expect("cache root should open");
	let displaced = temp_dir.path().join("displaced-cache");

	fs::rename(&root, &displaced).expect("cache root should be displaced");
	crate::ensure_private_directory(&root).expect("replacement root should be created");
	let replacement_error = cache
		.read(std::path::Path::new("github/test-files/review.json"))
		.expect_err("open descriptor must reject root path replacement");

	assert!(replacement_error.to_string().contains("root identity changed"));
}

#[test]
fn cache_io_rejects_reserved_lock_and_temporary_destinations_before_replacement() {
	let temp_dir = crate::test_support::private_tempdir();
	let root = temp_dir.path().join(crate::DEFAULT_CACHE_ROOT);
	let ordinary = root.join("github/test-files/review.json");

	crate::write_json(&ordinary, &serde_json::json!({"schema": "test"}))
		.expect("ordinary output should initialize the cache");
	let lock_path = root.join(".radar.lock");
	let lock_before = fs::metadata(&lock_path).expect("cache lock should exist");
	let lock_error = crate::write_json(&lock_path, &serde_json::json!({"replace": true}))
		.expect_err("the cache lock name must be reserved");
	let temp_error = crate::write_json(
		&root.join("github/test-files/.radar-tmp-forged"),
		&serde_json::json!({"replace": true}),
	)
	.expect_err("the temporary-file prefix must be reserved");
	let lock_after = fs::metadata(&lock_path).expect("cache lock should remain");

	assert!(lock_error.to_string().contains("reserved internal file name"));
	assert!(temp_error.to_string().contains("reserved internal file name"));
	assert_eq!(lock_before.ino(), lock_after.ino());
	let cache =
		crate::private_fs::PrivateCache::open_existing(&root).expect("cache root should reopen");

	drop(cache.try_lock().expect("the original lock must remain authoritative"));
	assert!(!root.join("github/test-files/.radar-tmp-forged").exists());
}

#[test]
fn private_cache_read_stops_at_the_bound_when_a_file_grows_after_metadata() {
	let temp_dir = crate::test_support::private_tempdir();
	let path =
		temp_dir.path().join(crate::DEFAULT_CACHE_ROOT).join("github/test-files/review.json");
	let now = SystemTime::now();

	private_file(&path, b"1234", now);
	let append_path = path.clone();
	let error = crate::private_fs::read_private_file_bounded_after_metadata(&path, 4, move || {
		let mut file = fs::OpenOptions::new()
			.append(true)
			.open(append_path)
			.expect("fixture should reopen for append");

		file.write_all(b"5").expect("fixture should grow after metadata");
		file.sync_all().expect("fixture growth should be visible");
	})
	.expect_err("a growing private file must stop at max plus one bytes");

	assert!(error.to_string().contains("bounded read limit"));
}

#[test]
fn private_cache_bounded_read_rejects_a_fifo_without_blocking() {
	let temp_dir = crate::test_support::private_tempdir();
	let root = temp_dir.path().join(crate::DEFAULT_CACHE_ROOT);
	let relative = std::path::PathBuf::from("github/test-files/review.json");
	let path = root.join(&relative);

	crate::ensure_private_directory(path.parent().expect("FIFO parent should exist"))
		.expect("private FIFO parent should be created");
	let fifo = CString::new(path.as_os_str().as_bytes()).expect("FIFO path should not contain NUL");

	assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0, "FIFO should be created");
	let (sender, receiver) = mpsc::channel();
	let reader = thread::spawn(move || {
		let result = crate::private_fs::PrivateCache::open_existing(&root)
			.and_then(crate::private_fs::PrivateCache::lock)
			.and_then(|lock| lock.read_bounded(&relative, 1024));

		sender.send(result).expect("FIFO read result should be observed");
	});
	let result = receiver
		.recv_timeout(Duration::from_secs(2))
		.expect("private FIFO bounded read must not wait for a writer");

	reader.join().expect("FIFO reader thread should finish");
	let error = result.expect_err("a private FIFO must fail regular-file validation");

	assert!(error.to_string().contains("regular"), "unexpected FIFO error: {error:?}");
}

#[test]
fn private_cache_read_detects_an_mtime_preserving_in_place_rewrite() {
	let temp_dir = crate::test_support::private_tempdir();
	let path =
		temp_dir.path().join(crate::DEFAULT_CACHE_ROOT).join("github/test-files/review.json");
	let modified = SystemTime::now() - Duration::from_secs(60);

	private_file(&path, b"1234", modified);
	let initial = fs::metadata(&path).expect("initial metadata should be readable");
	let initial_ctime = (initial.ctime(), initial.ctime_nsec());
	let rewrite_path = path.clone();
	let error = crate::private_fs::read_private_file_bounded_after_metadata(&path, 4, move || {
		thread::sleep(Duration::from_millis(10));
		let mut file = fs::OpenOptions::new()
			.write(true)
			.truncate(true)
			.open(&rewrite_path)
			.expect("fixture should reopen in place");

		file.write_all(b"5678").expect("replacement bytes should be written");
		file.sync_all().expect("replacement bytes should be visible");
		file.set_times(FileTimes::new().set_modified(modified))
			.expect("fixture mtime should be restored");
		let changed = file.metadata().expect("changed metadata should be readable");

		assert_ne!((changed.ctime(), changed.ctime_nsec()), initial_ctime);
	})
	.expect_err("ctime must detect same-inode, same-size, mtime-preserving rewrites");

	assert!(error.to_string().contains("identity changed during read"));
}

#[test]
fn private_entry_kind_rejects_replacement_between_snapshots() {
	let temp_dir = crate::test_support::private_tempdir();
	let path =
		temp_dir.path().join(crate::DEFAULT_CACHE_ROOT).join("github/test-files/review.json");
	let displaced = path.with_file_name("original-review.json");

	private_file(&path, b"old", SystemTime::now());
	let replacement = path.clone();
	let error = crate::private_fs::private_entry_kind_after_snapshot(&path, move || {
		fs::rename(&replacement, displaced).expect("original entry should be displaced");
		private_file(&replacement, b"new", SystemTime::now());
	})
	.expect_err("entry replacement between snapshots must fail closed");

	assert!(
		error.to_string().contains("changed during metadata inspection"),
		"unexpected replacement error: {error:?}"
	);
	assert_eq!(fs::read(path).expect("replacement entry should remain"), b"new");
}

#[test]
fn private_entry_kind_rejects_disappearance_between_snapshots() {
	let temp_dir = crate::test_support::private_tempdir();
	let path =
		temp_dir.path().join(crate::DEFAULT_CACHE_ROOT).join("github/test-files/review.json");

	private_file(&path, b"old", SystemTime::now());
	let removed = path.clone();
	let error = crate::private_fs::private_entry_kind_after_snapshot(&path, move || {
		fs::remove_file(removed).expect("entry should be removed after its first snapshot");
	})
	.expect_err("entry disappearance between snapshots must fail closed");

	assert!(
		error.to_string().contains("disappeared during metadata inspection"),
		"unexpected disappearance error: {error:?}"
	);
	assert!(!path.exists());
}

#[test]
fn private_entry_kind_rejects_symlink_substitution_between_snapshots() {
	let temp_dir = crate::test_support::private_tempdir();
	let path =
		temp_dir.path().join(crate::DEFAULT_CACHE_ROOT).join("github/test-files/review.json");
	let displaced = path.with_file_name("original-review.json");

	private_file(&path, b"old", SystemTime::now());
	let substitute = path.clone();
	let target = displaced.clone();
	let error = crate::private_fs::private_entry_kind_after_snapshot(&path, move || {
		fs::rename(&substitute, &target).expect("original entry should be displaced");
		std::os::unix::fs::symlink(&target, &substitute)
			.expect("symlink substitute should be installed");
	})
	.expect_err("symlink substitution between snapshots must fail closed");

	assert!(
		error.to_string().contains("changed during metadata inspection"),
		"unexpected symlink substitution error: {error:?}"
	);
	assert!(
		fs::symlink_metadata(path)
			.expect("symlink substitute should remain")
			.file_type()
			.is_symlink()
	);
}

#[test]
fn cache_lock_serializes_writers_and_gc_deletion_revalidates_identity() {
	let temp_dir = crate::test_support::private_tempdir();
	let root = temp_dir.path().join(crate::DEFAULT_CACHE_ROOT);
	let relative = std::path::Path::new("github/test-files/review.json");
	let path = root.join(relative);
	let now = SystemTime::now();

	private_file(&path, b"old", now);

	let cache =
		crate::private_fs::PrivateCache::open_existing(&root).expect("cache root should open");
	let expected = cache
		.metadata(relative)
		.expect("cache metadata should be readable")
		.expect("cache file should exist");
	let lock = cache.lock().expect("first writer should hold the cache lock");
	let second = crate::private_fs::PrivateCache::open_existing(&root)
		.expect("second cache root should open");
	let lock_error = second.try_lock().expect_err("second writer must not acquire the lock");

	assert!(
		lock_error
			.chain()
			.find_map(|cause| cause.downcast_ref::<std::io::Error>())
			.is_some_and(|error| error.kind() == std::io::ErrorKind::WouldBlock)
	);

	let displaced = root.join("github/test-files/original.json");

	fs::rename(&path, &displaced).expect("scanned file should be displaced");
	private_file(&path, b"new", now);
	let identity_error = lock
		.remove_if_matches(relative, &expected)
		.expect_err("GC must not delete a replacement file");

	assert!(identity_error.to_string().contains("identity changed"));
	assert_eq!(fs::read(path).expect("replacement should remain"), b"new");
}
