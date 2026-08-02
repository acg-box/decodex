//! One-time clean-start activation for the private content-review v2 state.

use std::path::{Path, PathBuf};

use crate::{
	CACHE_MAX_BYTES_PER_COLLECTION, DEFAULT_CACHE_ROOT, RadarContentV2ResetReport,
	RadarContentV2ResetRequest,
	prelude::{Result, eyre},
	private_fs::{PrivateCache, PrivateEntryKind, PrivateTreeSnapshot, RadarCacheLock},
};

const REPORT_SCHEMA: &str = "radar_content_v2_reset/v1";
const MARKER_SCHEMA: &str = "radar_content_v2_activation/v1";
const MARKER_RELATIVE_PATH: &str = "content-v2-activation.json";
const MAX_RESET_ENTRIES: usize = 8192;
const MAX_RESET_FILES: usize = 4096;
const RESET_COLLECTIONS: [&str; 4] = [
	"github/bundles",
	"github/content-review-pairs",
	"github/content-review-staging",
	"github/control-plane-upgrades",
];

#[derive(Clone, Copy)]
struct ResetLimits {
	entries: usize,
	files: usize,
	bytes: u64,
}

impl ResetLimits {
	const PRODUCTION: Self = Self {
		entries: MAX_RESET_ENTRIES,
		files: MAX_RESET_FILES,
		bytes: CACHE_MAX_BYTES_PER_COLLECTION,
	};
}

struct ResetInventory {
	collections: Vec<(PathBuf, Option<PrivateTreeSnapshot>)>,
	files: usize,
	directories: usize,
	bytes: u64,
}

pub(crate) fn reset_content_v2(
	request: &RadarContentV2ResetRequest,
) -> Result<RadarContentV2ResetReport> {
	reset_content_v2_with_limits(request, ResetLimits::PRODUCTION)
}

fn reset_content_v2_with_limits(
	request: &RadarContentV2ResetRequest,
	limits: ResetLimits,
) -> Result<RadarContentV2ResetReport> {
	let cache_root = authoritative_reset_cache_root(request)?;
	let cache = PrivateCache::open_or_create(&cache_root)?;
	let lock = cache.lock()?;
	let marker_path = Path::new(MARKER_RELATIVE_PATH);
	let marker = marker_bytes()?;

	match lock.cache().entry_kind(marker_path)? {
		Some(PrivateEntryKind::Directory) => {
			eyre::bail!("Radar content-v2 activation marker must be a regular file")
		},
		Some(PrivateEntryKind::File) => {
			if lock.read_bounded(marker_path, marker.len() as u64)? != marker {
				eyre::bail!("Radar content-v2 activation marker is invalid");
			}
			return Ok(report("already_active", 0, 0, 0, 0));
		},
		None => {},
	}

	let inventory = inventory_reset_collections(&lock, limits)?;
	let current = inventory_reset_collections(&lock, limits)?;
	if !same_inventory(&inventory, &current) {
		eyre::bail!("Radar content-v2 reset collections changed during preflight");
	}
	let collection_count =
		inventory.collections.iter().filter(|(_, snapshot)| snapshot.is_some()).count();
	for (path, snapshot) in &inventory.collections {
		if let Some(snapshot) = snapshot {
			lock.remove_directory_atomic_if_matches(path, snapshot)?;
		}
	}
	lock.write_atomic_if_matches(marker_path, None, &marker)?;
	if lock.read_bounded(marker_path, marker.len() as u64)? != marker {
		eyre::bail!("Radar content-v2 activation marker readback mismatch");
	}

	Ok(report("reset", collection_count, inventory.files, inventory.directories, inventory.bytes))
}

#[cfg(test)]
pub(crate) fn reset_content_v2_with_test_limits(
	request: &RadarContentV2ResetRequest,
	entries: usize,
	files: usize,
	bytes: u64,
) -> Result<RadarContentV2ResetReport> {
	reset_content_v2_with_limits(request, ResetLimits { entries, files, bytes })
}

fn authoritative_reset_cache_root(request: &RadarContentV2ResetRequest) -> Result<PathBuf> {
	let expected = crate::repo_root()?.join(DEFAULT_CACHE_ROOT);
	if request.cache_root == expected {
		return Ok(expected);
	}
	#[cfg(test)]
	return Ok(request.cache_root.clone());
	#[cfg(not(test))]
	validate_reset_cache_root(&expected, &request.cache_root)?;

	#[cfg(not(test))]
	Ok(expected)
}

pub(crate) fn validate_reset_cache_root(expected: &Path, actual: &Path) -> Result<()> {
	if actual != expected {
		eyre::bail!("Radar content-v2 reset cache root must be the detected repository cache root");
	}

	Ok(())
}

fn inventory_reset_collections(
	lock: &RadarCacheLock,
	limits: ResetLimits,
) -> Result<ResetInventory> {
	let mut collections = Vec::with_capacity(RESET_COLLECTIONS.len());
	let mut files = 0_usize;
	let mut directories = 0_usize;
	let mut bytes = 0_u64;
	for collection in RESET_COLLECTIONS {
		let remaining_entries = limits
			.entries
			.checked_sub(files.saturating_add(directories))
			.ok_or_else(|| eyre::eyre!("Radar content-v2 reset entry bound was exceeded"))?;
		let remaining_files = limits
			.files
			.checked_sub(files)
			.ok_or_else(|| eyre::eyre!("Radar content-v2 reset file bound was exceeded"))?;
		let remaining_bytes = limits
			.bytes
			.checked_sub(bytes)
			.ok_or_else(|| eyre::eyre!("Radar content-v2 reset byte bound was exceeded"))?;
		let path = PathBuf::from(collection);
		let snapshot = lock.inspect_directory_tree(
			&path,
			remaining_entries,
			remaining_files,
			remaining_bytes,
		)?;
		if let Some(snapshot) = &snapshot {
			files = files
				.checked_add(snapshot.file_count())
				.ok_or_else(|| eyre::eyre!("Radar content-v2 reset file count overflowed"))?;
			directories = directories
				.checked_add(snapshot.directory_count())
				.ok_or_else(|| eyre::eyre!("Radar content-v2 reset directory count overflowed"))?;
			bytes = bytes
				.checked_add(snapshot.byte_count())
				.ok_or_else(|| eyre::eyre!("Radar content-v2 reset byte count overflowed"))?;
		}
		collections.push((path, snapshot));
	}

	Ok(ResetInventory { collections, files, directories, bytes })
}

fn same_inventory(left: &ResetInventory, right: &ResetInventory) -> bool {
	left.files == right.files
		&& left.directories == right.directories
		&& left.bytes == right.bytes
		&& left.collections == right.collections
}

fn marker_bytes() -> Result<Vec<u8>> {
	let mut bytes = serde_json::to_vec_pretty(&serde_json::json!({
		"schema": MARKER_SCHEMA,
		"status": "active"
	}))?;
	bytes.push(b'\n');

	Ok(bytes)
}

fn report(
	status: &str,
	collections_cleared: usize,
	files_removed: usize,
	directories_removed: usize,
	bytes_removed: u64,
) -> RadarContentV2ResetReport {
	RadarContentV2ResetReport {
		schema: REPORT_SCHEMA.to_owned(),
		status: status.to_owned(),
		collections_cleared,
		files_removed,
		directories_removed,
		bytes_removed,
	}
}
