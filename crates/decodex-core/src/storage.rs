use std::{
	error::Error,
	fmt::{Display, Formatter},
};

use crate::PathError;

/// Typed storage failures with no caller keys, contents, or underlying error text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageError {
	/// Owned path validation or redacted I/O failed.
	Path(PathError),
	/// Blob hash text was not canonical lowercase SHA-256.
	InvalidBlobHash,
	/// Stored bytes no longer match their content address.
	BlobIntegrityMismatch,
	/// Blob input exceeded the public bound.
	BlobTooLarge {
		/// Maximum accepted blob bytes.
		limit: usize,
	},
	/// Cache limits were zero, inconsistent, or above hard ceilings.
	InvalidCacheLimits,
	/// An unexpected filename appeared in the owned cache directory.
	InvalidCacheEntry,
}
impl Display for StorageError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Path(error) => Display::fmt(error, formatter),
			Self::InvalidBlobHash => formatter.write_str("invalid SHA-256 blob identity"),
			Self::BlobIntegrityMismatch =>
				formatter.write_str("blob integrity verification failed"),
			Self::BlobTooLarge { limit } => write!(formatter, "blob exceeds {limit} bytes"),
			Self::InvalidCacheLimits => formatter.write_str("invalid disposable-cache limits"),
			Self::InvalidCacheEntry => formatter.write_str("invalid disposable-cache entry"),
		}
	}
}

impl Error for StorageError {}

impl From<PathError> for StorageError {
	fn from(error: PathError) -> Self {
		Self::Path(error)
	}
}
