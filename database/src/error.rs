use std::{
	error::Error,
	fmt::{Display, Formatter},
};

use decodex_core::StorageError;

/// Redacted product-database failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DatabaseError {
	/// The database could not be opened or one bounded operation failed.
	Unavailable,
	/// The database schema, migration ledger, or SQLite configuration is incompatible.
	Incompatible,
	/// The fixed database path failed the owner-private path policy.
	UnsafePath,
	/// The shared store was explicitly closed.
	Closed,
	/// An optimistic, idempotency, or compare-and-swap condition did not match.
	Conflict,
	/// A requested record does not exist.
	NotFound,
	/// A requested record already exists.
	AlreadyExists,
	/// Stored bounded data cannot be decoded or violates its declared metadata.
	Corrupt,
}

impl Display for DatabaseError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(match self {
			Self::Unavailable => "product database unavailable",
			Self::Incompatible => "product database incompatible",
			Self::UnsafePath => "product database path is unsafe",
			Self::Closed => "product database is closed",
			Self::Conflict => "product database operation conflicts with current state",
			Self::NotFound => "product database record not found",
			Self::AlreadyExists => "product database record already exists",
			Self::Corrupt => "product database record is corrupt",
		})
	}
}

impl Error for DatabaseError {}

pub(crate) fn sqlite_error(_error: rusqlite::Error) -> DatabaseError {
	DatabaseError::Unavailable
}

/// Typed failures from the local product-state boundary.
#[derive(Debug)]
pub enum StoreError {
	Database(DatabaseError),
	Incompatible(String),
	UnsafeHostPath,
	IdempotencyConflict,
	OperationIdConflict,
	RevisionConflict { entity: String, expected: Option<i64>, actual: Option<i64> },
	OwnershipLost(&'static str),
	CredentialRejected,
	InvalidInput(&'static str),
	CapacityExhausted(&'static str),
	Blob(StorageError),
}

impl StoreError {
	pub fn bootstrap_failure(&self) -> BootstrapFailure {
		match self {
			Self::UnsafeHostPath | Self::Database(DatabaseError::UnsafePath) =>
				BootstrapFailure::UnsafeHostPath,
			Self::Incompatible(_) | Self::Database(DatabaseError::Incompatible) =>
				BootstrapFailure::Incompatible,
			_ => BootstrapFailure::Unreachable,
		}
	}
}

impl Display for StoreError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Database(error) => Display::fmt(error, formatter),
			Self::Incompatible(_) => formatter.write_str("incompatible product database state"),
			Self::UnsafeHostPath => formatter.write_str("unsafe product database path"),
			Self::IdempotencyConflict =>
				formatter.write_str("idempotency key reused with a different request"),
			Self::OperationIdConflict =>
				formatter.write_str("operation identity is assigned to another descriptor"),
			Self::RevisionConflict { entity, expected, actual } => write!(
				formatter,
				"revision conflict for {entity}: expected {expected:?}, actual {actual:?}"
			),
			Self::OwnershipLost(owner) => write!(formatter, "{owner} ownership was lost"),
			Self::CredentialRejected =>
				formatter.write_str("credential material is forbidden in ordinary database rows"),
			Self::InvalidInput(reason) => write!(formatter, "invalid store input: {reason}"),
			Self::CapacityExhausted(resource) => {
				write!(formatter, "{resource} capacity is exhausted")
			},
			Self::Blob(error) => write!(formatter, "content-addressed blob error: {error}"),
		}
	}
}

impl Error for StoreError {}

impl From<DatabaseError> for StoreError {
	fn from(error: DatabaseError) -> Self {
		match error {
			DatabaseError::UnsafePath => Self::UnsafeHostPath,
			DatabaseError::Incompatible => Self::Incompatible("schema".to_owned()),
			_ => Self::Database(error),
		}
	}
}

impl From<StorageError> for StoreError {
	fn from(error: StorageError) -> Self {
		Self::Blob(error)
	}
}

/// Closed startup classification retained by Doctor and installer diagnostics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BootstrapFailure {
	Authentication,
	Unreachable,
	Incompatible,
	UnsafeAuthority,
	UnsafeHostPath,
}
