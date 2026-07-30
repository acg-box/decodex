//! Deterministic retention for disposable local Radar cache state.

use std::{
	cmp::Reverse,
	path::{Path, PathBuf},
	time::Duration,
};

use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	RETAINED_CACHE_COLLECTIONS, RadarCacheGcReport, RadarCacheGcRequest,
	prelude::{Result, eyre},
	private_fs::{
		PrivateCache, PrivateEntryKind, PrivateFileIdentity, RadarCacheLock, TEMP_FILE_PREFIX,
	},
};

const LEDGER_RELATIVE_PATH: &str = "github/radar.sqlite3";
const LEDGER_TABLES: &[(&str, &str)] = &[
	("upstream_commit", "last_seen_at"),
	("radar_review", "updated_at"),
	("artifact_link", "created_at"),
	("source_cache", "fetched_at"),
];

#[derive(Debug)]
struct CacheFile {
	relative: PathBuf,
	name: std::ffi::OsString,
	identity: PrivateFileIdentity,
}

pub(crate) fn cache_gc(request: &RadarCacheGcRequest) -> Result<RadarCacheGcReport> {
	validate_policy(request)?;
	let cache = PrivateCache::open_or_create(&request.cache_root)?;
	let lock = cache.lock()?;
	let mut report =
		RadarCacheGcReport { collections_pruned: 0, files_removed: 0, ledger_rows_removed: 0 };

	recover_stale_temporary_files(&lock, Path::new(""), &mut report)?;
	for relative in RETAINED_CACHE_COLLECTIONS {
		if *relative == crate::content_pair::PAIRS_RELATIVE_PATH {
			prune_pair_collection(&lock, Path::new(relative), request, &mut report)?;
		} else {
			prune_collection(&lock, Path::new(relative), request, &mut report)?;
		}
	}

	if lock.cache().metadata(Path::new(LEDGER_RELATIVE_PATH))?.is_some() {
		prune_ledger(&lock, request, &mut report)?;
	}

	Ok(report)
}

fn prune_pair_collection(
	lock: &RadarCacheLock,
	directory: &Path,
	request: &RadarCacheGcRequest,
	report: &mut RadarCacheGcReport,
) -> Result<()> {
	lock.cache().create_directory_all(directory)?;
	let mut pairs = Vec::new();

	for entry in lock.cache().entries(directory)? {
		if entry.kind != PrivateEntryKind::Directory {
			eyre::bail!("Radar committed content-review root contains a non-directory entry");
		}
		let relative = directory.join(&entry.name);
		let files = lock.cache().entries(&relative)?;
		if files.len() != 2
			|| files.iter().any(|file| file.kind != PrivateEntryKind::File)
			|| !files.iter().any(|file| file.name == "review.json")
			|| !files.iter().any(|file| file.name == "impact.json")
		{
			eyre::bail!("Radar committed content-review pair must contain exactly two artifacts");
		}
		crate::content_pair::validate_committed_pair_directory(lock, &relative)?;
		let mut newest = std::time::UNIX_EPOCH;
		let mut bytes = 0_u64;

		for file in files {
			let identity = file
				.identity
				.ok_or_else(|| eyre::eyre!("Radar committed pair file lacks an identity"))?;

			newest = newest.max(identity.modified());
			bytes = bytes.saturating_add(identity.size());
		}
		pairs.push((relative, newest, bytes));
	}
	pairs.sort_by_key(|(relative, modified, _)| {
		(Reverse(*modified), Reverse(relative.as_os_str().to_os_string()))
	});
	let max_age = Duration::from_secs(request.policy.max_age_days.saturating_mul(24 * 60 * 60));
	let mut retained_files = 0_usize;
	let mut retained_bytes = 0_u64;
	let mut pruned = false;

	for (relative, modified, bytes) in pairs {
		let stale = request.now.duration_since(modified).is_ok_and(|age| age > max_age);
		let exceeds_count =
			retained_files.saturating_add(2) > request.policy.max_files_per_collection;
		let exceeds_bytes =
			retained_bytes.saturating_add(bytes) > request.policy.max_bytes_per_collection;

		if stale || exceeds_count || exceeds_bytes {
			lock.remove_directory_atomic(&relative)?;
			report.files_removed += 2;
			pruned = true;
		} else {
			retained_files += 2;
			retained_bytes = retained_bytes.saturating_add(bytes);
		}
	}
	if pruned {
		report.collections_pruned += 1;
	}

	Ok(())
}

fn validate_policy(request: &RadarCacheGcRequest) -> Result<()> {
	let policy = request.policy;

	if policy.max_age_days == 0
		|| policy.max_files_per_collection == 0
		|| policy.max_bytes_per_collection == 0
		|| policy.ledger_max_rows_per_table == 0
		|| policy.ledger_max_bytes == 0
	{
		eyre::bail!("Radar cache retention limits must all be positive");
	}

	Ok(())
}

