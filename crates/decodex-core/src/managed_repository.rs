//! Pure, mechanism-neutral managed-repository decisions.
//!
//! Every fact, view, and decision in this module is forgeable data. None establishes durable
//! freshness, permits an external effect, or authorizes persistence. durable-store must load current
//! facts inside its transaction and owns checkpoints, global operation assignment, append-only
//! evidence, CAS, transaction completeness, restart loads, and private post-commit dispatch.
//!
//! Admission observations support exact comparison of path, layout, external identity, base, and
//! currently observable object metadata. They do not prove uninterrupted historical object
//! identity while the daemon is stopped: V1 cannot distinguish an exact delete/recreate whose
//! device, inode, type, UID, and mode are reused. Hostile same-UID operation or a requirement for
//! stronger historical identity is therefore a V1 falsifier, not authority supplied by these types.

use std::{
	error::Error,
	ffi::OsStr,
	fmt::{Debug, Display, Formatter},
	path::{Component, Path, PathBuf},
};

use sha2::{Digest as _, Sha256};

use crate::ProjectId;

/// Maximum bytes in a bounded managed-repository identity or revision.
pub const MAX_MANAGED_REPOSITORY_VALUE_BYTES: usize = 256;
/// Maximum UTF-8 bytes in a persisted absolute server-host path.
pub const MAX_MANAGED_REPOSITORY_PATH_BYTES: usize = 4_096;
/// Maximum persisted objects/components in one admission descriptor.
pub const MAX_REPOSITORY_ADMISSION_OBSERVATIONS: usize = 256;
/// Maximum semantic registration roles attached to one observed object.
pub const MAX_REPOSITORY_OBSERVATION_ROLES: usize = 8;
/// Maximum UTF-8 bytes in one canonical linked-worktree registration identity.
pub const MAX_REPOSITORY_REGISTRATION_ID_BYTES: usize = 128;
/// Maximum UTF-8 bytes in one canonical commit message.
pub const MAX_REPOSITORY_COMMIT_MESSAGE_BYTES: usize = 16_384;

macro_rules! uuid_id {
	($(#[$meta:meta])* $name:ident, $invalid:ident) => {
		$(#[$meta])*
		#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
		pub struct $name(String);
		impl $name {
			/// Parse one canonical lowercase RFC 9562 UUID version 4 identity.
			pub fn new(value: impl Into<String>) -> Result<Self, ManagedRepositoryError> {
				let value = value.into();
				if !is_canonical_uuid_v4(&value) {
					return Err(ManagedRepositoryError::$invalid);
				}
				Ok(Self(value))
			}

			/// Borrow the canonical identity.
			pub fn as_str(&self) -> &str {
				&self.0
			}
		}
		impl Display for $name {
			fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
				formatter.write_str(&self.0)
			}
		}
	};
}

macro_rules! bounded_value {
	($(#[$meta:meta])* $name:ident, $invalid:ident) => {
		$(#[$meta])*
		#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
		pub struct $name(String);
		impl $name {
			/// Parse one nonempty bounded canonical opaque value.
			pub fn new(value: impl Into<String>) -> Result<Self, ManagedRepositoryError> {
				let value = value.into();
				if value.is_empty()
					|| value.len() > MAX_MANAGED_REPOSITORY_VALUE_BYTES
					|| value.trim() != value
					|| value.chars().any(char::is_control)
				{
					return Err(ManagedRepositoryError::$invalid);
				}
				Ok(Self(value))
			}

			/// Borrow the canonical opaque value.
			pub fn as_str(&self) -> &str {
				&self.0
			}
		}
		impl Display for $name {
			fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
				formatter.write_str(&self.0)
			}
		}
	};
}

uuid_id!(/// Stable managed-repository identity.
	ManagedRepositoryId, InvalidRepositoryId);
uuid_id!(/// Stable allocation identity.
	RepositoryAllocationId, InvalidAllocationId);
uuid_id!(/// Stable managed-worktree identity.
	ManagedWorktreeId, InvalidWorktreeId);
uuid_id!(/// Globally single-assigned repository-operation identity.
	RepositoryOperationId, InvalidOperationId);
uuid_id!(/// Immutable read-only evidence identity.
	RepositoryEvidenceId, InvalidEvidenceId);
uuid_id!(/// durable-store-issued aggregate authority-event identity carried only as data.
	RepositoryAuthorityTip, InvalidAuthorityTip);

bounded_value!(/// Opaque external identity fixed at repository admission.
	AdmittedRepositoryIdentity, InvalidAdmittedIdentity);
bounded_value!(/// Opaque exact repository content revision.
	RepositoryContentRevision, InvalidContentRevision);
bounded_value!(/// Exact canonical repository reference updated by Commit.
	RepositoryReferenceName, InvalidReferenceName);
bounded_value!(/// Bounded canonical commit actor name.
	RepositoryCommitActorName, InvalidCommitActorName);
bounded_value!(/// Bounded canonical commit actor email.
	RepositoryCommitActorEmail, InvalidCommitActorEmail);

/// Bounded canonical Git linked-worktree registration identity.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RepositoryRegistrationId(String);
impl RepositoryRegistrationId {
	/// Parse one nonempty bounded registration identity valid as exactly one path component.
	pub fn new(value: impl Into<String>) -> Result<Self, ManagedRepositoryError> {
		let value = value.into();
		if value.is_empty()
			|| value.len() > MAX_REPOSITORY_REGISTRATION_ID_BYTES
			|| matches!(value.as_str(), "." | "..")
			|| !value
				.bytes()
				.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
		{
			return Err(ManagedRepositoryError::InvalidRegistrationId);
		}
		Ok(Self(value))
	}

	/// Borrow the canonical registration identity.
	pub fn as_str(&self) -> &str {
		&self.0
	}
}

/// SHA-256 digest of the complete admitted repository descriptor.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AdmissionDescriptorDigest(String);
impl AdmissionDescriptorDigest {
	/// Parse one canonical lowercase SHA-256 digest.
	pub fn new(value: impl Into<String>) -> Result<Self, ManagedRepositoryError> {
		let value = value.into();
		if value.len() != 64
			|| !value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
		{
			return Err(ManagedRepositoryError::InvalidDescriptorDigest);
		}
		Ok(Self(value))
	}

	/// Borrow the canonical digest.
	pub fn as_str(&self) -> &str {
		&self.0
	}

	fn digest(bytes: &[u8]) -> Self {
		Self(hex_sha256(bytes))
	}
}

/// Normalized absolute path persisted for server-host reacquisition.
#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PersistedAbsolutePath(PathBuf);
impl PersistedAbsolutePath {
	/// Validate a normalized absolute UTF-8 path without `.` or `..` components.
	///
	/// This validates representation only. It does not attest symlink freedom or object identity.
	pub fn new(value: PathBuf) -> Result<Self, ManagedRepositoryError> {
		let Some(encoded) = value.to_str() else {
			return Err(ManagedRepositoryError::InvalidAbsolutePath);
		};
		if encoded.len() > MAX_MANAGED_REPOSITORY_PATH_BYTES || !is_normalized_absolute(&value) {
			return Err(ManagedRepositoryError::InvalidAbsolutePath);
		}
		Ok(Self(value))
	}

	/// Borrow the server-host path.
	pub fn as_path(&self) -> &Path {
		&self.0
	}
}
impl Debug for PersistedAbsolutePath {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str("PersistedAbsolutePath(<server-host-only>)")
	}
}

/// Normalized absolute path of one persisted reacquisition observation, including `/`.
#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RepositoryObservationPath(PathBuf);
impl RepositoryObservationPath {
	/// Validate a normalized absolute UTF-8 observation path without `.` or `..` components.
	pub fn new(value: PathBuf) -> Result<Self, ManagedRepositoryError> {
		let Some(encoded) = value.to_str() else {
			return Err(ManagedRepositoryError::InvalidAbsolutePath);
		};
		if encoded.len() > MAX_MANAGED_REPOSITORY_PATH_BYTES
			|| (value != Path::new("/") && !is_normalized_absolute(&value))
		{
			return Err(ManagedRepositoryError::InvalidAbsolutePath);
		}
		Ok(Self(value))
	}

	/// Borrow the normalized observation path.
	pub fn as_path(&self) -> &Path {
		&self.0
	}
}
impl Debug for RepositoryObservationPath {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str("RepositoryObservationPath(<server-host-only>)")
	}
}

/// Version of the complete persisted repository-admission descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RepositoryAdmissionDescriptorVersion {
	/// Initial restart-reacquisition contract.
	V1,
}

/// Closed admitted Git registration shape.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RepositoryGitRegistrationRole {
	/// The admitted root owns the one shared `.git` directory directly.
	PrimaryWorktree,
	/// The admitted root is a linked worktree with one named private-admin child and reciprocity.
	LinkedWorktree,
}

/// Closed semantic role of one persisted path/object observation.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum RepositoryPathRegistrationRole {
	/// Ancestor component traversed to reacquire the repository root.
	RepositoryRootComponent,
	/// Exact admitted repository root.
	RepositoryRoot,
	/// Exact `.git` directory or link file at the admitted root.
	WorktreeGitEntry,
	/// Ancestor component traversed to reacquire the Git directory.
	GitDirectoryComponent,
	/// Exact per-worktree Git directory.
	GitDirectory,
	/// Ancestor component traversed to reacquire the Git common directory.
	GitCommonDirectoryComponent,
	/// Exact Git common directory.
	GitCommonDirectory,
	/// Ancestor component traversed to reacquire the Git object directory.
	GitObjectsDirectoryComponent,
	/// Exact Git object directory.
	GitObjectsDirectory,
	/// Ancestor component traversed to reacquire the optional refs directory.
	GitRefsDirectoryComponent,
	/// Exact optional Git refs directory.
	GitRefsDirectory,
	/// Exact `commondir` metadata file resolving the common directory.
	GitCommonDirectoryFile,
	/// Exact linked-worktree `gitdir` reciprocal metadata file.
	GitDirectoryBacklinkFile,
}

/// Closed admitted filesystem object type. Symlinks and special objects are unrepresentable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RepositoryObservedObjectType {
	/// Directory opened one component at a time without following symlinks.
	Directory,
	/// Regular file opened through its verified parent without following the final component.
	RegularFile,
}

