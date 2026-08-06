/// Typed failures from the PostgreSQL authority boundary.
#[derive(Debug)]
pub enum StoreError {
	/// A connection could not be checked out of the bounded pool.
	Pool(deadpool_postgres::PoolError),
	/// PostgreSQL rejected or could not execute the request.
	Database(tokio_postgres::Error),
	/// The server or exact current schema is incompatible with this store.
	Incompatible(String),
	/// The steady-state identity retains authority forbidden to the runtime adapter.
	UnsafeAuthority(&'static str),
	/// The configured Unix socket endpoint is not bound to a stable local authority.
	UnsafeHostPath,
	/// The configured Unix socket endpoint does not currently exist.
	SocketUnavailable,
	/// A caller reused an idempotency key for a different logical request.
	IdempotencyConflict,
	/// One global repository operation ID was permanently assigned another descriptor.
	OperationIdConflict,
	/// An immutable repository admission differed or collided with an admitted identity/path.
	ManagedRepositoryAdmissionConflict,
	/// A repository already owns an allocation projection.
	ManagedRepositoryAlreadyAllocated,
	/// An allocation, worktree, or persisted path is already claimed.
	ManagedRepositoryAllocationConflict,
	/// Current generation/tip/fence did not match the transaction's locked facts.
	ManagedRepositoryCompareAndSwapConflict,
	/// Preparation COMMIT did not return a success acknowledgement; no receipt was minted.
	RepositoryCommitOutcomeUnknown(tokio_postgres::Error),
	/// Reset-card preparation COMMIT may have succeeded, but same-key readback was unavailable.
	ResetCardCommitOutcomeUnknown,
	/// An older operation already owns the selected public reset-card descriptor.
	ResetCardSelectionConflict,
	/// A pure managed-repository decision rejected the transaction.
	ManagedRepository(decodex_core::ManagedRepositoryError),
	/// The expected entity revision did not match authoritative state.
	RevisionConflict {
		/// Stable entity identity used by the attempted mutation.
		entity: String,
		/// Revision supplied by the caller.
		expected: Option<i64>,
		/// Current authoritative revision, or `None` when the entity is absent.
		actual: Option<i64>,
	},
	/// A lease or outbox transition was attempted by a non-owner or stale token.
	OwnershipLost(&'static str),
	/// Credential-shaped material was rejected at the ordinary-row boundary.
	CredentialRejected,
	/// A public input violated the store contract before a transaction began.
	InvalidInput(&'static str),
	/// The bounded PostgreSQL resource inventory cannot issue another durable handle yet.
	CapacityExhausted(&'static str),
	/// Content-addressed bytes were missing, tampered, unsafe, or could not be persisted.
	Blob(decodex_core::StorageError),
}

/// Closed PostgreSQL identity retained only for explicit bootstrap diagnostics.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct BootstrapDatabaseDiagnostic {
	pub(crate) sqlstate: Option<String>,
	pub(crate) statement_byte_position: Option<u64>,
}

/// Closed bootstrap identity derived once from the private store failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BootstrapFailureIdentity {
	pub(crate) classification: BootstrapFailure,
	pub(crate) category: &'static str,
}

impl StoreError {
	/// Classify bootstrap without exporting database, socket, role, or credential text.
	pub fn bootstrap_failure(&self) -> BootstrapFailure {
		self.bootstrap_failure_identity().classification
	}

	/// Return one paired closed classification and detail category for bootstrap reporting.
	pub(crate) fn bootstrap_failure_identity(&self) -> BootstrapFailureIdentity {
		match self {
			Self::Pool(deadpool_postgres::PoolError::Backend(error)) =>
				classify_pool_backend(error),
			Self::Database(error) => database_bootstrap_identity(error),
			Self::UnsafeAuthority(_) => BootstrapFailureIdentity {
				classification: BootstrapFailure::UnsafeAuthority,
				category: "authority",
			},
			Self::UnsafeHostPath => BootstrapFailureIdentity {
				classification: BootstrapFailure::UnsafeHostPath,
				category: "host_path",
			},
			Self::SocketUnavailable => BootstrapFailureIdentity {
				classification: BootstrapFailure::Unreachable,
				category: "transport",
			},
			Self::Incompatible(_) => BootstrapFailureIdentity {
				classification: BootstrapFailure::Incompatible,
				category: "evidence",
			},
			Self::RepositoryCommitOutcomeUnknown(error) if is_authentication_error(error) =>
				BootstrapFailureIdentity {
					classification: BootstrapFailure::Authentication,
					category: "authentication",
				},
			Self::RepositoryCommitOutcomeUnknown(_)
			| Self::ResetCardCommitOutcomeUnknown
			| Self::ResetCardSelectionConflict
			| Self::OperationIdConflict
			| Self::ManagedRepositoryAdmissionConflict
			| Self::ManagedRepositoryAlreadyAllocated
			| Self::ManagedRepositoryAllocationConflict
			| Self::ManagedRepositoryCompareAndSwapConflict
			| Self::ManagedRepository(_)
			| Self::Pool(_)
			| Self::IdempotencyConflict
			| Self::RevisionConflict { .. }
			| Self::OwnershipLost(_)
			| Self::CredentialRejected
			| Self::InvalidInput(_)
			| Self::CapacityExhausted(_)
			| Self::Blob(_) => BootstrapFailureIdentity {
				classification: BootstrapFailure::Unreachable,
				category: "internal",
			},
		}
	}

	pub(crate) fn bootstrap_database_diagnostic(
		&self,
		statement: Option<&str>,
	) -> BootstrapDatabaseDiagnostic {
		let error = match self {
			Self::Database(error) | Self::Pool(deadpool_postgres::PoolError::Backend(error)) =>
				error,
			_ => return BootstrapDatabaseDiagnostic::default(),
		};
		let Some(database) = error.as_db_error() else {
			return BootstrapDatabaseDiagnostic::default();
		};
		let statement_byte_position = statement.and_then(|statement| {
			let tokio_postgres::error::ErrorPosition::Original(position) = database.position()?
			else {
				return None;
			};
			original_statement_byte_position(statement, *position)
		});
		BootstrapDatabaseDiagnostic {
			sqlstate: Some(database.code().code().to_owned()),
			statement_byte_position,
		}
	}
}

impl std::error::Error for StoreError {}

impl std::fmt::Display for StoreError {
	fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Pool(error) => write!(formatter, "PostgreSQL pool error: {error}"),
			Self::Database(error) => write!(formatter, "PostgreSQL error: {error}"),
			Self::Incompatible(reason) => {
				write!(formatter, "incompatible PostgreSQL state: {reason}")
			},
			Self::UnsafeAuthority(reason) => {
				write!(formatter, "unsafe PostgreSQL runtime authority: {reason}")
			},
			Self::UnsafeHostPath => formatter.write_str("unsafe PostgreSQL Unix socket authority"),
			Self::SocketUnavailable => formatter.write_str("PostgreSQL Unix socket is unavailable"),
			Self::IdempotencyConflict =>
				formatter.write_str("idempotency key reused with a different request"),
			Self::OperationIdConflict => formatter
				.write_str("repository operation ID is permanently assigned to another descriptor"),
			Self::ManagedRepositoryAdmissionConflict => formatter
				.write_str("managed-repository admission conflicts with immutable authority"),
			Self::ManagedRepositoryAlreadyAllocated =>
				formatter.write_str("managed repository is already allocated"),
			Self::ManagedRepositoryAllocationConflict => formatter
				.write_str("managed-repository allocation identity or path is already claimed"),
			Self::ManagedRepositoryCompareAndSwapConflict =>
				formatter.write_str("managed-repository generation, tip, or fence changed"),
			Self::RepositoryCommitOutcomeUnknown(error) => write!(
				formatter,
				"managed-repository preparation COMMIT outcome is unknown; no dispatch receipt was minted: {error}"
			),
			Self::ResetCardCommitOutcomeUnknown => formatter.write_str(
				"reset-card preparation COMMIT outcome is unknown after same-key readback",
			),
			Self::ResetCardSelectionConflict =>
				formatter.write_str("an older operation owns the selected reset card"),
			Self::ManagedRepository(error) =>
				write!(formatter, "managed-repository decision rejected: {error}"),
			Self::RevisionConflict { entity, expected, actual } => write!(
				formatter,
				"revision conflict for {entity}: expected {expected:?}, actual {actual:?}"
			),
			Self::OwnershipLost(owner) => write!(formatter, "{owner} ownership was lost"),
			Self::CredentialRejected =>
				formatter.write_str("credential material is forbidden in ordinary PostgreSQL rows"),
			Self::InvalidInput(reason) => write!(formatter, "invalid store input: {reason}"),
			Self::CapacityExhausted(resource) =>
				write!(formatter, "{resource} capacity is exhausted"),
			Self::Blob(error) => write!(formatter, "content-addressed blob error: {error}"),
		}
	}
}