fn prune_collection(
	lock: &RadarCacheLock,
	directory: &Path,
	request: &RadarCacheGcRequest,
	report: &mut RadarCacheGcReport,
) -> Result<()> {
	lock.cache().create_directory_all(directory)?;
	let mut files = Vec::new();

	collect_collection_files(lock, directory, &mut files)?;
	files.sort_by_key(|file| (Reverse(file.identity.modified()), Reverse(file.name.clone())));

	let max_age = Duration::from_secs(request.policy.max_age_days.saturating_mul(24 * 60 * 60));
	let mut retained_files = 0_usize;
	let mut retained_bytes = 0_u64;
	let mut pruned = false;

	for file in files {
		let stale =
			request.now.duration_since(file.identity.modified()).is_ok_and(|age| age > max_age);
		let exceeds_count = retained_files >= request.policy.max_files_per_collection;
		let exceeds_bytes = retained_bytes.saturating_add(file.identity.size())
			> request.policy.max_bytes_per_collection;

		if stale || exceeds_count || exceeds_bytes {
			lock.remove_if_matches(&file.relative, &file.identity)?;
			report.files_removed += 1;
			pruned = true;
		} else {
			retained_files += 1;
			retained_bytes = retained_bytes.saturating_add(file.identity.size());
		}
	}
	if pruned {
		report.collections_pruned += 1;
	}

	Ok(())
}

fn collect_collection_files(
	lock: &RadarCacheLock,
	directory: &Path,
	files: &mut Vec<CacheFile>,
) -> Result<()> {
	for entry in lock.cache().entries(directory)? {
		let relative = directory.join(&entry.name);

		match entry.kind {
			PrivateEntryKind::Directory => collect_collection_files(lock, &relative, files)?,
			PrivateEntryKind::File => {
				let identity = entry
					.identity
					.ok_or_else(|| eyre::eyre!("Radar retained file lacks an identity"))?;

				files.push(CacheFile {
					name: relative.as_os_str().to_os_string(),
					relative,
					identity,
				});
			},
		}
	}

	Ok(())
}

fn recover_stale_temporary_files(
	lock: &RadarCacheLock,
	directory: &Path,
	report: &mut RadarCacheGcReport,
) -> Result<()> {
	for entry in lock.cache().entries(directory)? {
		let relative = directory.join(&entry.name);

		match entry.kind {
			PrivateEntryKind::Directory
				if entry.name.as_encoded_bytes().starts_with(TEMP_FILE_PREFIX.as_bytes()) =>
			{
				let count = count_files(lock, &relative)?;

				lock.remove_directory_atomic(&relative)?;
				report.files_removed += count;
			},
			PrivateEntryKind::Directory => recover_stale_temporary_files(lock, &relative, report)?,
			PrivateEntryKind::File
				if entry.name.as_encoded_bytes().starts_with(TEMP_FILE_PREFIX.as_bytes()) =>
			{
				let identity = entry
					.identity
					.ok_or_else(|| eyre::eyre!("Radar temporary cache file lacks an identity"))?;

				lock.remove_if_matches(&relative, &identity)?;
				report.files_removed += 1;
			},
			PrivateEntryKind::File => {},
		}
	}

	Ok(())
}

fn count_files(lock: &RadarCacheLock, directory: &Path) -> Result<usize> {
	let mut count = 0;

	for entry in lock.cache().entries(directory)? {
		let relative = directory.join(&entry.name);

		match entry.kind {
			PrivateEntryKind::Directory => count += count_files(lock, &relative)?,
			PrivateEntryKind::File => count += 1,
		}
	}

	Ok(count)
}

fn prune_ledger(
	lock: &RadarCacheLock,
	request: &RadarCacheGcRequest,
	report: &mut RadarCacheGcReport,
) -> Result<()> {
	let relative = Path::new(LEDGER_RELATIVE_PATH);
	let connection = crate::ledger::open_ledger_under_cache_lock(relative, lock)?;
	let cutoff = retention_cutoff(request)?;
	let row_limit = i64::try_from(request.policy.ledger_max_rows_per_table)
		.map_err(|_| eyre::eyre!("Radar ledger row limit is too large"))?;

	connection.execute_batch("BEGIN IMMEDIATE")?;
	let prune_result = prune_ledger_rows(&connection, &cutoff, row_limit);

	match prune_result {
		Ok(removed) => {
			connection.execute_batch("COMMIT")?;
			report.ledger_rows_removed += removed;
		},
		Err(error) => {
			let _ = connection.execute_batch("ROLLBACK");

			return Err(error);
		},
	}

	connection.execute_batch("VACUUM")?;
	connection.persist(lock, request.policy.ledger_max_bytes)?;

	Ok(())
}

fn prune_ledger_rows(
	connection: &rusqlite::Connection,
	cutoff: &str,
	row_limit: i64,
) -> Result<usize> {
	let mut removed = 0;

	for (table, timestamp) in LEDGER_TABLES {
		removed +=
			connection.execute(&format!("DELETE FROM {table} WHERE {timestamp} < ?1"), [cutoff])?;
		removed += connection.execute(
			&format!(
				"DELETE FROM {table} WHERE rowid NOT IN (
				   SELECT rowid FROM {table}
				   ORDER BY {timestamp} DESC, rowid DESC
				   LIMIT ?1
				 )"
			),
			[row_limit],
		)?;
	}

	Ok(removed)
}

fn retention_cutoff(request: &RadarCacheGcRequest) -> Result<String> {
	let age_seconds = request.policy.max_age_days.saturating_mul(24 * 60 * 60);
	let age_seconds = i64::try_from(age_seconds)
		.map_err(|_| eyre::eyre!("Radar cache retention age is too large"))?;
	let now = OffsetDateTime::from(request.now);
	let cutoff = now - time::Duration::seconds(age_seconds);

	Ok(cutoff.format(&Rfc3339)?)
}