/// One persisted snapshot of observable path/object metadata used during reacquisition.
///
/// Equality can detect changed observed metadata. It is not proof that an inode was never deleted
/// and reused while the daemon was stopped.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryPathObservation {
	path: RepositoryObservationPath,
	roles: Vec<RepositoryPathRegistrationRole>,
	device: u64,
	inode: u64,
	object_type: RepositoryObservedObjectType,
	owner_uid: u32,
	permissions: u16,
}
impl RepositoryPathObservation {
	/// Construct one observable metadata snapshot. Roles must be nonempty, unique, and ordered.
	pub fn new(
		path: RepositoryObservationPath,
		roles: Vec<RepositoryPathRegistrationRole>,
		device: u64,
		inode: u64,
		object_type: RepositoryObservedObjectType,
		owner_uid: u32,
		permissions: u32,
	) -> Result<Self, ManagedRepositoryError> {
		if roles.is_empty()
			|| roles.len() > MAX_REPOSITORY_OBSERVATION_ROLES
			|| !strictly_ordered(&roles)
			|| inode == 0
			|| permissions & !0o7777 != 0
			|| permissions & 0o022 != 0
		{
			return Err(ManagedRepositoryError::InvalidPathObservation);
		}
		Ok(Self {
			path,
			roles,
			device,
			inode,
			object_type,
			owner_uid,
			permissions: permissions as u16,
		})
	}

	/// Borrow the exact normalized path.
	pub fn path(&self) -> &RepositoryObservationPath {
		&self.path
	}

	/// Borrow the nonempty canonical role set.
	pub fn roles(&self) -> &[RepositoryPathRegistrationRole] {
		&self.roles
	}

	/// Return the admitted device fact.
	pub const fn device(&self) -> u64 {
		self.device
	}

	/// Return the admitted inode fact.
	pub const fn inode(&self) -> u64 {
		self.inode
	}

	/// Return the admitted object type.
	pub const fn object_type(&self) -> RepositoryObservedObjectType {
		self.object_type
	}

	/// Return the admitted owner UID.
	pub const fn owner_uid(&self) -> u32 {
		self.owner_uid
	}

	/// Return the exact admitted Unix permission bits.
	pub const fn permissions(&self) -> u16 {
		self.permissions
	}
}

/// Exact closed V1 Git layout resolved at admission without embedding Git execution behavior.
///
/// For a linked worktree, the `.git` file resolves to `git_directory`, `commondir` resolves to
/// `common_directory`, and `gitdir` resolves back to `worktree_git_entry`; adapters must observe
/// those exact reciprocal values when constructing or reacquiring this forgeable data.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryAdmittedGitLayout {
	registration_role: RepositoryGitRegistrationRole,
	registration_id: Option<RepositoryRegistrationId>,
	repository_root: PersistedAbsolutePath,
	worktree_git_entry: PersistedAbsolutePath,
	git_directory: PersistedAbsolutePath,
	common_directory: PersistedAbsolutePath,
	objects_directory: PersistedAbsolutePath,
	refs_directory: Option<PersistedAbsolutePath>,
	common_directory_file: Option<PersistedAbsolutePath>,
	git_directory_backlink_file: Option<PersistedAbsolutePath>,
}
impl RepositoryAdmittedGitLayout {
	/// Collect the resolved Git layout facts. The descriptor constructor validates relationships.
	#[allow(clippy::too_many_arguments)]
	pub fn new(
		registration_role: RepositoryGitRegistrationRole,
		registration_id: Option<RepositoryRegistrationId>,
		repository_root: PersistedAbsolutePath,
		worktree_git_entry: PersistedAbsolutePath,
		git_directory: PersistedAbsolutePath,
		common_directory: PersistedAbsolutePath,
		objects_directory: PersistedAbsolutePath,
		refs_directory: Option<PersistedAbsolutePath>,
		common_directory_file: Option<PersistedAbsolutePath>,
		git_directory_backlink_file: Option<PersistedAbsolutePath>,
	) -> Self {
		Self {
			registration_role,
			registration_id,
			repository_root,
			worktree_git_entry,
			git_directory,
			common_directory,
			objects_directory,
			refs_directory,
			common_directory_file,
			git_directory_backlink_file,
		}
	}

	/// Return the admitted Git registration role.
	pub const fn registration_role(&self) -> RepositoryGitRegistrationRole {
		self.registration_role
	}

	/// Borrow the linked-worktree registration identity; primary layouts have none.
	pub fn registration_id(&self) -> Option<&RepositoryRegistrationId> {
		self.registration_id.as_ref()
	}

	/// Borrow the exact admitted repository root.
	pub fn repository_root(&self) -> &PersistedAbsolutePath {
		&self.repository_root
	}

	/// Borrow the exact `.git` entry at the admitted root.
	pub fn worktree_git_entry(&self) -> &PersistedAbsolutePath {
		&self.worktree_git_entry
	}

	/// Borrow the exact per-worktree Git directory.
	pub fn git_directory(&self) -> &PersistedAbsolutePath {
		&self.git_directory
	}

	/// Borrow the exact Git common directory.
	pub fn common_directory(&self) -> &PersistedAbsolutePath {
		&self.common_directory
	}

	/// Borrow the exact Git objects directory.
	pub fn objects_directory(&self) -> &PersistedAbsolutePath {
		&self.objects_directory
	}

	/// Borrow the exact optional Git refs directory.
	pub fn refs_directory(&self) -> Option<&PersistedAbsolutePath> {
		self.refs_directory.as_ref()
	}

	/// Borrow the `commondir` file required when Git and common directories differ.
	pub fn common_directory_file(&self) -> Option<&PersistedAbsolutePath> {
		self.common_directory_file.as_ref()
	}

	/// Borrow the reciprocal `gitdir` file required for a linked worktree.
	pub fn git_directory_backlink_file(&self) -> Option<&PersistedAbsolutePath> {
		self.git_directory_backlink_file.as_ref()
	}
}

/// Complete versioned repository-admission descriptor and its derived inventory digest.
///
/// Exact descriptor equality is authoritative input equality. It detects changed observable facts;
/// it does not establish uninterrupted filesystem identity across daemon downtime. The digest is
/// derived evidence only.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryAdmissionDescriptor {
	version: RepositoryAdmissionDescriptorVersion,
	project_id: ProjectId,
	repository_id: ManagedRepositoryId,
	admitted_identity: AdmittedRepositoryIdentity,
	admitted_base: RepositoryContentRevision,
	repository_path: PersistedAbsolutePath,
	git_layout: RepositoryAdmittedGitLayout,
	observations: Vec<RepositoryPathObservation>,
	digest: AdmissionDescriptorDigest,
}
impl RepositoryAdmissionDescriptor {
	/// Construct and validate one complete V1 descriptor, deriving its SHA-256 digest.
	#[allow(clippy::too_many_arguments)]
	pub fn new_v1(
		project_id: ProjectId,
		repository_id: ManagedRepositoryId,
		admitted_identity: AdmittedRepositoryIdentity,
		admitted_base: RepositoryContentRevision,
		repository_path: PersistedAbsolutePath,
		git_layout: RepositoryAdmittedGitLayout,
		observations: Vec<RepositoryPathObservation>,
	) -> Result<Self, ManagedRepositoryError> {
		let mut descriptor = Self {
			version: RepositoryAdmissionDescriptorVersion::V1,
			project_id,
			repository_id,
			admitted_identity,
			admitted_base,
			repository_path,
			git_layout,
			observations,
			digest: AdmissionDescriptorDigest(String::new()),
		};
		descriptor.validate_shape()?;
		descriptor.digest = AdmissionDescriptorDigest::digest(&descriptor.canonical_bytes());
		Ok(descriptor)
	}

	/// Return the closed descriptor version.
	pub const fn version(&self) -> RepositoryAdmissionDescriptorVersion {
		self.version
	}

	/// Borrow the exact owning Project identity.
	pub fn project_id(&self) -> &ProjectId {
		&self.project_id
	}

	/// Borrow the exact managed repository identity.
	pub fn repository_id(&self) -> &ManagedRepositoryId {
		&self.repository_id
	}

	/// Borrow the opaque admitted external identity.
	pub fn admitted_identity(&self) -> &AdmittedRepositoryIdentity {
		&self.admitted_identity
	}

	/// Borrow the exact admitted base.
	pub fn admitted_base(&self) -> &RepositoryContentRevision {
		&self.admitted_base
	}

	/// Borrow the exact normalized admitted repository path.
	pub fn repository_path(&self) -> &PersistedAbsolutePath {
		&self.repository_path
	}

	/// Borrow the complete admitted Git layout.
	pub fn git_layout(&self) -> &RepositoryAdmittedGitLayout {
		&self.git_layout
	}

	/// Borrow the bounded canonical observation sequence.
	pub fn observations(&self) -> &[RepositoryPathObservation] {
		&self.observations
	}

	/// Borrow the derived canonical SHA-256 digest.
	pub fn digest(&self) -> &AdmissionDescriptorDigest {
		&self.digest
	}

	/// Return the module-owned canonical V1 byte representation hashed by the digest.
	pub fn canonical_bytes(&self) -> Vec<u8> {
		encode_admission_descriptor_v1(self)
	}

	/// Verify persisted digest evidence against the complete canonical descriptor.
	pub fn verify_digest(&self, expected: &AdmissionDescriptorDigest) -> bool {
		let computed = AdmissionDescriptorDigest::digest(&self.canonical_bytes());
		self.digest == computed && *expected == computed
	}

	fn validate_shape(&self) -> Result<(), ManagedRepositoryError> {
		if self.repository_path != self.git_layout.repository_root
			|| self.observations.is_empty()
			|| self.observations.len() > MAX_REPOSITORY_ADMISSION_OBSERVATIONS
			|| !self.observations.windows(2).all(|pair| pair[0].path < pair[1].path)
			|| self.observations.iter().enumerate().any(|(index, observation)| {
				self.observations[index + 1..].iter().any(|candidate| {
					candidate.device == observation.device && candidate.inode == observation.inode
				})
			}) {
			return Err(ManagedRepositoryError::InvalidAdmissionDescriptor);
		}
		validate_git_layout(&self.git_layout, &self.observations)
	}
}

/// Forgeable immutable repository-admission facts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryAdmissionFacts {
	descriptor: RepositoryAdmissionDescriptor,
}
impl RepositoryAdmissionFacts {
	/// Wrap one validated complete descriptor as forgeable admission facts.
	pub fn new(descriptor: RepositoryAdmissionDescriptor) -> Self {
		Self { descriptor }
	}

	/// Borrow the complete descriptor.
	pub fn descriptor(&self) -> &RepositoryAdmissionDescriptor {
		&self.descriptor
	}

	/// Borrow its derived digest; no independent digest field can disagree.
	pub fn descriptor_digest(&self) -> &AdmissionDescriptorDigest {
		self.descriptor.digest()
	}
}

