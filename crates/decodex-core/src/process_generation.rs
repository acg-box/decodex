//! Durable, mechanism-neutral authority for one supervised provider-process generation.
//!
//! These values carry exact positive identities and closed state labels. They do not provide
//! routing, process adoption, provider dispatch, retry, or negative-observation authority.

use std::{
	error::Error,
	fmt::{Display, Formatter},
	io::ErrorKind,
	str,
};

use sha2::{Digest as _, Sha256};

use crate::{
	AccountId, ConfigError, CredentialBinding, DecodexPaths, PathError, ServerIdentity, paths,
};

/// Maximum bytes in one exact operating-system boot or process-start identity.
pub const MAX_PROCESS_IDENTITY_BYTES: usize = 128;
/// Maximum bytes in one immutable attested launch-manifest identity.
pub const MAX_PROCESS_RUNNER_IDENTITY_BYTES: usize = 128;
const EXECUTION_AUTHORIZATION_SCHEMA: &str = "decodex/process-execution-authorization/1";
const MAX_EXECUTION_AUTHORIZATION_BYTES: usize = 192;

/// Canonical identity of one durable process generation.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ProcessGenerationId(String);
impl ProcessGenerationId {
	/// Parse one canonical lower-case UUID.
	pub fn new(value: impl Into<String>) -> Result<Self, ProcessGenerationError> {
		let value = value.into();

		if !is_canonical_uuid(&value) {
			return Err(ProcessGenerationError::InvalidGenerationId);
		}

		Ok(Self(value))
	}

	/// Borrow the canonical identity.
	pub fn as_str(&self) -> &str {
		&self.0
	}
}
impl Display for ProcessGenerationId {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(&self.0)
	}
}

/// Canonical identity of one accepted external execution epoch.
///
/// A database row is not sufficient authority for this value. The backup and restore gate must
/// supply the matching authorization digest from outside the restored database.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ProcessExecutionEpochId(String);
impl ProcessExecutionEpochId {
	/// Parse one canonical lower-case UUID.
	pub fn new(value: impl Into<String>) -> Result<Self, ProcessGenerationError> {
		let value = value.into();

		if !is_canonical_uuid(&value) {
			return Err(ProcessGenerationError::InvalidExecutionEpochId);
		}

		Ok(Self(value))
	}

	/// Borrow the canonical identity.
	pub fn as_str(&self) -> &str {
		&self.0
	}
}
impl Display for ProcessExecutionEpochId {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(&self.0)
	}
}

/// Canonical identity of one positive generation-bound death receipt.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ProcessDeathEvidenceId(String);
impl ProcessDeathEvidenceId {
	/// Parse one canonical lower-case UUID.
	pub fn new(value: impl Into<String>) -> Result<Self, ProcessGenerationError> {
		let value = value.into();

		if !is_canonical_uuid(&value) {
			return Err(ProcessGenerationError::InvalidDeathEvidenceId);
		}

		Ok(Self(value))
	}

	/// Borrow the canonical identity.
	pub fn as_str(&self) -> &str {
		&self.0
	}
}
impl Display for ProcessDeathEvidenceId {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(&self.0)
	}
}

/// Exact external restore-gate authority required for one new generation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessExecutionAuthorization {
	/// Accepted execution epoch identity.
	pub epoch_id: ProcessExecutionEpochId,
	/// Lower-case SHA-256 of the accepted external authorization receipt.
	pub authorization_digest: String,
}
impl ProcessExecutionAuthorization {
	/// Validate one externally supplied execution authorization.
	pub fn new(
		epoch_id: ProcessExecutionEpochId,
		authorization_digest: impl Into<String>,
	) -> Result<Self, ProcessGenerationError> {
		let authorization_digest = authorization_digest.into();

		if !is_sha256(&authorization_digest) {
			return Err(ProcessGenerationError::InvalidExecutionAuthorization);
		}

		Ok(Self { epoch_id, authorization_digest })
	}