impl From<decodex_core::StorageError> for StoreError {
	fn from(error: decodex_core::StorageError) -> Self {
		Self::Blob(error)
	}
}

impl From<deadpool_postgres::PoolError> for StoreError {
	fn from(error: deadpool_postgres::PoolError) -> Self {
		Self::Pool(error)
	}
}

impl From<deadpool_postgres::BuildError> for StoreError {
	fn from(error: deadpool_postgres::BuildError) -> Self {
		Self::Incompatible(error.to_string())
	}
}

impl From<tokio_postgres::Error> for StoreError {
	fn from(error: tokio_postgres::Error) -> Self {
		if error.code().is_some_and(|code| code.code() == "DX001") {
			Self::IdempotencyConflict
		} else if error.as_db_error().is_some_and(|database| {
			database.constraint() == Some("account_routing_universe_complete")
		}) {
			Self::Incompatible("stored account routing universe is incomplete".into())
		} else if error.as_db_error().is_some_and(|database| {
			database.code() == &tokio_postgres::error::SqlState::CHECK_VIOLATION
				&& database.constraint().is_some_and(|name| name.contains("no_credentials"))
		}) {
			Self::CredentialRejected
		} else if error.as_db_error().is_some_and(|database| database.code().code() == "54000") {
			Self::CapacityExhausted("history cursor")
		} else {
			Self::Database(error)
		}
	}
}