/// Read-only positive external evidence used by Allocate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PositiveAllocationEvidence {
	evidence_id: RepositoryEvidenceId,
	admission_descriptor: RepositoryAdmissionDescriptor,
	vacant_worktree_path: PersistedAbsolutePath,
}
impl PositiveAllocationEvidence {
	/// Construct forgeable positive evidence from a complete observed descriptor.
	pub fn new(
		evidence_id: RepositoryEvidenceId,
		admission_descriptor: RepositoryAdmissionDescriptor,
		vacant_worktree_path: PersistedAbsolutePath,
	) -> Self {
		Self { evidence_id, admission_descriptor, vacant_worktree_path }
	}

	/// Borrow the immutable evidence identity.
	pub fn evidence_id(&self) -> &RepositoryEvidenceId {
		&self.evidence_id
	}

	/// Borrow the complete descriptor observed during read-only reacquisition.
	pub fn admission_descriptor(&self) -> &RepositoryAdmissionDescriptor {
		&self.admission_descriptor
	}

	/// Borrow the exact path observed vacant.
	pub fn vacant_worktree_path(&self) -> &PersistedAbsolutePath {
		&self.vacant_worktree_path
	}
}

/// Forgeable representation of durable-store's current aggregate checkpoint.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AggregateCheckpoint {
	/// Positive durable-store-owned generation.
	pub generation: u64,
	/// Exact durable-store-owned authority-event tip.
	pub tip: RepositoryAuthorityTip,
}
impl AggregateCheckpoint {
	/// Construct a non-authoritative checkpoint fact.
	pub fn new(
		generation: u64,
		tip: RepositoryAuthorityTip,
	) -> Result<Self, ManagedRepositoryError> {
		if generation == 0 {
			return Err(ManagedRepositoryError::InvalidCheckpoint);
		}
		Ok(Self { generation, tip })
	}
}

/// Version of the complete canonical operation descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OperationDescriptorVersion {
	/// Initial managed-repository descriptor contract.
	V1,
}

/// Version of the fixed executor behavior interpreting a descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExecutorContractVersion(u16);
impl ExecutorContractVersion {
	/// Construct a positive executor contract version.
	pub fn new(value: u16) -> Result<Self, ManagedRepositoryError> {
		if value == 0 {
			return Err(ManagedRepositoryError::InvalidExecutorContractVersion);
		}
		Ok(Self(value))
	}

	/// Return the version number.
	pub const fn get(self) -> u16 {
		self.0
	}
}

/// Forgeable positive availability facts that durable-store must independently enforce.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AllocationAvailabilityFacts {
	/// Globally available allocation identity.
	pub allocation_id: RepositoryAllocationId,
	/// Globally available worktree identity.
	pub worktree_id: ManagedWorktreeId,
	/// Globally available persisted path claim.
	pub worktree_path: PersistedAbsolutePath,
}

/// Persistence-only Allocate command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AllocateRepositoryCommand {
	/// Allocation identity to claim in durable-store.
	pub allocation_id: RepositoryAllocationId,
	/// Worktree identity to claim in durable-store.
	pub worktree_id: ManagedWorktreeId,
	/// Final worktree path to claim in durable-store.
	pub worktree_path: PersistedAbsolutePath,
}

/// Pure Allocate proposal containing no filesystem or Git instruction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AllocateRepositoryDecision {
	/// Immutable admission copied into the proposed projection.
	pub admission: RepositoryAdmissionFacts,
	/// Allocation identity proposed for persistence.
	pub allocation_id: RepositoryAllocationId,
	/// Worktree identity proposed for persistence.
	pub worktree_id: ManagedWorktreeId,
	/// Worktree path proposed for a durable-store uniqueness claim only.
	pub worktree_path: PersistedAbsolutePath,
	/// Exact initial head, unchanged from admission.
	pub head: RepositoryContentRevision,
	/// Exact positive read-only evidence accepted by the decision.
	pub evidence: PositiveAllocationEvidence,
}

/// Decide a persistence-only allocation from exact positive read-only evidence.
pub fn decide_allocate(
	admission: &RepositoryAdmissionFacts,
	availability: &AllocationAvailabilityFacts,
	command: &AllocateRepositoryCommand,
	evidence: &PositiveAllocationEvidence,
) -> Result<AllocateRepositoryDecision, ManagedRepositoryError> {
	if admission.descriptor.repository_path == command.worktree_path {
		return Err(ManagedRepositoryError::InvalidAllocationTarget);
	}
	if availability.allocation_id != command.allocation_id
		|| availability.worktree_id != command.worktree_id
		|| availability.worktree_path != command.worktree_path
	{
		return Err(ManagedRepositoryError::AvailabilityMismatch);
	}
	if evidence.admission_descriptor != admission.descriptor
		|| evidence.vacant_worktree_path != command.worktree_path
	{
		return Err(ManagedRepositoryError::AllocationEvidenceMismatch);
	}
	Ok(AllocateRepositoryDecision {
		admission: admission.clone(),
		allocation_id: command.allocation_id.clone(),
		worktree_id: command.worktree_id.clone(),
		worktree_path: command.worktree_path.clone(),
		head: admission.descriptor.admitted_base.clone(),
		evidence: evidence.clone(),
	})
}

/// Current lifecycle projection loaded from persistence as forgeable data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManagedRepositoryPhase {
	/// durable-store allocation exists; no external mutation is implied.
	Allocated,
	/// Exact reciprocal repository/worktree registration completed.
	Registered,
	/// The worktree is ready at the unchanged exact head.
	Ready,
	/// Readback cannot establish a safe current external state.
	Ambiguous(RepositoryAmbiguity),
}

/// Complete forgeable current projection facts used by pure decisions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManagedRepositoryFacts {
	/// Immutable admission facts.
	pub admission: RepositoryAdmissionFacts,
	/// Exact allocation identity.
	pub allocation_id: RepositoryAllocationId,
	/// Exact worktree identity.
	pub worktree_id: ManagedWorktreeId,
	/// Persisted worktree path.
	pub worktree_path: PersistedAbsolutePath,
	/// Current lifecycle fact.
	pub phase: ManagedRepositoryPhase,
	/// Current exact head fact.
	pub head: RepositoryContentRevision,
	/// durable-store checkpoint fact; this value grants no CAS or write authority.
	pub checkpoint: AggregateCheckpoint,
	/// Operation currently fencing the aggregate, if any.
	pub active_operation: Option<RepositoryOperationId>,
}

/// Closed operation kind sharing one global ID namespace.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RepositoryOperationKind {
	/// One-shot reciprocal registration.
	Register,
	/// Worktree readiness while preserving the exact head.
	WorktreeReady,
	/// Exact head advancement.
	Commit,
}

/// Exact reciprocal target of registration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegistrationTarget {
	/// Repository side of the relationship.
	pub repository_id: ManagedRepositoryId,
	/// Worktree side of the relationship.
	pub worktree_id: ManagedWorktreeId,
	/// Exact repository path.
	pub repository_path: PersistedAbsolutePath,
	/// Exact worktree path.
	pub worktree_path: PersistedAbsolutePath,
}

/// Closed V1 policy for preparing a worktree without advancing its head.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorktreeReadyPolicy {
	/// Materialize the exact registered content into a clean service-private worktree and index.
	ExactCleanWorktree,
}

/// Bounded canonical commit message.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryCommitMessage(String);
impl RepositoryCommitMessage {
	/// Validate exact UTF-8 commit-message text.
	pub fn new(value: impl Into<String>) -> Result<Self, ManagedRepositoryError> {
		let value = value.into();
		if value.is_empty()
			|| value.len() > MAX_REPOSITORY_COMMIT_MESSAGE_BYTES
			|| value.contains('\0')
			|| value.contains('\r')
		{
			return Err(ManagedRepositoryError::InvalidCommitMessage);
		}
		Ok(Self(value))
	}

	/// Borrow the exact message.
	pub fn as_str(&self) -> &str {
		&self.0
	}
}

/// Exact canonical commit attribution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryCommitActor {
	/// Bounded display name.
	pub name: RepositoryCommitActorName,
	/// Bounded email-shaped opaque identity.
	pub email: RepositoryCommitActorEmail,
	/// Exact Unix timestamp seconds used in commit identity.
	pub timestamp_seconds: i64,
	/// Exact UTC offset minutes in the inclusive canonical range.
	pub utc_offset_minutes: i16,
}
impl RepositoryCommitActor {
	/// Validate canonical timestamp offset bounds.
	pub fn new(
		name: RepositoryCommitActorName,
		email: RepositoryCommitActorEmail,
		timestamp_seconds: i64,
		utc_offset_minutes: i16,
	) -> Result<Self, ManagedRepositoryError> {
		if !(-1_439..=1_439).contains(&utc_offset_minutes) {
			return Err(ManagedRepositoryError::InvalidCommitActor);
		}
		Ok(Self { name, email, timestamp_seconds, utc_offset_minutes })
	}
}

/// Complete immutable semantic commit intent interpreted by the fixed executor contract.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalCommitIntent {
	/// Exact reference advanced by compare-and-swap.
	pub target_reference: RepositoryReferenceName,
	/// Exact tree content used by the commit.
	pub tree: RepositoryContentRevision,
	/// Exact commit message.
	pub message: RepositoryCommitMessage,
	/// Exact author identity and time.
	pub author: RepositoryCommitActor,
	/// Exact committer identity and time.
	pub committer: RepositoryCommitActor,
}

/// Closed complete semantic payload of one canonical operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CanonicalOperationPayload {
	/// Registration with exact reciprocal target and unchanged head.
	Register {
		/// Exact head before and after successful registration.
		expected_head: RepositoryContentRevision,
		/// Exact reciprocal relationship to establish.
		target: RegistrationTarget,
	},
	/// Readiness with an exact head that must remain unchanged.
	WorktreeReady {
		/// Exact head before and after readiness.
		expected_head: RepositoryContentRevision,
		/// Closed preparation policy.
		policy: WorktreeReadyPolicy,
	},
	/// Commit from one exact head to one distinct exact head.
	Commit {
		/// Exact predecessor head.
		expected_head: RepositoryContentRevision,
		/// Exact completion head.
		next_head: RepositoryContentRevision,
		/// Complete immutable semantic commit intent.
		intent: CanonicalCommitIntent,
	},
}
impl CanonicalOperationPayload {
	/// Return the closed operation kind.
	pub const fn kind(&self) -> RepositoryOperationKind {
		match self {
			Self::Register { .. } => RepositoryOperationKind::Register,
			Self::WorktreeReady { .. } => RepositoryOperationKind::WorktreeReady,
			Self::Commit { .. } => RepositoryOperationKind::Commit,
		}
	}
}