	/// Load the fixed owner-only external launch capability.
	pub fn load(paths: &DecodexPaths) -> Result<Self, ConfigError> {
		let bytes = paths::read_private_file(
			paths,
			&paths.process_execution_authorization_file(),
			MAX_EXECUTION_AUTHORIZATION_BYTES,
		)?;
		let text = str::from_utf8(&bytes).map_err(|_| ConfigError::Malformed)?;
		let mut lines = text.lines();
		if lines.next() != Some(EXECUTION_AUTHORIZATION_SCHEMA) {
			return Err(ConfigError::Malformed);
		}
		let epoch_id = lines.next().ok_or(ConfigError::Malformed)?;
		let digest = lines.next().ok_or(ConfigError::Malformed)?;
		if lines.next().is_some() {
			return Err(ConfigError::Malformed);
		}
		Self::new(
			ProcessExecutionEpochId::new(epoch_id.to_owned())
				.map_err(|_| ConfigError::Malformed)?,
			digest.to_owned(),
		)
		.map_err(|_| ConfigError::Malformed)
	}

	/// Load an existing capability or atomically create one for an offline installer cutover.
	pub fn load_or_create(paths: &DecodexPaths) -> Result<Self, ConfigError> {
		paths.ensure_layout()?;
		match Self::load(paths) {
			Ok(value) => return Ok(value),
			Err(ConfigError::Path(PathError::Io { kind: ErrorKind::NotFound, .. })) => {},
			Err(error) => return Err(error),
		}
		let epoch = ServerIdentity::generate()?;
		let nonce = ServerIdentity::generate()?;
		let mut hasher = Sha256::new();
		hasher.update(EXECUTION_AUTHORIZATION_SCHEMA.as_bytes());
		hasher.update([0]);
		hasher.update(epoch.as_str().as_bytes());
		hasher.update([0]);
		hasher.update(nonce.as_str().as_bytes());
		let authorization_digest =
			hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect::<String>();
		let authorization = Self::new(
			ProcessExecutionEpochId::new(epoch.as_str().to_owned())
				.map_err(|_| ConfigError::Malformed)?,
			authorization_digest,
		)
		.map_err(|_| ConfigError::Malformed)?;
		let body = format!(
			"{EXECUTION_AUTHORIZATION_SCHEMA}\n{}\n{}\n",
			authorization.epoch_id.as_str(),
			authorization.authorization_digest,
		);
		match paths::atomic_write_new(
			paths,
			&paths.process_execution_authorization_file(),
			body.as_bytes(),
			MAX_EXECUTION_AUTHORIZATION_BYTES,
		) {
			Ok(()) => Ok(authorization),
			Err(PathError::AlreadyExists) => Self::load(paths),
			Err(error) => Err(error.into()),
		}
	}
}

/// Immutable exact launch-manifest identity established before process creation.
///
/// The ProcessSupervisor derives this value from one opaque attested launch authority. A caller
/// cannot pair it with an unrelated executable, command, environment, account, or capability.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ProcessRunnerIdentity(String);
impl ProcessRunnerIdentity {
	/// Parse the canonical opaque SHA-256 runner identity.
	pub fn new(value: impl Into<String>) -> Result<Self, ProcessGenerationError> {
		let value = value.into();
		let digest = value.strip_prefix("sha256:");

		if value.len() > MAX_PROCESS_RUNNER_IDENTITY_BYTES || !digest.is_some_and(is_sha256) {
			return Err(ProcessGenerationError::InvalidRunnerIdentity);
		}

		Ok(Self(value))
	}

	/// Borrow the exact runner identity.
	pub fn as_str(&self) -> &str {
		&self.0
	}
}

/// Exact current host boot identity.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ProcessBootIdentity(String);
impl ProcessBootIdentity {
	/// Parse one bounded, printable, mechanism-owned identity.
	pub fn new(value: impl Into<String>) -> Result<Self, ProcessGenerationError> {
		let value = value.into();

		if !is_bounded_identity(&value) {
			return Err(ProcessGenerationError::InvalidBootIdentity);
		}

		Ok(Self(value))
	}

	/// Borrow the exact identity.
	pub fn as_str(&self) -> &str {
		&self.0
	}
}

/// Exact operating-system process-start identity within one boot.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ProcessStartIdentity(String);
impl ProcessStartIdentity {
	/// Parse one bounded, printable, mechanism-owned identity.
	pub fn new(value: impl Into<String>) -> Result<Self, ProcessGenerationError> {
		let value = value.into();

		if !is_bounded_identity(&value) {
			return Err(ProcessGenerationError::InvalidProcessStartIdentity);
		}

		Ok(Self(value))
	}

	/// Borrow the exact identity.
	pub fn as_str(&self) -> &str {
		&self.0
	}
}

