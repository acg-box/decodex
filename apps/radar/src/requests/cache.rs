use std::{path::PathBuf, time::SystemTime};

use serde::Serialize;

use crate::{
	CACHE_MAX_AGE_DAYS, CACHE_MAX_BYTES_PER_COLLECTION, CACHE_MAX_FILES_PER_COLLECTION,
	DEFAULT_CACHE_ROOT, LEDGER_MAX_BYTES, LEDGER_MAX_ROWS_PER_TABLE,
};

#[derive(Clone, Copy, Debug)]
pub(crate) struct CacheRetentionPolicy {
	pub(crate) max_age_days: u64,
	pub(crate) max_files_per_collection: usize,
	pub(crate) max_bytes_per_collection: u64,
	pub(crate) ledger_max_rows_per_table: usize,
	pub(crate) ledger_max_bytes: u64,
}
impl Default for CacheRetentionPolicy {
	fn default() -> Self {
		Self {
			max_age_days: CACHE_MAX_AGE_DAYS,
			max_files_per_collection: CACHE_MAX_FILES_PER_COLLECTION,
			max_bytes_per_collection: CACHE_MAX_BYTES_PER_COLLECTION,
			ledger_max_rows_per_table: LEDGER_MAX_ROWS_PER_TABLE,
			ledger_max_bytes: LEDGER_MAX_BYTES,
		}
	}
}

#[derive(Debug)]
pub(crate) struct RadarCacheGcRequest {
	pub(crate) cache_root: PathBuf,
	pub(crate) policy: CacheRetentionPolicy,
	pub(crate) now: SystemTime,
}
impl Default for RadarCacheGcRequest {
	fn default() -> Self {
		Self {
			cache_root: PathBuf::from(DEFAULT_CACHE_ROOT),
			policy: CacheRetentionPolicy::default(),
			now: SystemTime::now(),
		}
	}
}

#[derive(Debug, Eq, PartialEq, Serialize)]
pub(crate) struct RadarCacheGcReport {
	pub(crate) collections_pruned: usize,
	pub(crate) files_removed: usize,
	pub(crate) ledger_rows_removed: usize,
}

#[derive(Debug)]
pub(crate) struct RadarContentV2ResetRequest {
	pub(crate) cache_root: PathBuf,
}

#[derive(Debug, Eq, PartialEq, Serialize)]
pub(crate) struct RadarContentV2ResetReport {
	pub(crate) schema: String,
	pub(crate) status: String,
	pub(crate) collections_cleared: usize,
	pub(crate) files_removed: usize,
	pub(crate) directories_removed: usize,
	pub(crate) bytes_removed: u64,
}