/// Complete canonical immutable assignment descriptor.
///
/// Equality covers every field. durable-store must persist the complete representation and enforce
/// global assignment; a digest alone is not assignment authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalOperationDescriptor {
	/// Descriptor schema.
	pub schema: OperationDescriptorVersion,
	/// Globally single-assigned operation identity.
	pub operation_id: RepositoryOperationId,
	/// Owning Project.
	pub project_id: ProjectId,
	/// Managed repository.
	pub repository_id: ManagedRepositoryId,
	/// Immutable admitted external identity.
	pub admitted_identity: AdmittedRepositoryIdentity,
	/// Immutable admitted base.
	pub admitted_base: RepositoryContentRevision,
	/// Complete admission descriptor digest.
	pub admission_descriptor_digest: AdmissionDescriptorDigest,
	/// Exact allocation.
	pub allocation_id: RepositoryAllocationId,
	/// Exact managed worktree.
	pub worktree_id: ManagedWorktreeId,
	/// Exact persisted repository path.
	pub repository_absolute_path: PersistedAbsolutePath,
	/// Exact persisted worktree path.
	pub worktree_absolute_path: PersistedAbsolutePath,
	/// Pre-operation durable-store checkpoint fact.
	pub expected_checkpoint: AggregateCheckpoint,
	/// Closed kind, redundant by design for canonical persistence checks.
	pub kind: RepositoryOperationKind,
	/// Complete closed semantic payload.
	pub payload: CanonicalOperationPayload,
	/// Fixed executor behavior version.
	pub executor_contract: ExecutorContractVersion,
}

/// Terminal reason established by transition-specific readback.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RepositoryAmbiguity {
	/// External facts were older than the required operation boundary.
	Stale,
	/// External facts named another repository, worktree, allocation, or operation.
	Foreign,
	/// Reacquisition observed path/object metadata different from the admitted descriptor.
	Replaced,
	/// Unreserved or unexpected mutation was observed.
	Dirty,
	/// Readback established rollback from the required state.
	Rollback,
	/// Readback established that the effect did not occur.
	NoEffect,
	/// Required exact positive evidence was missing or incomplete.
	Incomplete,
	/// Bounded readback completed without one safe conclusion.
	Inconclusive,
}

/// Exact immutable successful operation result.
#[allow(missing_docs)] // The type-level contract documents its self-describing result fields.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RepositoryOperationResult {
	/// Registration completed at an unchanged exact head.
	Registered { head: RepositoryContentRevision },
	/// Worktree readiness completed at an unchanged exact head.
	WorktreeReady { head: RepositoryContentRevision },
	/// Commit advanced one exact predecessor to one exact successor.
	Committed { from: RepositoryContentRevision, to: RepositoryContentRevision },
}

/// Immutable lifecycle view of one globally assigned operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RepositoryOperationState {
	/// The external effect may have occurred and only readback may follow.
	PossiblyEffected,
	/// Exact transition-specific positive evidence completed the operation.
	Completed(RepositoryOperationResult),
	/// Readback permanently failed closed.
	Ambiguous(RepositoryAmbiguity),
}

/// Forgeable persisted operation view. It never carries dispatch authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationView {
	/// Complete immutable canonical descriptor.
	pub descriptor: CanonicalOperationDescriptor,
	/// Current append-only lifecycle projection.
	pub state: RepositoryOperationState,
}

/// Explicit proof at the type level that an assignment result cannot dispatch.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NoDispatch;

/// Pure global operation-assignment comparison.
#[allow(clippy::large_enum_variant)] // Preserve the stable by-value semantic result algebra.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AssignmentResolution {
	/// No assignment fact was supplied; durable-store may attempt a new global insert.
	NewlyAssigned,
	/// The complete existing descriptor is exact and can only be read back.
	ExistingExact(OperationView, NoDispatch),
	/// The same global operation ID was assigned a different complete descriptor.
	OperationIdConflict,
}

/// Compare a requested descriptor with a durable-store-loaded global assignment fact.
pub fn resolve_operation_assignment(
	requested: &CanonicalOperationDescriptor,
	existing: Option<&OperationView>,
) -> AssignmentResolution {
	match existing {
		None => AssignmentResolution::NewlyAssigned,
		Some(operation) if operation.descriptor == *requested =>
			AssignmentResolution::ExistingExact(operation.clone(), NoDispatch),
		Some(_) => AssignmentResolution::OperationIdConflict,
	}
}

/// Command to begin the distinct registration operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BeginRegistrationCommand {
	/// Globally single-assigned operation ID.
	pub operation_id: RepositoryOperationId,
	/// Exact pre-operation checkpoint.
	pub expected_checkpoint: AggregateCheckpoint,
	/// Exact head that registration must preserve.
	pub expected_head: RepositoryContentRevision,
	/// Fixed executor behavior version.
	pub executor_contract: ExecutorContractVersion,
}

/// Command to begin the distinct WorktreeReady operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BeginWorktreeReadyCommand {
	/// Globally single-assigned operation ID.
	pub operation_id: RepositoryOperationId,
	/// Exact pre-operation checkpoint.
	pub expected_checkpoint: AggregateCheckpoint,
	/// Exact head that readiness must preserve.
	pub expected_head: RepositoryContentRevision,
	/// Closed readiness policy.
	pub policy: WorktreeReadyPolicy,
	/// Fixed executor behavior version.
	pub executor_contract: ExecutorContractVersion,
}

/// Command to begin the distinct Commit operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BeginCommitCommand {
	/// Globally single-assigned operation ID.
	pub operation_id: RepositoryOperationId,
	/// Exact pre-operation checkpoint.
	pub expected_checkpoint: AggregateCheckpoint,
	/// Exact predecessor head.
	pub expected_head: RepositoryContentRevision,
	/// Exact distinct completion head.
	pub next_head: RepositoryContentRevision,
	/// Complete immutable semantic commit intent.
	pub intent: CanonicalCommitIntent,
	/// Fixed executor behavior version.
	pub executor_contract: ExecutorContractVersion,
}

/// Pure proposal to atomically fence registration as `PossiblyEffected`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BeginRegistrationDecision {
	/// Complete immutable descriptor to assign globally.
	pub descriptor: CanonicalOperationDescriptor,
	/// Initial operation view to persist append-only.
	pub operation: OperationView,
	/// Operation ID proposed as the aggregate fence.
	pub active_operation: RepositoryOperationId,
}

/// Pure proposal to atomically fence WorktreeReady as `PossiblyEffected`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BeginWorktreeReadyDecision {
	/// Complete immutable descriptor to assign globally.
	pub descriptor: CanonicalOperationDescriptor,
	/// Initial operation view to persist append-only.
	pub operation: OperationView,
	/// Operation ID proposed as the aggregate fence.
	pub active_operation: RepositoryOperationId,
}

/// Pure proposal to atomically fence Commit as `PossiblyEffected`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BeginCommitDecision {
	/// Complete immutable descriptor to assign globally.
	pub descriptor: CanonicalOperationDescriptor,
	/// Initial operation view to persist append-only.
	pub operation: OperationView,
	/// Operation ID proposed as the aggregate fence.
	pub active_operation: RepositoryOperationId,
}

/// Begin registration without producing an execution capability.
pub fn decide_begin_registration(
	facts: &ManagedRepositoryFacts,
	command: &BeginRegistrationCommand,
) -> Result<BeginRegistrationDecision, ManagedRepositoryError> {
	validate_begin(facts, &command.expected_checkpoint, ManagedRepositoryPhase::Allocated)?;
	if facts.head != command.expected_head {
		return Err(ManagedRepositoryError::HeadPreconditionMismatch);
	}
	let payload = CanonicalOperationPayload::Register {
		expected_head: command.expected_head.clone(),
		target: RegistrationTarget {
			repository_id: facts.admission.descriptor.repository_id.clone(),
			worktree_id: facts.worktree_id.clone(),
			repository_path: facts.admission.descriptor.repository_path.clone(),
			worktree_path: facts.worktree_path.clone(),
		},
	};
	let descriptor =
		descriptor(facts, command.operation_id.clone(), payload, command.executor_contract);
	let operation = possibly_effected(descriptor.clone());
	Ok(BeginRegistrationDecision {
		descriptor,
		operation,
		active_operation: command.operation_id.clone(),
	})
}

/// Begin WorktreeReady without producing an execution capability.
pub fn decide_begin_worktree_ready(
	facts: &ManagedRepositoryFacts,
	command: &BeginWorktreeReadyCommand,
) -> Result<BeginWorktreeReadyDecision, ManagedRepositoryError> {
	validate_begin(facts, &command.expected_checkpoint, ManagedRepositoryPhase::Registered)?;
	if facts.head != command.expected_head {
		return Err(ManagedRepositoryError::HeadPreconditionMismatch);
	}
	let payload = CanonicalOperationPayload::WorktreeReady {
		expected_head: command.expected_head.clone(),
		policy: command.policy,
	};
	let descriptor =
		descriptor(facts, command.operation_id.clone(), payload, command.executor_contract);
	let operation = possibly_effected(descriptor.clone());
	Ok(BeginWorktreeReadyDecision {
		descriptor,
		operation,
		active_operation: command.operation_id.clone(),
	})
}

/// Begin Commit without producing an execution capability.
pub fn decide_begin_commit(
	facts: &ManagedRepositoryFacts,
	command: &BeginCommitCommand,
) -> Result<BeginCommitDecision, ManagedRepositoryError> {
	validate_begin(facts, &command.expected_checkpoint, ManagedRepositoryPhase::Ready)?;
	if facts.head != command.expected_head {
		return Err(ManagedRepositoryError::HeadPreconditionMismatch);
	}
	if command.next_head == command.expected_head {
		return Err(ManagedRepositoryError::HeadDidNotAdvance);
	}
	validate_commit_actor(&command.intent.author)?;
	validate_commit_actor(&command.intent.committer)?;
	let payload = CanonicalOperationPayload::Commit {
		expected_head: command.expected_head.clone(),
		next_head: command.next_head.clone(),
		intent: command.intent.clone(),
	};
	let descriptor =
		descriptor(facts, command.operation_id.clone(), payload, command.executor_contract);
	let operation = possibly_effected(descriptor.clone());
	Ok(BeginCommitDecision {
		descriptor,
		operation,
		active_operation: command.operation_id.clone(),
	})
}

/// Readback-only request for a possibly-effected registration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegistrationReadbackRequest {
	/// Complete descriptor; this data cannot authorize execution.
	pub descriptor: CanonicalOperationDescriptor,
}

/// Readback-only request for a possibly-effected WorktreeReady operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorktreeReadyReadbackRequest {
	/// Complete descriptor; this data cannot authorize execution.
	pub descriptor: CanonicalOperationDescriptor,
}