/// Durable generation state. Account quarantine is derived from nonterminal states.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessGenerationState {
	/// Fenced intent exists before process creation.
	Starting,
	/// Exact process identity and application readiness are durable.
	Ready,
	/// The supervisor requested exact termination.
	Stopping,
	/// Positive generation-bound evidence proves death.
	Dead,
	/// The supervisor lost authority and positive death is not yet proved.
	DeathUnknown,
}
impl ProcessGenerationState {
	/// Return the canonical durable-store label.
	pub const fn as_sql(self) -> &'static str {
		match self {
			Self::Starting => "starting",
			Self::Ready => "ready",
			Self::Stopping => "stopping",
			Self::Dead => "dead",
			Self::DeathUnknown => "death_unknown",
		}
	}

	/// True when this state prevents a replacement for the same account.
	pub const fn quarantines_account(self) -> bool {
		!matches!(self, Self::Dead)
	}
}

/// Exact ProcessGeneration lifetime mechanism bound before the child starts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessControlKind {
	/// The supervisor privately owns stdio. EOF is a best-effort request, not death proof.
	StdioOnlyBestEffortEof,
	/// Reserved for an accepted Linux profile that also applies an uncatchable parent-death signal.
	ParentDeathSignalAndStdioEof,
}
impl ProcessControlKind {
	/// Return the canonical durable-store label.
	pub const fn as_sql(self) -> &'static str {
		match self {
			Self::StdioOnlyBestEffortEof => "stdio_only_best_effort_eof",
			Self::ParentDeathSignalAndStdioEof => "parent_death_signal_and_stdio_eof",
		}
	}
}

/// Operating-system isolation scope owned by the generation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessIsolationKind {
	/// The child is the leader of a new session and process group.
	Session,
}
impl ProcessIsolationKind {
	/// Return the canonical durable-store label.
	pub const fn as_sql(self) -> &'static str {
		match self {
			Self::Session => "session",
		}
	}
}

/// Closed reason why a nonterminal generation lost supervision authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessAuthorityLossReason {
	/// A new daemon restored or restarted without the original live child handle.
	SupervisorRestarted,
	/// Spawn returned a child but exact identity could not be persisted.
	IdentityPersistenceFailed,
	/// Ready-state persistence failed after process identity was bound.
	ReadinessPersistenceFailed,
	/// Exact shutdown did not produce positive death evidence.
	TerminationUnproved,
	/// The supervising control channel or owned child handle was lost.
	ControlAuthorityLost,
}
impl ProcessAuthorityLossReason {
	/// Return the canonical durable-store label.
	pub const fn as_sql(self) -> &'static str {
		match self {
			Self::SupervisorRestarted => "supervisor_restarted",
			Self::IdentityPersistenceFailed => "identity_persistence_failed",
			Self::ReadinessPersistenceFailed => "readiness_persistence_failed",
			Self::TerminationUnproved => "termination_unproved",
			Self::ControlAuthorityLost => "control_authority_lost",
		}
	}
}

/// Closed positive evidence kind. It intentionally has no absence, timeout, or lease variant.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessDeathEvidenceKind {
	/// Spawn failed before the operating system returned any child identity.
	SpawnNotCreated,
	/// The original supervisor positively reaped the exact child and completed owned cleanup.
	OwnedChildExit,
	/// A Linux pidfd attached to the exact persisted identity reported process exit.
	LinuxPidfdExit,
	/// Exact macOS `NOTE_EXIT` was followed by process-group quiescence.
	MacosKqueueExitAndGroupQuiescence,
	/// Exact-identity termination was followed by positive owned-child exit and group cleanup.
	ExactTerminationExit,
	/// The current boot differs from the generation's intended boot.
	PriorBootEnded,
}
impl ProcessDeathEvidenceKind {
	/// Return the canonical durable-store label.
	pub const fn as_sql(self) -> &'static str {
		match self {
			Self::SpawnNotCreated => "spawn_not_created",
			Self::OwnedChildExit => "owned_child_exit",
			Self::LinuxPidfdExit => "linux_pidfd_exit",
			Self::MacosKqueueExitAndGroupQuiescence => "macos_kqueue_exit_and_group_quiescence",
			Self::ExactTerminationExit => "exact_termination_exit",
			Self::PriorBootEnded => "prior_boot_ended",
		}
	}
}

