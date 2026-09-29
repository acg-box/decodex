use crate::StorageError;

/// Hard ceiling for configured disposable-cache entries.
pub const MAX_CACHE_ENTRIES: usize = 10_000;
/// Hard ceiling for configured disposable-cache bytes.
pub const MAX_CACHE_BYTES: usize = 512 * 1_024 * 1_024;
/// Hard ceiling for one disposable cache entry.
pub const MAX_CACHE_ENTRY_BYTES: usize = 16 * 1_024 * 1_024;

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