/// Readback-only request for a possibly-effected Commit operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitReadbackRequest {
	/// Complete descriptor; this data cannot authorize execution.
	pub descriptor: CanonicalOperationDescriptor,
}

/// Form a registration readback request and no execution authority.
pub fn registration_readback_request(
	operation: &OperationView,
) -> Result<RegistrationReadbackRequest, ManagedRepositoryError> {
	validate_readback(operation, RepositoryOperationKind::Register)?;
	Ok(RegistrationReadbackRequest { descriptor: operation.descriptor.clone() })
}

/// Form a WorktreeReady readback request and no execution authority.
pub fn worktree_ready_readback_request(
	operation: &OperationView,
) -> Result<WorktreeReadyReadbackRequest, ManagedRepositoryError> {
	validate_readback(operation, RepositoryOperationKind::WorktreeReady)?;
	Ok(WorktreeReadyReadbackRequest { descriptor: operation.descriptor.clone() })
}

/// Form a Commit readback request and no execution authority.
pub fn commit_readback_request(
	operation: &OperationView,
) -> Result<CommitReadbackRequest, ManagedRepositoryError> {
	validate_readback(operation, RepositoryOperationKind::Commit)?;
	Ok(CommitReadbackRequest { descriptor: operation.descriptor.clone() })
}

/// Exact positive reciprocal-registration evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExactRepositoryReadbackScope {
	/// Immutable evidence identity.
	pub evidence_id: RepositoryEvidenceId,
	/// Exact operation read back.
	pub operation_id: RepositoryOperationId,
	/// Exact admitted external identity observed.
	pub admitted_identity: AdmittedRepositoryIdentity,
	/// Exact admitted base observed.
	pub admitted_base: RepositoryContentRevision,
	/// Exact managed repository observed.
	pub repository_id: ManagedRepositoryId,
	/// Exact allocation observed.
	pub allocation_id: RepositoryAllocationId,
	/// Exact managed worktree observed.
	pub worktree_id: ManagedWorktreeId,
	/// Exact repository path reacquired.
	pub repository_path: PersistedAbsolutePath,
	/// Exact worktree path reacquired.
	pub worktree_path: PersistedAbsolutePath,
}

/// Exact positive reciprocal-registration evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExactRegistrationEvidence {
	/// Complete exact readback scope.
	pub scope: ExactRepositoryReadbackScope,
	/// Repository side's reciprocal worktree identity.
	pub repository_names_worktree: ManagedWorktreeId,
	/// Worktree side's reciprocal repository identity.
	pub worktree_names_repository: ManagedRepositoryId,
	/// Exact unchanged head.
	pub unchanged_head: RepositoryContentRevision,
}

/// Transition-specific registration evidence.
#[allow(clippy::large_enum_variant, missing_docs)] // Preserve the stable by-value evidence algebra.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RegistrationEvidence {
	ExactReciprocal(ExactRegistrationEvidence),
	NoEffect,
	MissingReciprocal,
	Stale,
	Foreign,
	Replaced,
	Dirty,
	Rollback,
	Inconclusive,
	Unavailable,
}

/// Exact positive unchanged-head readiness evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExactWorktreeReadyEvidence {
	/// Complete exact readback scope.
	pub scope: ExactRepositoryReadbackScope,
	/// Exact unchanged head.
	pub unchanged_head: RepositoryContentRevision,
}

/// Transition-specific WorktreeReady evidence.
#[allow(clippy::large_enum_variant, missing_docs)] // Preserve the stable by-value evidence algebra.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorktreeReadyEvidence {
	Exact(ExactWorktreeReadyEvidence),
	NoEffect,
	Incomplete,
	Stale,
	Foreign,
	Replaced,
	Dirty,
	Rollback,
	Inconclusive,
	Unavailable,
}

/// Exact positive commit evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExactCommitEvidence {
	/// Complete exact readback scope.
	pub scope: ExactRepositoryReadbackScope,
	/// Exact reference whose value was read back.
	pub target_reference: RepositoryReferenceName,
	/// Complete immutable commit intent verified at the completed head.
	pub intent: CanonicalCommitIntent,
	/// Exact predecessor head read back.
	pub predecessor_head: RepositoryContentRevision,
	/// Exact completion head read back.
	pub completed_head: RepositoryContentRevision,
}

/// Transition-specific Commit evidence.
#[allow(clippy::large_enum_variant, missing_docs)] // Preserve the stable by-value evidence algebra.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommitEvidence {
	Exact(ExactCommitEvidence),
	NoEffect,
	Incomplete,
	Stale,
	Foreign,
	Replaced,
	Dirty,
	Rollback,
	Inconclusive,
	Unavailable,
}

/// Proposed terminal aggregate projection update. durable-store alone applies it and issues its next
/// checkpoint.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryProjectionUpdate {
	/// Proposed terminal lifecycle.
	pub phase: ManagedRepositoryPhase,
	/// Proposed exact current head.
	pub head: RepositoryContentRevision,
	/// Exact active operation that durable-store must clear atomically.
	pub clear_active_operation: RepositoryOperationId,
}

/// Registration readback decision.
#[allow(missing_docs)] // Variant payload names are the complete semantic decision contract.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RegistrationReconciliation {
	Pending,
	Completed {
		operation: OperationView,
		repository: RepositoryProjectionUpdate,
		evidence: RegistrationEvidence,
	},
	Ambiguous {
		operation: OperationView,
		repository: RepositoryProjectionUpdate,
		evidence: RegistrationEvidence,
	},
}

/// WorktreeReady readback decision.
#[allow(missing_docs)] // Variant payload names are the complete semantic decision contract.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorktreeReadyReconciliation {
	Pending,
	Completed {
		operation: OperationView,
		repository: RepositoryProjectionUpdate,
		evidence: WorktreeReadyEvidence,
	},
	Ambiguous {
		operation: OperationView,
		repository: RepositoryProjectionUpdate,
		evidence: WorktreeReadyEvidence,
	},
}

/// Commit readback decision.
#[allow(missing_docs)] // Variant payload names are the complete semantic decision contract.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommitReconciliation {
	Pending,
	Completed {
		operation: OperationView,
		repository: RepositoryProjectionUpdate,
		evidence: CommitEvidence,
	},
	Ambiguous {
		operation: OperationView,
		repository: RepositoryProjectionUpdate,
		evidence: CommitEvidence,
	},
}

/// Reconcile registration from exact transition-specific readback.
pub fn decide_registration_readback(
	facts: &ManagedRepositoryFacts,
	operation: &OperationView,
	evidence: &RegistrationEvidence,
) -> Result<RegistrationReconciliation, ManagedRepositoryError> {
	validate_reconciliation(
		facts,
		operation,
		ManagedRepositoryPhase::Allocated,
		RepositoryOperationKind::Register,
	)?;
	let CanonicalOperationPayload::Register { expected_head, target } =
		&operation.descriptor.payload
	else {
		return Err(ManagedRepositoryError::OperationKindMismatch);
	};
	if facts.head != *expected_head {
		return Err(ManagedRepositoryError::OperationContextMismatch);
	}
	if matches!(evidence, RegistrationEvidence::Unavailable) {
		return Ok(RegistrationReconciliation::Pending);
	}
	let ambiguity = match evidence {
		RegistrationEvidence::ExactReciprocal(observed) =>
			registration_mismatch(facts, operation, target, expected_head, observed),
		RegistrationEvidence::NoEffect => Some(RepositoryAmbiguity::NoEffect),
		RegistrationEvidence::MissingReciprocal => Some(RepositoryAmbiguity::Incomplete),
		RegistrationEvidence::Stale => Some(RepositoryAmbiguity::Stale),
		RegistrationEvidence::Foreign => Some(RepositoryAmbiguity::Foreign),
		RegistrationEvidence::Replaced => Some(RepositoryAmbiguity::Replaced),
		RegistrationEvidence::Dirty => Some(RepositoryAmbiguity::Dirty),
		RegistrationEvidence::Rollback => Some(RepositoryAmbiguity::Rollback),
		RegistrationEvidence::Inconclusive => Some(RepositoryAmbiguity::Inconclusive),
		RegistrationEvidence::Unavailable => unreachable!(),
	};
	Ok(match ambiguity {
		Some(reason) => RegistrationReconciliation::Ambiguous {
			operation: ambiguous_operation(operation, reason),
			repository: terminal_update(
				operation,
				ManagedRepositoryPhase::Ambiguous(reason),
				facts.head.clone(),
			),
			evidence: evidence.clone(),
		},
		None => RegistrationReconciliation::Completed {
			operation: completed_operation(
				operation,
				RepositoryOperationResult::Registered { head: expected_head.clone() },
			),
			repository: terminal_update(
				operation,
				ManagedRepositoryPhase::Registered,
				expected_head.clone(),
			),
			evidence: evidence.clone(),
		},
	})
}

/// Reconcile WorktreeReady from exact transition-specific readback.
pub fn decide_worktree_ready_readback(
	facts: &ManagedRepositoryFacts,
	operation: &OperationView,
	evidence: &WorktreeReadyEvidence,
) -> Result<WorktreeReadyReconciliation, ManagedRepositoryError> {
	validate_reconciliation(
		facts,
		operation,
		ManagedRepositoryPhase::Registered,
		RepositoryOperationKind::WorktreeReady,
	)?;
	let CanonicalOperationPayload::WorktreeReady { expected_head, .. } =
		&operation.descriptor.payload
	else {
		return Err(ManagedRepositoryError::OperationKindMismatch);
	};
	if facts.head != *expected_head {
		return Err(ManagedRepositoryError::OperationContextMismatch);
	}
	if matches!(evidence, WorktreeReadyEvidence::Unavailable) {
		return Ok(WorktreeReadyReconciliation::Pending);
	}
	let ambiguity = match evidence {
		WorktreeReadyEvidence::Exact(observed) =>
			worktree_ready_mismatch(facts, operation, expected_head, observed),
		WorktreeReadyEvidence::NoEffect => Some(RepositoryAmbiguity::NoEffect),
		WorktreeReadyEvidence::Incomplete => Some(RepositoryAmbiguity::Incomplete),
		WorktreeReadyEvidence::Stale => Some(RepositoryAmbiguity::Stale),
		WorktreeReadyEvidence::Foreign => Some(RepositoryAmbiguity::Foreign),
		WorktreeReadyEvidence::Replaced => Some(RepositoryAmbiguity::Replaced),
		WorktreeReadyEvidence::Dirty => Some(RepositoryAmbiguity::Dirty),
		WorktreeReadyEvidence::Rollback => Some(RepositoryAmbiguity::Rollback),
		WorktreeReadyEvidence::Inconclusive => Some(RepositoryAmbiguity::Inconclusive),
		WorktreeReadyEvidence::Unavailable => unreachable!(),
	};
	Ok(match ambiguity {
		Some(reason) => WorktreeReadyReconciliation::Ambiguous {
			operation: ambiguous_operation(operation, reason),
			repository: terminal_update(
				operation,
				ManagedRepositoryPhase::Ambiguous(reason),
				facts.head.clone(),
			),
			evidence: evidence.clone(),
		},
		None => WorktreeReadyReconciliation::Completed {
			operation: completed_operation(
				operation,
				RepositoryOperationResult::WorktreeReady { head: expected_head.clone() },
			),
			repository: terminal_update(
				operation,
				ManagedRepositoryPhase::Ready,
				expected_head.clone(),
			),
			evidence: evidence.clone(),
		},
	})
}