/// Complete process identity persisted immediately after spawn.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessIdentity {
	/// Host boot in which the process started.
	pub boot_id: ProcessBootIdentity,
	/// Positive process identifier.
	pub process_id: u32,
	/// Exact process-start identity within `boot_id`.
	pub process_start_id: ProcessStartIdentity,
	/// Process group led by this exact child.
	pub process_group_id: u32,
	/// Session led by this exact child.
	pub session_id: u32,
}
impl ProcessIdentity {
	/// Validate one exact session-leader identity.
	pub fn new(
		boot_id: ProcessBootIdentity,
		process_id: u32,
		process_start_id: ProcessStartIdentity,
		process_group_id: u32,
		session_id: u32,
	) -> Result<Self, ProcessGenerationError> {
		if process_id == 0 || process_group_id != process_id || session_id != process_id {
			return Err(ProcessGenerationError::InvalidProcessIdentity);
		}

		Ok(Self { boot_id, process_id, process_start_id, process_group_id, session_id })
	}
}

/// Durable pre-spawn intent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessGenerationIntent {
	/// New generation identity.
	pub generation_id: ProcessGenerationId,
	/// Account exclusively bound to the child.
	pub account_id: AccountId,
	/// Immutable launch-manifest identity derived by the opaque launch authority.
	pub runner_identity: ProcessRunnerIdentity,
	/// Current boot observed before spawn.
	pub intended_boot_id: ProcessBootIdentity,
	/// Exact profile-derived lifetime mechanism.
	pub control_kind: ProcessControlKind,
	/// Exact process-group or session isolation mechanism.
	pub isolation_kind: ProcessIsolationKind,
	/// External restore-gate authority. Database readback alone cannot construct it.
	pub execution_authorization: ProcessExecutionAuthorization,
}

/// Immutable account/store/provider/callback facts bound to one process generation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessGenerationAccountBinding {
	/// Exact account registry revision observed before the durable spawn fence.
	pub account_revision: i64,
	/// Exact canonical store version, fingerprint, and provider identity.
	pub credential: CredentialBinding,
	/// Exact generated/live refresh callback capability profile.
	pub refresh_callback_profile_sha256: String,
}
impl ProcessGenerationAccountBinding {
	/// Construct a complete non-secret binding. Partial or unhashed capability facts are rejected.
	pub fn new(
		account_revision: i64,
		credential: CredentialBinding,
		refresh_callback_profile_sha256: impl Into<String>,
	) -> Result<Self, ProcessGenerationError> {
		let refresh_callback_profile_sha256 = refresh_callback_profile_sha256.into();
		if account_revision < 1 {
			return Err(ProcessGenerationError::InvalidAccountRevision);
		}
		if !is_sha256(&refresh_callback_profile_sha256) {
			return Err(ProcessGenerationError::InvalidCallbackProfile);
		}
		Ok(Self { account_revision, credential, refresh_callback_profile_sha256 })
	}
}

/// Existing generation projection paired with its immutable V27 account binding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundProcessGeneration {
	/// Persisted process-generation projection.
	pub generation: ProcessGeneration,
	/// Immutable account binding, when the generation uses account authority.
	pub account_binding: Option<ProcessGenerationAccountBinding>,
}

/// One exact persisted generation projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessGeneration {
	/// Durable generation identity.
	pub generation_id: ProcessGenerationId,
	/// Bound account.
	pub account_id: AccountId,
	/// External execution epoch.
	pub execution_epoch_id: ProcessExecutionEpochId,
	/// Immutable launch-manifest identity.
	pub runner_identity: ProcessRunnerIdentity,
	/// Intended boot captured before spawn.
	pub intended_boot_id: ProcessBootIdentity,
	/// Exact profile-derived lifetime mechanism.
	pub control_kind: ProcessControlKind,
	/// Isolation mechanism.
	pub isolation_kind: ProcessIsolationKind,
	/// Exact process identity, absent only before identity persistence.
	pub process_identity: Option<ProcessIdentity>,
	/// Current durable state.
	pub state: ProcessGenerationState,
	/// Closed reason for a `death_unknown` projection.
	pub authority_loss_reason: Option<ProcessAuthorityLossReason>,
	/// Positive death receipt bound to a `dead` projection.
	pub death_evidence_id: Option<ProcessDeathEvidenceId>,
	/// Positive current revision.
	pub revision: i64,
	/// durable-store-authored creation instant in Unix microseconds.
	pub created_at_micros: i64,
	/// durable-store-authored last-transition instant in Unix microseconds.
	pub updated_at_micros: i64,
}