impl From<decodex_core::ManagedRepositoryError> for StoreError {
	fn from(error: decodex_core::ManagedRepositoryError) -> Self {
		Self::ManagedRepository(error)
	}
}

/// Stable classification of failures during explicit adapter bootstrap.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BootstrapFailure {
	/// PostgreSQL rejected the supplied credential.
	Authentication,
	/// The configured endpoint could not establish a usable connection.
	Unreachable,
	/// Server, extension, checksum, or exact current schema is incompatible.
	Incompatible,
	/// The configured runtime identity retains forbidden database authority.
	UnsafeAuthority,
	/// The Unix socket path or peer did not match the pinned local authority.
	UnsafeHostPath,
}

fn is_authentication_error(error: &tokio_postgres::Error) -> bool {
	error.as_db_error().is_some_and(|database| is_authentication_sqlstate(database.code().code()))
}

fn is_authentication_sqlstate(sqlstate: &str) -> bool {
	matches!(sqlstate, "28000" | "28P01")
}

fn database_bootstrap_identity(error: &tokio_postgres::Error) -> BootstrapFailureIdentity {
	database_bootstrap_identity_from_sqlstate(
		error.as_db_error().map(|database| database.code().code()),
	)
}

fn database_bootstrap_identity_from_sqlstate(sqlstate: Option<&str>) -> BootstrapFailureIdentity {
	let classification = match sqlstate {
		Some(code) if is_authentication_sqlstate(code) => BootstrapFailure::Authentication,
		Some(_) => BootstrapFailure::Incompatible,
		None => BootstrapFailure::Unreachable,
	};
	let category = match sqlstate.and_then(|code| code.get(..2)) {
		None | Some("08") => "transport",
		Some("23") => "constraint",
		Some("25") | Some("40") => "transaction",
		Some("28") => "authentication",
		Some("3D") => "catalog",
		Some("42") if sqlstate == Some("42501") => "authorization",
		Some("42") => "catalog",
		Some("53") | Some("54") | Some("55") | Some("57") | Some("58") | Some("XX") => "server",
		Some(_) => "server",
	};
	BootstrapFailureIdentity { classification, category }
}

fn classify_pool_backend(error: &tokio_postgres::Error) -> BootstrapFailureIdentity {
	#[cfg(unix)]
	if crate::socket::rejected_endpoint_failure(error)
		== Some(crate::socket::SocketConnectFailure::UnsafeAuthority)
	{
		return BootstrapFailureIdentity {
			classification: BootstrapFailure::UnsafeHostPath,
			category: "host_path",
		};
	}

	database_bootstrap_identity(error)
}

fn original_statement_byte_position(statement: &str, character_position: u32) -> Option<u64> {
	let character_index = usize::try_from(character_position).ok()?.checked_sub(1)?;
	let byte_index =
		statement.char_indices().nth(character_index).map(|(index, _)| index).or_else(|| {
			(statement.chars().count() == character_index).then_some(statement.len())
		})?;
	u64::try_from(byte_index.checked_add(1)?).ok()
}

#[cfg(test)]
mod tests {
	use super::{
		BootstrapFailure, BootstrapFailureIdentity, StoreError,
		database_bootstrap_identity_from_sqlstate, original_statement_byte_position,
	};

	#[test]
	fn bootstrap_identity_keeps_unsafe_host_path_and_database_transport_consistent() {
		assert_eq!(
			StoreError::UnsafeHostPath.bootstrap_failure_identity(),
			BootstrapFailureIdentity {
				classification: BootstrapFailure::UnsafeHostPath,
				category: "host_path",
			}
		);
		assert_eq!(
			database_bootstrap_identity_from_sqlstate(Some("08006")),
			BootstrapFailureIdentity {
				classification: BootstrapFailure::Incompatible,
				category: "transport",
			}
		);
	}

	#[test]
	fn postgres_character_position_maps_to_one_based_original_statement_byte_position() {
		assert_eq!(original_statement_byte_position("SELECT x", 1), Some(1));
		assert_eq!(original_statement_byte_position("SELECT x", 8), Some(8));
		assert_eq!(original_statement_byte_position("a\u{e9}z", 3), Some(4));
		assert_eq!(original_statement_byte_position("a\u{e9}z", 4), Some(5));
		assert_eq!(original_statement_byte_position("SELECT x", 0), None);
		assert_eq!(original_statement_byte_position("SELECT x", 10), None);
	}
}