/// Reconcile Commit from exact transition-specific readback.
pub fn decide_commit_readback(
	facts: &ManagedRepositoryFacts,
	operation: &OperationView,
	evidence: &CommitEvidence,
) -> Result<CommitReconciliation, ManagedRepositoryError> {
	validate_reconciliation(
		facts,
		operation,
		ManagedRepositoryPhase::Ready,
		RepositoryOperationKind::Commit,
	)?;
	let CanonicalOperationPayload::Commit { expected_head, next_head, intent } =
		&operation.descriptor.payload
	else {
		return Err(ManagedRepositoryError::OperationKindMismatch);
	};
	if facts.head != *expected_head {
		return Err(ManagedRepositoryError::OperationContextMismatch);
	}
	if matches!(evidence, CommitEvidence::Unavailable) {
		return Ok(CommitReconciliation::Pending);
	}
	let ambiguity = match evidence {
		CommitEvidence::Exact(observed) =>
			commit_mismatch(facts, operation, expected_head, next_head, intent, observed),
		CommitEvidence::NoEffect => Some(RepositoryAmbiguity::NoEffect),
		CommitEvidence::Incomplete => Some(RepositoryAmbiguity::Incomplete),
		CommitEvidence::Stale => Some(RepositoryAmbiguity::Stale),
		CommitEvidence::Foreign => Some(RepositoryAmbiguity::Foreign),
		CommitEvidence::Replaced => Some(RepositoryAmbiguity::Replaced),
		CommitEvidence::Dirty => Some(RepositoryAmbiguity::Dirty),
		CommitEvidence::Rollback => Some(RepositoryAmbiguity::Rollback),
		CommitEvidence::Inconclusive => Some(RepositoryAmbiguity::Inconclusive),
		CommitEvidence::Unavailable => unreachable!(),
	};
	Ok(match ambiguity {
		Some(reason) => CommitReconciliation::Ambiguous {
			operation: ambiguous_operation(operation, reason),
			repository: terminal_update(
				operation,
				ManagedRepositoryPhase::Ambiguous(reason),
				facts.head.clone(),
			),
			evidence: evidence.clone(),
		},
		None => CommitReconciliation::Completed {
			operation: completed_operation(
				operation,
				RepositoryOperationResult::Committed {
					from: expected_head.clone(),
					to: next_head.clone(),
				},
			),
			repository: terminal_update(
				operation,
				ManagedRepositoryPhase::Ready,
				next_head.clone(),
			),
			evidence: evidence.clone(),
		},
	})
}

/// Pure contract rejection. Infrastructure and transaction-outcome errors belong to adapters.
#[allow(missing_docs)] // Display supplies the stable public rejection description for each variant.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManagedRepositoryError {
	InvalidRepositoryId,
	InvalidAllocationId,
	InvalidWorktreeId,
	InvalidOperationId,
	InvalidEvidenceId,
	InvalidAuthorityTip,
	InvalidAdmittedIdentity,
	InvalidContentRevision,
	InvalidReferenceName,
	InvalidCommitActorName,
	InvalidCommitActorEmail,
	InvalidRegistrationId,
	InvalidDescriptorDigest,
	InvalidAbsolutePath,
	InvalidPathObservation,
	MissingPathObservation,
	InvalidGitLayout,
	InvalidAdmissionDescriptor,
	InvalidCheckpoint,
	InvalidExecutorContractVersion,
	InvalidCommitMessage,
	InvalidCommitActor,
	InvalidAllocationTarget,
	AvailabilityMismatch,
	AllocationEvidenceMismatch,
	StaleCheckpoint,
	ActiveOperation,
	WrongPhase,
	HeadPreconditionMismatch,
	HeadDidNotAdvance,
	OperationNotPossiblyEffected,
	OperationKindMismatch,
	OperationContextMismatch,
}
impl Error for ManagedRepositoryError {}
impl Display for ManagedRepositoryError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(match self {
			Self::InvalidRepositoryId => "invalid managed repository identity",
			Self::InvalidAllocationId => "invalid repository allocation identity",
			Self::InvalidWorktreeId => "invalid managed worktree identity",
			Self::InvalidOperationId => "invalid repository operation identity",
			Self::InvalidEvidenceId => "invalid repository evidence identity",
			Self::InvalidAuthorityTip => "invalid repository authority tip",
			Self::InvalidAdmittedIdentity => "invalid admitted repository identity",
			Self::InvalidContentRevision => "invalid repository content revision",
			Self::InvalidReferenceName => "invalid repository reference name",
			Self::InvalidCommitActorName => "invalid repository commit actor name",
			Self::InvalidCommitActorEmail => "invalid repository commit actor email",
			Self::InvalidRegistrationId => "invalid repository registration identity",
			Self::InvalidDescriptorDigest => "invalid admission descriptor digest",
			Self::InvalidAbsolutePath => "invalid persisted absolute path",
			Self::InvalidPathObservation => "invalid repository path observation",
			Self::MissingPathObservation => "repository admission path observation is missing",
			Self::InvalidGitLayout => "invalid admitted Git layout",
			Self::InvalidAdmissionDescriptor => "invalid repository admission descriptor",
			Self::InvalidCheckpoint => "invalid aggregate checkpoint",
			Self::InvalidExecutorContractVersion => "invalid executor contract version",
			Self::InvalidCommitMessage => "invalid repository commit message",
			Self::InvalidCommitActor => "invalid repository commit actor",
			Self::InvalidAllocationTarget => "repository and worktree paths must differ",
			Self::AvailabilityMismatch => "allocation availability facts do not match command",
			Self::AllocationEvidenceMismatch => "positive allocation evidence is not exact",
			Self::StaleCheckpoint => "aggregate checkpoint is stale",
			Self::ActiveOperation => "repository already has an active operation",
			Self::WrongPhase => "repository is in the wrong transition phase",
			Self::HeadPreconditionMismatch => "repository head precondition does not match",
			Self::HeadDidNotAdvance => "commit target does not advance the exact head",
			Self::OperationNotPossiblyEffected => "operation is not readback-only",
			Self::OperationKindMismatch => "operation kind does not match transition",
			Self::OperationContextMismatch => "operation does not match repository facts",
		})
	}
}

fn descriptor(
	facts: &ManagedRepositoryFacts,
	operation_id: RepositoryOperationId,
	payload: CanonicalOperationPayload,
	executor_contract: ExecutorContractVersion,
) -> CanonicalOperationDescriptor {
	let admission = &facts.admission.descriptor;
	CanonicalOperationDescriptor {
		schema: OperationDescriptorVersion::V1,
		operation_id,
		project_id: admission.project_id.clone(),
		repository_id: admission.repository_id.clone(),
		admitted_identity: admission.admitted_identity.clone(),
		admitted_base: admission.admitted_base.clone(),
		admission_descriptor_digest: admission.digest.clone(),
		allocation_id: facts.allocation_id.clone(),
		worktree_id: facts.worktree_id.clone(),
		repository_absolute_path: admission.repository_path.clone(),
		worktree_absolute_path: facts.worktree_path.clone(),
		expected_checkpoint: facts.checkpoint.clone(),
		kind: payload.kind(),
		payload,
		executor_contract,
	}
}

fn possibly_effected(descriptor: CanonicalOperationDescriptor) -> OperationView {
	OperationView { descriptor, state: RepositoryOperationState::PossiblyEffected }
}

fn validate_begin(
	facts: &ManagedRepositoryFacts,
	expected_checkpoint: &AggregateCheckpoint,
	expected_phase: ManagedRepositoryPhase,
) -> Result<(), ManagedRepositoryError> {
	if facts.checkpoint.generation == 0 || expected_checkpoint.generation == 0 {
		return Err(ManagedRepositoryError::InvalidCheckpoint);
	}
	if facts.checkpoint != *expected_checkpoint {
		return Err(ManagedRepositoryError::StaleCheckpoint);
	}
	if facts.admission.descriptor.repository_path == facts.worktree_path {
		return Err(ManagedRepositoryError::OperationContextMismatch);
	}
	if facts.active_operation.is_some() {
		return Err(ManagedRepositoryError::ActiveOperation);
	}
	if facts.phase != expected_phase {
		return Err(ManagedRepositoryError::WrongPhase);
	}
	Ok(())
}

fn validate_commit_actor(actor: &RepositoryCommitActor) -> Result<(), ManagedRepositoryError> {
	if !(-1_439..=1_439).contains(&actor.utc_offset_minutes) {
		return Err(ManagedRepositoryError::InvalidCommitActor);
	}
	Ok(())
}

fn validate_readback(
	operation: &OperationView,
	expected_kind: RepositoryOperationKind,
) -> Result<(), ManagedRepositoryError> {
	if operation.state != RepositoryOperationState::PossiblyEffected {
		return Err(ManagedRepositoryError::OperationNotPossiblyEffected);
	}
	if operation.descriptor.kind != expected_kind
		|| operation.descriptor.payload.kind() != expected_kind
	{
		return Err(ManagedRepositoryError::OperationKindMismatch);
	}
	match &operation.descriptor.payload {
		CanonicalOperationPayload::Register { target, .. }
			if target.repository_id != operation.descriptor.repository_id
				|| target.worktree_id != operation.descriptor.worktree_id
				|| target.repository_path != operation.descriptor.repository_absolute_path
				|| target.worktree_path != operation.descriptor.worktree_absolute_path =>
		{
			return Err(ManagedRepositoryError::OperationContextMismatch);
		},
		CanonicalOperationPayload::Commit { expected_head, next_head, .. }
			if expected_head == next_head =>
		{
			return Err(ManagedRepositoryError::OperationContextMismatch);
		},
		_ => {},
	}
	Ok(())
}