/// One positive death receipt supplied to the sole generation writer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessDeathEvidence {
	/// Unique append-only evidence identity.
	pub evidence_id: ProcessDeathEvidenceId,
	/// Exact generation proved dead.
	pub generation_id: ProcessGenerationId,
	/// Positive evidence mechanism.
	pub kind: ProcessDeathEvidenceKind,
	/// Current boot at observation time.
	pub observed_boot_id: ProcessBootIdentity,
	/// Exact persisted process identity for process-exit evidence.
	pub process_identity: Option<ProcessIdentity>,
	/// Lower-case SHA-256 of the kernel witness or owned-wait receipt.
	pub witness_digest: String,
}
impl ProcessDeathEvidence {
	/// Validate the evidence shape without treating a negative observation as proof.
	pub fn new(
		evidence_id: ProcessDeathEvidenceId,
		generation_id: ProcessGenerationId,
		kind: ProcessDeathEvidenceKind,
		observed_boot_id: ProcessBootIdentity,
		process_identity: Option<ProcessIdentity>,
		witness_digest: impl Into<String>,
	) -> Result<Self, ProcessGenerationError> {
		let witness_digest = witness_digest.into();
		let identity_shape_valid = match kind {
			ProcessDeathEvidenceKind::SpawnNotCreated
			| ProcessDeathEvidenceKind::PriorBootEnded => process_identity.is_none(),
			ProcessDeathEvidenceKind::OwnedChildExit => true,
			ProcessDeathEvidenceKind::LinuxPidfdExit
			| ProcessDeathEvidenceKind::MacosKqueueExitAndGroupQuiescence
			| ProcessDeathEvidenceKind::ExactTerminationExit => process_identity.is_some(),
		};

		if !is_sha256(&witness_digest) || !identity_shape_valid {
			return Err(ProcessGenerationError::InvalidDeathEvidence);
		}

		Ok(Self {
			evidence_id,
			generation_id,
			kind,
			observed_boot_id,
			process_identity,
			witness_digest,
		})
	}
}

/// Derived per-account quarantine. It is not another durable writer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessAccountQuarantine {
	/// Affected account only.
	pub account_id: AccountId,
	/// Exact unresolved generation.
	pub generation_id: ProcessGenerationId,
	/// Nonterminal state that prevents replacement.
	pub state: ProcessGenerationState,
	/// True when exact process identity was persisted before authority loss.
	pub has_process_identity: bool,
}

/// Closed process-generation validation error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessGenerationError {
	/// Generation identity was not a canonical UUID.
	InvalidGenerationId,
	/// Execution epoch identity was not a canonical UUID.
	InvalidExecutionEpochId,
	/// Death-evidence identity was not a canonical UUID.
	InvalidDeathEvidenceId,
	/// External execution authorization was malformed.
	InvalidExecutionAuthorization,
	/// Runner identity was not the exact SHA-256 form.
	InvalidRunnerIdentity,
	/// Boot identity was empty, unbounded, or contained control bytes.
	InvalidBootIdentity,
	/// Process-start identity was empty, unbounded, or contained control bytes.
	InvalidProcessStartIdentity,
	/// PID, process group, or session identity was invalid.
	InvalidProcessIdentity,
	/// Account registry revisions start at one.
	InvalidAccountRevision,
	/// The callback profile was not a canonical SHA-256 digest.
	InvalidCallbackProfile,
	/// Death evidence did not match its closed positive shape.
	InvalidDeathEvidence,
}
impl Error for ProcessGenerationError {}
impl Display for ProcessGenerationError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		write!(formatter, "{self:?}")
	}
}

fn is_sha256(value: &str) -> bool {
	value.len() == 64
		&& value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn is_canonical_uuid(value: &str) -> bool {
	value.len() == 36
		&& value.bytes().enumerate().all(|(index, byte)| match index {
			8 | 13 | 18 | 23 => byte == b'-',
			_ => byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte),
		})
}

fn is_bounded_identity(value: &str) -> bool {
	!value.is_empty()
		&& value.len() <= MAX_PROCESS_IDENTITY_BYTES
		&& value.bytes().all(|byte| byte.is_ascii_graphic())
}
