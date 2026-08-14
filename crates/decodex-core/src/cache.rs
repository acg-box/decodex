use std::{
	io::ErrorKind,
	path::{Path, PathBuf},
	time::SystemTime,
};

use crate::{
	BlobHash, DecodexPaths, PathError, StorageError,
	paths::{self, IoOperation},
};

/// Hard ceiling for configured disposable-cache entries.
pub const MAX_CACHE_ENTRIES: usize = 10_000;
/// Hard ceiling for configured disposable-cache bytes.
pub const MAX_CACHE_BYTES: usize = 512 * 1_024 * 1_024;
/// Hard ceiling for one disposable cache entry.
pub const MAX_CACHE_ENTRY_BYTES: usize = 16 * 1_024 * 1_024;

const MAX_CACHE_KEY_BYTES: usize = 1_024;

/// Mechanical limits for disposable, non-authoritative cache bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CacheLimits {
	max_entries: usize,
	max_bytes: usize,
	max_entry_bytes: usize,
}
impl CacheLimits {
	/// Validate non-zero limits against hard process ceilings.
	pub fn new(
		max_entries: usize,
		max_bytes: usize,
		max_entry_bytes: usize,
	) -> Result<Self, StorageError> {
		if max_entries == 0
			|| max_entries > MAX_CACHE_ENTRIES
			|| max_bytes == 0
			|| max_bytes > MAX_CACHE_BYTES
			|| max_entry_bytes == 0
			|| max_entry_bytes > MAX_CACHE_ENTRY_BYTES
			|| max_entry_bytes > max_bytes
		{
			return Err(StorageError::InvalidCacheLimits);
		}

		Ok(Self { max_entries, max_bytes, max_entry_bytes })
	}

	/// Entry-count ceiling.
	pub const fn max_entries(self) -> usize {
		self.max_entries
	}

	/// Aggregate byte ceiling.
	pub const fn max_bytes(self) -> usize {
		self.max_bytes
	}

	/// Per-entry byte ceiling.
	pub const fn max_entry_bytes(self) -> usize {
		self.max_entry_bytes
	}
}

/// Current mechanically verified cache usage.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CacheUsage {
	/// Retained regular-file entries.
	pub entries: usize,
	/// Retained aggregate bytes.
	pub bytes: usize,
}

/// Disposable, byte-and-entry-bounded cache. It exposes no authority contract and
/// may always be cleared and rebuilt from durable-store/blob authority.
#[derive(Clone, Debug)]
pub struct BoundedCache {
	paths: DecodexPaths,
	limits: CacheLimits,
}
impl BoundedCache {
	/// Open the cache, reject unsafe file kinds, and immediately enforce both bounds.
	pub fn open(paths: DecodexPaths, limits: CacheLimits) -> Result<Self, StorageError> {
		paths.ensure_layout()?;

		let cache = Self { paths, limits };

		cache.enforce_bounds()?;

		Ok(cache)
	}

	/// Atomically write one disposable entry and evict the oldest entries until both
	/// caps hold. Caller keys are hashed and never enter filenames or errors.
	pub fn put(&self, key: &str, bytes: &[u8]) -> Result<CacheUsage, StorageError> {
		if key.is_empty() || key.len() > MAX_CACHE_KEY_BYTES {
			return Err(StorageError::InvalidCacheKey);
		}
		if bytes.len() > self.limits.max_entry_bytes {
			return Err(StorageError::CacheEntryTooLarge { limit: self.limits.max_entry_bytes });
		}

		self.enforce_bounds()?;

		let path = self.entry_path(key);

		paths::atomic_write_replace(&self.paths, &path, bytes, self.limits.max_entry_bytes)?;

		self.enforce_bounds()
	}

	/// Read one bounded disposable entry. Missing entries are ordinary cache misses.
	pub fn get(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
		if key.is_empty() || key.len() > MAX_CACHE_KEY_BYTES {
			return Err(StorageError::InvalidCacheKey);
		}

		self.enforce_bounds()?;

		let path = self.entry_path(key);

		match paths::read_private_file(&self.paths, &path, self.limits.max_entry_bytes) {
			Ok(bytes) => Ok(Some(bytes)),
			Err(PathError::Io { kind: ErrorKind::NotFound, .. }) => Ok(None),
			Err(error) => Err(error.into()),
		}
	}

	/// Delete every validated cache entry without touching durable-store or blobs.
	pub fn clear(&self) -> Result<(), StorageError> {
		paths::visit_private_files(
			&self.paths,
			&self.paths.cache_dir(),
			|path, _| -> Result<(), StorageError> {
				if paths::is_atomic_temporary_file(&path) {
					Ok(())
				} else {
					validate_cache_filename(&path)
				}
			},
		)?;
		paths::visit_private_files(
			&self.paths,
			&self.paths.cache_dir(),
			|path, _| -> Result<(), StorageError> {
				paths::remove_private_file(&self.paths, &path).map_err(Into::into)
			},
		)?;

		Ok(())
	}

	/// Re-scan disk, reject unsafe entries, and return verified bounded usage.
	pub fn usage(&self) -> Result<CacheUsage, StorageError> {
		self.enforce_bounds()
	}

	fn enforce_bounds(&self) -> Result<CacheUsage, StorageError> {
		let mut retained = Vec::new();
		let mut total_bytes = 0_usize;

		paths::visit_private_files(
			&self.paths,
			&self.paths.cache_dir(),
			|path, metadata| -> Result<(), StorageError> {
				if paths::is_atomic_temporary_file(&path) {
					paths::remove_private_file(&self.paths, &path)?;

					return Ok(());
				}

				validate_cache_filename(&path)?;

				let bytes = usize::try_from(metadata.len())
					.map_err(|_| StorageError::CacheBoundOverflow)?;

				if bytes > self.limits.max_entry_bytes || retained.len() >= MAX_CACHE_ENTRIES {
					paths::remove_private_file(&self.paths, &path)?;

					return Ok(());
				}

				total_bytes =
					total_bytes.checked_add(bytes).ok_or(StorageError::CacheBoundOverflow)?;

				let modified = metadata.modified().map_err(|error| {
					StorageError::Path(PathError::Io {
						operation: IoOperation::Inspect,
						kind: error.kind(),
					})
				})?;

				retained.push(CacheEntry { path, bytes, modified });

				Ok(())
			},
		)?;

		retained.sort_by(|left, right| {
			left.modified.cmp(&right.modified).then_with(|| left.path.cmp(&right.path))
		});

		let mut retained_entries = retained.len();

		for entry in retained {
			if retained_entries <= self.limits.max_entries && total_bytes <= self.limits.max_bytes {
				break;
			}

			paths::remove_private_file(&self.paths, &entry.path)?;

			retained_entries -= 1;
			total_bytes -= entry.bytes;
		}

		Ok(CacheUsage { entries: retained_entries, bytes: total_bytes })
	}

	fn entry_path(&self, key: &str) -> PathBuf {
		let digest = BlobHash::digest(key.as_bytes()).to_hex();

		self.paths.cache_dir().join(format!("{digest}.cache"))
	}
}

struct CacheEntry {
	path: PathBuf,
	bytes: usize,
	modified: SystemTime,
}

fn validate_cache_filename(path: &Path) -> Result<(), StorageError> {
	let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
		return Err(StorageError::InvalidCacheEntry);
	};
	let Some(hash) = name.strip_suffix(".cache") else {
		return Err(StorageError::InvalidCacheEntry);
	};

	BlobHash::parse(hash).map(|_| ()).map_err(|_| StorageError::InvalidCacheEntry)
}