fn validate_reconciliation(
	facts: &ManagedRepositoryFacts,
	operation: &OperationView,
	expected_phase: ManagedRepositoryPhase,
	expected_kind: RepositoryOperationKind,
) -> Result<(), ManagedRepositoryError> {
	validate_readback(operation, expected_kind)?;
	let admission = &facts.admission.descriptor;
	if facts.phase != expected_phase {
		return Err(ManagedRepositoryError::WrongPhase);
	}
	if facts.active_operation.as_ref() != Some(&operation.descriptor.operation_id)
		|| admission.project_id != operation.descriptor.project_id
		|| admission.repository_id != operation.descriptor.repository_id
		|| admission.admitted_identity != operation.descriptor.admitted_identity
		|| admission.admitted_base != operation.descriptor.admitted_base
		|| admission.digest != operation.descriptor.admission_descriptor_digest
		|| admission.repository_path != operation.descriptor.repository_absolute_path
		|| facts.allocation_id != operation.descriptor.allocation_id
		|| facts.worktree_id != operation.descriptor.worktree_id
		|| facts.worktree_path != operation.descriptor.worktree_absolute_path
	{
		return Err(ManagedRepositoryError::OperationContextMismatch);
	}
	Ok(())
}

fn registration_mismatch(
	facts: &ManagedRepositoryFacts,
	operation: &OperationView,
	target: &RegistrationTarget,
	expected_head: &RepositoryContentRevision,
	observed: &ExactRegistrationEvidence,
) -> Option<RepositoryAmbiguity> {
	if let Some(reason) = positive_scope_mismatch(facts, operation, &observed.scope) {
		return Some(reason);
	}
	if observed.unchanged_head != *expected_head {
		return Some(RepositoryAmbiguity::Stale);
	}
	if observed.scope.repository_path != target.repository_path
		|| observed.scope.worktree_path != target.worktree_path
	{
		return Some(RepositoryAmbiguity::Replaced);
	}
	if observed.repository_names_worktree != target.worktree_id
		|| observed.worktree_names_repository != target.repository_id
	{
		return Some(RepositoryAmbiguity::Incomplete);
	}
	None
}

fn worktree_ready_mismatch(
	facts: &ManagedRepositoryFacts,
	operation: &OperationView,
	expected_head: &RepositoryContentRevision,
	observed: &ExactWorktreeReadyEvidence,
) -> Option<RepositoryAmbiguity> {
	if let Some(reason) = positive_scope_mismatch(facts, operation, &observed.scope) {
		return Some(reason);
	}
	if observed.unchanged_head != *expected_head {
		return Some(RepositoryAmbiguity::Stale);
	}
	None
}

fn commit_mismatch(
	facts: &ManagedRepositoryFacts,
	operation: &OperationView,
	expected_head: &RepositoryContentRevision,
	next_head: &RepositoryContentRevision,
	intent: &CanonicalCommitIntent,
	observed: &ExactCommitEvidence,
) -> Option<RepositoryAmbiguity> {
	if let Some(reason) = positive_scope_mismatch(facts, operation, &observed.scope) {
		return Some(reason);
	}
	if observed.predecessor_head != *expected_head {
		return Some(RepositoryAmbiguity::Stale);
	}
	if observed.target_reference != intent.target_reference
		|| observed.intent != *intent
		|| observed.completed_head != *next_head
	{
		return Some(RepositoryAmbiguity::Foreign);
	}
	None
}

fn positive_scope_mismatch(
	facts: &ManagedRepositoryFacts,
	operation: &OperationView,
	observed: &ExactRepositoryReadbackScope,
) -> Option<RepositoryAmbiguity> {
	let admission = &facts.admission.descriptor;
	if observed.operation_id != operation.descriptor.operation_id
		|| observed.admitted_identity != admission.admitted_identity
		|| observed.repository_id != admission.repository_id
		|| observed.allocation_id != facts.allocation_id
		|| observed.worktree_id != facts.worktree_id
	{
		return Some(RepositoryAmbiguity::Foreign);
	}
	if observed.repository_path != admission.repository_path
		|| observed.worktree_path != facts.worktree_path
	{
		return Some(RepositoryAmbiguity::Replaced);
	}
	if observed.admitted_base != admission.admitted_base {
		return Some(RepositoryAmbiguity::Stale);
	}
	None
}

fn completed_operation(
	operation: &OperationView,
	result: RepositoryOperationResult,
) -> OperationView {
	OperationView {
		descriptor: operation.descriptor.clone(),
		state: RepositoryOperationState::Completed(result),
	}
}

fn ambiguous_operation(operation: &OperationView, reason: RepositoryAmbiguity) -> OperationView {
	OperationView {
		descriptor: operation.descriptor.clone(),
		state: RepositoryOperationState::Ambiguous(reason),
	}
}

fn terminal_update(
	operation: &OperationView,
	phase: ManagedRepositoryPhase,
	head: RepositoryContentRevision,
) -> RepositoryProjectionUpdate {
	RepositoryProjectionUpdate {
		phase,
		head,
		clear_active_operation: operation.descriptor.operation_id.clone(),
	}
}

/// Canonical V1 bytes are the fixed ASCII magic below followed by fields in this exact order:
/// version (`u16`), project/repository/admitted identity/base/repository path (length-prefixed),
/// Git role (`u8`), optional registration ID, five layout paths, optional refs/commondir/backlink
/// paths, observation count (`u16`), then each observation's path, role count and role tags,
/// device, inode, object type, UID, and permissions. Lengths and UID are `u32`; device/inode are
/// `u64`; permissions are `u16`. Every integer is unsigned big-endian. Text/path lengths count
/// exact UTF-8 bytes. Options use one `u8` tag (`0` absent, `1` present) followed by the value only
/// when present. Enum tags are fixed by the functions below, not Rust discriminants.
const ADMISSION_DESCRIPTOR_V1_MAGIC: &[u8] = b"decodex/repository-admission-descriptor\0";

fn encode_admission_descriptor_v1(descriptor: &RepositoryAdmissionDescriptor) -> Vec<u8> {
	let layout = &descriptor.git_layout;
	let mut encoder = CanonicalV1Encoder::new();
	encoder.bytes.extend_from_slice(ADMISSION_DESCRIPTOR_V1_MAGIC);
	encoder.u16(1);
	encoder.text(descriptor.project_id.as_str());
	encoder.text(descriptor.repository_id.as_str());
	encoder.text(descriptor.admitted_identity.as_str());
	encoder.text(descriptor.admitted_base.as_str());
	encoder.path(&descriptor.repository_path);
	encoder.u8(git_registration_role_tag(layout.registration_role));
	encoder.optional_text(layout.registration_id.as_ref().map(RepositoryRegistrationId::as_str));
	encoder.path(&layout.repository_root);
	encoder.path(&layout.worktree_git_entry);
	encoder.path(&layout.git_directory);
	encoder.path(&layout.common_directory);
	encoder.path(&layout.objects_directory);
	encoder.optional_path(layout.refs_directory.as_ref());
	encoder.optional_path(layout.common_directory_file.as_ref());
	encoder.optional_path(layout.git_directory_backlink_file.as_ref());
	encoder.u16(layout_observation_count(&descriptor.observations));
	for observation in &descriptor.observations {
		encoder.observation_path(&observation.path);
		encoder.u8(observation.roles.len() as u8);
		for role in &observation.roles {
			encoder.u8(path_registration_role_tag(*role));
		}
		encoder.u64(observation.device);
		encoder.u64(observation.inode);
		encoder.u8(observed_object_type_tag(observation.object_type));
		encoder.u32(observation.owner_uid);
		encoder.u16(observation.permissions);
	}
	encoder.bytes
}

struct CanonicalV1Encoder {
	bytes: Vec<u8>,
}
impl CanonicalV1Encoder {
	fn new() -> Self {
		Self { bytes: Vec::new() }
	}

	fn u8(&mut self, value: u8) {
		self.bytes.push(value);
	}

	fn u16(&mut self, value: u16) {
		self.bytes.extend_from_slice(&value.to_be_bytes());
	}

	fn u32(&mut self, value: u32) {
		self.bytes.extend_from_slice(&value.to_be_bytes());
	}

	fn u64(&mut self, value: u64) {
		self.bytes.extend_from_slice(&value.to_be_bytes());
	}

	fn text(&mut self, value: &str) {
		self.u32(value.len() as u32);
		self.bytes.extend_from_slice(value.as_bytes());
	}

	fn path(&mut self, value: &PersistedAbsolutePath) {
		self.text(path_str(value));
	}

	fn observation_path(&mut self, value: &RepositoryObservationPath) {
		self.text(observation_path_str(value));
	}

	fn optional_text(&mut self, value: Option<&str>) {
		match value {
			Some(value) => {
				self.u8(1);
				self.text(value);
			},
			None => self.u8(0),
		}
	}

	fn optional_path(&mut self, value: Option<&PersistedAbsolutePath>) {
		match value {
			Some(value) => {
				self.u8(1);
				self.path(value);
			},
			None => self.u8(0),
		}
	}
}

fn layout_observation_count(observations: &[RepositoryPathObservation]) -> u16 {
	debug_assert!(observations.len() <= MAX_REPOSITORY_ADMISSION_OBSERVATIONS);
	observations.len() as u16
}

