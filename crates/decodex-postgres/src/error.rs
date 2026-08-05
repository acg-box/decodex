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
impl StoreError {
	/// Classify bootstrap without exporting database, socket, role, or credential text.
	pub fn bootstrap_failure(&self) -> BootstrapFailure {
		match self {
			Self::Pool(deadpool_postgres::PoolError::Backend(error)) =>
				classify_pool_backend(error),
			Self::Database(error) => classify_database_error(error),
			Self::UnsafeAuthority(_) => BootstrapFailure::UnsafeAuthority,
			Self::UnsafeHostPath => BootstrapFailure::UnsafeHostPath,
			Self::SocketUnavailable => BootstrapFailure::Unreachable,
			Self::Incompatible(_) => BootstrapFailure::Incompatible,
			Self::RepositoryCommitOutcomeUnknown(error) if is_authentication_error(error) =>
				BootstrapFailure::Authentication,
			Self::RepositoryCommitOutcomeUnknown(_)
			| Self::ResetCardCommitOutcomeUnknown
			| Self::ResetCardSelectionConflict
			| Self::OperationIdConflict
			| Self::ManagedRepositoryAdmissionConflict
			| Self::ManagedRepositoryAlreadyAllocated
			| Self::ManagedRepositoryAllocationConflict
			| Self::ManagedRepositoryCompareAndSwapConflict
			| Self::ManagedRepository(_) => BootstrapFailure::Unreachable,
			_ => BootstrapFailure::Unreachable,
		}
	}

	pub(crate) fn bootstrap_query_failure_category(&self) -> &'static str {
		match self {
			Self::Database(error) | Self::Pool(deadpool_postgres::PoolError::Backend(error)) =>
				query_failure_category(error),
			Self::UnsafeAuthority(_) => "authority",
			Self::Incompatible(_) => "evidence",
			Self::UnsafeHostPath => "host_path",
			Self::SocketUnavailable => "transport",
			_ => "internal",
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
	error.as_db_error().is_some_and(|database| {
		matches!(
			database.code(),
			&tokio_postgres::error::SqlState::INVALID_AUTHORIZATION_SPECIFICATION
				| &tokio_postgres::error::SqlState::INVALID_PASSWORD
		)
	})
}

fn classify_database_error(error: &tokio_postgres::Error) -> BootstrapFailure {
	if is_authentication_error(error) {
		BootstrapFailure::Authentication
	} else if error.as_db_error().is_some() {
		BootstrapFailure::Incompatible
	} else {
		BootstrapFailure::Unreachable
	}
}

fn query_failure_category(error: &tokio_postgres::Error) -> &'static str {
	let Some(database) = error.as_db_error() else {
		return "transport";
	};
	let code = database.code().code();
	match code.get(..2).unwrap_or_default() {
		"08" => "transport",
		"23" => "constraint",
		"25" | "40" => "transaction",
		"28" => "authentication",
		"3D" => "catalog",
		"42" if code == "42501" => "authorization",
		"42" => "catalog",
		"53" | "54" | "55" | "57" | "58" | "XX" => "server",
		_ => "server",
	}
}

fn classify_pool_backend(error: &tokio_postgres::Error) -> BootstrapFailure {
	#[cfg(unix)]
	if crate::socket::rejected_endpoint_failure(error)
		== Some(crate::socket::SocketConnectFailure::UnsafeAuthority)
	{
		return BootstrapFailure::UnsafeHostPath;
	}

	classify_database_error(error)
}