fn validate_git_layout(
	layout: &RepositoryAdmittedGitLayout,
	observations: &[RepositoryPathObservation],
) -> Result<(), ManagedRepositoryError> {
	let dot_git = layout.repository_root.as_path().join(".git");
	if layout.worktree_git_entry.as_path() != dot_git
		|| layout.objects_directory.as_path() != layout.common_directory.as_path().join("objects")
		|| layout
			.refs_directory
			.as_ref()
			.is_some_and(|path| path.as_path() != layout.common_directory.as_path().join("refs"))
	{
		return Err(ManagedRepositoryError::InvalidGitLayout);
	}

	match layout.registration_role {
		RepositoryGitRegistrationRole::PrimaryWorktree => {
			if layout.registration_id.is_some()
				|| layout.git_directory != layout.worktree_git_entry
				|| layout.common_directory != layout.git_directory
				|| layout.common_directory_file.is_some()
				|| layout.git_directory_backlink_file.is_some()
			{
				return Err(ManagedRepositoryError::InvalidGitLayout);
			}
		},
		RepositoryGitRegistrationRole::LinkedWorktree => {
			let Some(registration_id) = &layout.registration_id else {
				return Err(ManagedRepositoryError::InvalidGitLayout);
			};
			let expected_git_directory =
				layout.common_directory.as_path().join("worktrees").join(registration_id.as_str());
			let expected_common_file = layout.git_directory.as_path().join("commondir");
			let expected_backlink = layout.git_directory.as_path().join("gitdir");
			if layout.git_directory == layout.common_directory
				|| layout.git_directory.as_path() != expected_git_directory
				|| layout.common_directory_file.as_ref().map(PersistedAbsolutePath::as_path)
					!= Some(expected_common_file.as_path())
				|| layout.git_directory_backlink_file.as_ref().map(PersistedAbsolutePath::as_path)
					!= Some(expected_backlink.as_path())
			{
				return Err(ManagedRepositoryError::InvalidGitLayout);
			}
		},
	}

	require_directory_chain(
		observations,
		&layout.repository_root,
		RepositoryPathRegistrationRole::RepositoryRootComponent,
		RepositoryPathRegistrationRole::RepositoryRoot,
	)?;
	require_directory_chain(
		observations,
		&layout.git_directory,
		RepositoryPathRegistrationRole::GitDirectoryComponent,
		RepositoryPathRegistrationRole::GitDirectory,
	)?;
	require_directory_chain(
		observations,
		&layout.common_directory,
		RepositoryPathRegistrationRole::GitCommonDirectoryComponent,
		RepositoryPathRegistrationRole::GitCommonDirectory,
	)?;
	require_directory_chain(
		observations,
		&layout.objects_directory,
		RepositoryPathRegistrationRole::GitObjectsDirectoryComponent,
		RepositoryPathRegistrationRole::GitObjectsDirectory,
	)?;
	if let Some(refs) = &layout.refs_directory {
		require_directory_chain(
			observations,
			refs,
			RepositoryPathRegistrationRole::GitRefsDirectoryComponent,
			RepositoryPathRegistrationRole::GitRefsDirectory,
		)?;
	}
	require_observation(
		observations,
		layout.worktree_git_entry.as_path(),
		RepositoryPathRegistrationRole::WorktreeGitEntry,
		match layout.registration_role {
			RepositoryGitRegistrationRole::PrimaryWorktree =>
				RepositoryObservedObjectType::Directory,
			RepositoryGitRegistrationRole::LinkedWorktree =>
				RepositoryObservedObjectType::RegularFile,
		},
	)?;
	if let Some(path) = &layout.common_directory_file {
		require_observation(
			observations,
			path.as_path(),
			RepositoryPathRegistrationRole::GitCommonDirectoryFile,
			RepositoryObservedObjectType::RegularFile,
		)?;
	}
	if let Some(path) = &layout.git_directory_backlink_file {
		require_observation(
			observations,
			path.as_path(),
			RepositoryPathRegistrationRole::GitDirectoryBacklinkFile,
			RepositoryObservedObjectType::RegularFile,
		)?;
	}

	if observations
		.iter()
		.flat_map(|observation| observation.roles.iter().map(move |role| (observation, *role)))
		.any(|(observation, role)| !role_matches_layout(observation, role, layout))
	{
		return Err(ManagedRepositoryError::InvalidPathObservation);
	}
	Ok(())
}

fn require_directory_chain(
	observations: &[RepositoryPathObservation],
	endpoint: &PersistedAbsolutePath,
	component_role: RepositoryPathRegistrationRole,
	endpoint_role: RepositoryPathRegistrationRole,
) -> Result<(), ManagedRepositoryError> {
	let mut path = PathBuf::from("/");
	let mut components = endpoint.as_path().components().peekable();
	if !matches!(components.next(), Some(Component::RootDir)) {
		return Err(ManagedRepositoryError::InvalidGitLayout);
	}
	require_observation(
		observations,
		RepositoryObservationPath::new(path.clone())?.as_path(),
		component_role,
		RepositoryObservedObjectType::Directory,
	)?;
	while let Some(Component::Normal(component)) = components.next() {
		path.push(component);
		let persisted = RepositoryObservationPath::new(path.clone())?;
		let role = if components.peek().is_none() { endpoint_role } else { component_role };
		require_observation(
			observations,
			persisted.as_path(),
			role,
			RepositoryObservedObjectType::Directory,
		)?;
	}
	Ok(())
}

fn require_observation(
	observations: &[RepositoryPathObservation],
	path: &Path,
	role: RepositoryPathRegistrationRole,
	object_type: RepositoryObservedObjectType,
) -> Result<(), ManagedRepositoryError> {
	if observations.iter().any(|observation| {
		observation.path.as_path() == path
			&& observation.object_type == object_type
			&& observation.roles.binary_search(&role).is_ok()
	}) {
		Ok(())
	} else {
		Err(ManagedRepositoryError::MissingPathObservation)
	}
}

fn role_matches_layout(
	observation: &RepositoryPathObservation,
	role: RepositoryPathRegistrationRole,
	layout: &RepositoryAdmittedGitLayout,
) -> bool {
	let directory_endpoint = |endpoint: &PersistedAbsolutePath, exact: bool| {
		observation.object_type == RepositoryObservedObjectType::Directory
			&& if exact {
				observation.path.as_path() == endpoint.as_path()
			} else {
				observation.path.as_path() != endpoint.as_path()
					&& endpoint.as_path().starts_with(observation.path.as_path())
			}
	};
	match role {
		RepositoryPathRegistrationRole::RepositoryRootComponent =>
			directory_endpoint(&layout.repository_root, false),
		RepositoryPathRegistrationRole::RepositoryRoot =>
			directory_endpoint(&layout.repository_root, true),
		RepositoryPathRegistrationRole::WorktreeGitEntry =>
			observation.path.as_path() == layout.worktree_git_entry.as_path()
				&& observation.object_type
					== match layout.registration_role {
						RepositoryGitRegistrationRole::PrimaryWorktree =>
							RepositoryObservedObjectType::Directory,
						RepositoryGitRegistrationRole::LinkedWorktree =>
							RepositoryObservedObjectType::RegularFile,
					},
		RepositoryPathRegistrationRole::GitDirectoryComponent =>
			directory_endpoint(&layout.git_directory, false),
		RepositoryPathRegistrationRole::GitDirectory =>
			directory_endpoint(&layout.git_directory, true),
		RepositoryPathRegistrationRole::GitCommonDirectoryComponent =>
			directory_endpoint(&layout.common_directory, false),
		RepositoryPathRegistrationRole::GitCommonDirectory =>
			directory_endpoint(&layout.common_directory, true),
		RepositoryPathRegistrationRole::GitObjectsDirectoryComponent =>
			directory_endpoint(&layout.objects_directory, false),
		RepositoryPathRegistrationRole::GitObjectsDirectory =>
			directory_endpoint(&layout.objects_directory, true),
		RepositoryPathRegistrationRole::GitRefsDirectoryComponent =>
			layout.refs_directory.as_ref().is_some_and(|path| directory_endpoint(path, false)),
		RepositoryPathRegistrationRole::GitRefsDirectory =>
			layout.refs_directory.as_ref().is_some_and(|path| directory_endpoint(path, true)),
		RepositoryPathRegistrationRole::GitCommonDirectoryFile =>
			observation.object_type == RepositoryObservedObjectType::RegularFile
				&& layout
					.common_directory_file
					.as_ref()
					.is_some_and(|path| path.as_path() == observation.path.as_path()),
		RepositoryPathRegistrationRole::GitDirectoryBacklinkFile =>
			observation.object_type == RepositoryObservedObjectType::RegularFile
				&& layout
					.git_directory_backlink_file
					.as_ref()
					.is_some_and(|path| path.as_path() == observation.path.as_path()),
	}
}

fn strictly_ordered<T: Ord>(values: &[T]) -> bool {
	values.windows(2).all(|pair| pair[0] < pair[1])
}

fn path_str(path: &PersistedAbsolutePath) -> &str {
	path.as_path().to_str().expect("persisted paths are validated UTF-8")
}

fn observation_path_str(path: &RepositoryObservationPath) -> &str {
	path.as_path().to_str().expect("observation paths are validated UTF-8")
}

fn git_registration_role_tag(role: RepositoryGitRegistrationRole) -> u8 {
	match role {
		RepositoryGitRegistrationRole::PrimaryWorktree => 0,
		RepositoryGitRegistrationRole::LinkedWorktree => 1,
	}
}

fn observed_object_type_tag(object_type: RepositoryObservedObjectType) -> u8 {
	match object_type {
		RepositoryObservedObjectType::Directory => 0,
		RepositoryObservedObjectType::RegularFile => 1,
	}
}

fn path_registration_role_tag(role: RepositoryPathRegistrationRole) -> u8 {
	match role {
		RepositoryPathRegistrationRole::RepositoryRootComponent => 0,
		RepositoryPathRegistrationRole::RepositoryRoot => 1,
		RepositoryPathRegistrationRole::WorktreeGitEntry => 2,
		RepositoryPathRegistrationRole::GitDirectoryComponent => 3,
		RepositoryPathRegistrationRole::GitDirectory => 4,
		RepositoryPathRegistrationRole::GitCommonDirectoryComponent => 5,
		RepositoryPathRegistrationRole::GitCommonDirectory => 6,
		RepositoryPathRegistrationRole::GitObjectsDirectoryComponent => 7,
		RepositoryPathRegistrationRole::GitObjectsDirectory => 8,
		RepositoryPathRegistrationRole::GitRefsDirectoryComponent => 9,
		RepositoryPathRegistrationRole::GitRefsDirectory => 10,
		RepositoryPathRegistrationRole::GitCommonDirectoryFile => 11,
		RepositoryPathRegistrationRole::GitDirectoryBacklinkFile => 12,
	}
}

fn hex_sha256(bytes: &[u8]) -> String {
	let digest = Sha256::digest(bytes);
	let mut output = String::with_capacity(64);
	for byte in digest {
		use std::fmt::Write as _;
		let _ = write!(output, "{byte:02x}");
	}
	output
}

fn is_canonical_uuid_v4(value: &str) -> bool {
	let bytes = value.as_bytes();
	bytes.len() == 36
		&& bytes[8] == b'-'
		&& bytes[13] == b'-'
		&& bytes[18] == b'-'
		&& bytes[23] == b'-'
		&& bytes[14] == b'4'
		&& matches!(bytes[19], b'8' | b'9' | b'a' | b'b')
		&& bytes.iter().enumerate().all(|(index, byte)| {
			matches!(index, 8 | 13 | 18 | 23)
				|| byte.is_ascii_digit()
				|| (b'a'..=b'f').contains(byte)
		})
}

fn is_normalized_absolute(path: &Path) -> bool {
	let mut components = path.components();
	if !matches!(components.next(), Some(Component::RootDir)) {
		return false;
	}
	let mut saw_normal = false;
	let valid = components.all(|component| {
		if matches!(component, Component::Normal(_)) {
			saw_normal = true;
		}
		matches!(component, Component::Normal(value) if value != OsStr::new(""))
	});
	valid && saw_normal
}
