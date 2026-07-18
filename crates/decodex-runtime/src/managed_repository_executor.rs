//! Trusted-host managed-repository acquisition, effects, and exact readback.
//!
//! This module is deliberately crate-private. PostgreSQL and the future repository saga own
//! durable authority. The executor accepts only their complete canonical descriptors, remembers
//! every attempted operation for the lifetime of this daemon, and exposes separate readback-only
//! entry points. A descriptor, operation view, or readback request is never a dispatch receipt.
//!
//! V1 trusts one same-UID `decodexd` on one host. Descriptor retention and pre/post identity
//! checks detect replacement, but Git 2.54 still reopens absolute pathnames internally. A hostile
//! same-UID swap-and-restore during one Git invocation is therefore an accepted V1 residual risk,
//! not a confinement property.

#![allow(dead_code)] // XY-1351 owns the first in-crate composition of this private boundary.

use std::{
	collections::BTreeSet,
	ffi::{CString, OsStr, OsString},
	fs::{self, File, Metadata},
	io::{self, ErrorKind, Read, Write},
	os::unix::{
		ffi::OsStrExt as _,
		fs::{MetadataExt as _, PermissionsExt as _},
		io::{AsRawFd as _, FromRawFd as _},
		process::{CommandExt as _, ExitStatusExt as _},
	},
	path::{Component, Path, PathBuf},
	process::{Command, ExitStatus, Stdio},
	sync::{
		Arc,
		atomic::{AtomicBool, Ordering},
	},
	thread,
	time::{Duration, Instant},
};

use decodex_core::{
	CanonicalCommitIntent, CanonicalOperationDescriptor, CanonicalOperationPayload, CommitEvidence,
	CommitReadbackRequest, ExactCommitEvidence, ExactRegistrationEvidence,
	ExactRepositoryReadbackScope, ExactWorktreeReadyEvidence, OperationDescriptorVersion,
	PersistedAbsolutePath, PositiveAllocationEvidence, RegistrationEvidence,
	RegistrationReadbackRequest, RepositoryAdmissionFacts, RepositoryContentRevision,
	RepositoryEvidenceId, RepositoryOperationId, RepositoryOperationKind,
	WorktreeReadyEvidence, WorktreeReadyPolicy, WorktreeReadyReadbackRequest,
};
use sha2::{Digest as _, Sha256};

/// The only executor interpretation accepted by this source tree.
pub(crate) const EXECUTOR_CONTRACT_V1: u16 = 1;

const PINNED_GIT_PATH: &str =
	"/nix/store/01258rj9fvamcl4bf7yjffysmwyvd72i-git-2.54.0/bin/git";
const PINNED_GIT_VERSION: &str = "git version 2.54.0";
const PINNED_GIT_SHA256: &str =
	"b743c5b502287883caee7d2042f2b0400d58672f3f97ecead0a63e6fed7eaa46";
const NEUTRAL_CWD: &str = "/var/empty";
const DISABLED_EXECUTABLE: &str = "/usr/bin/false";
const PRIVATE_INDEX_NAME: &str = "decodex-index";
const MAX_GIT_OUTPUT_BYTES: usize = 4 * 1_024 * 1_024;
const MAX_CONFIG_OUTPUT_BYTES: usize = 64 * 1_024;
const MAX_COMMIT_OUTPUT_BYTES: usize = 64 * 1_024;
const MAX_METADATA_FILE_BYTES: usize = 64 * 1_024;
const GIT_TIMEOUT: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_millis(10);
const MAX_ATTRIBUTE_FILES: usize = 256;

/// Read-only Allocate acquisition input. Its evidence identity is persistence-owned data, not an
/// execution capability.
pub(crate) struct AllocationAcquisitionRequest<'a> {
	pub(crate) admission: &'a RepositoryAdmissionFacts,
	pub(crate) vacant_worktree_path: &'a PersistedAbsolutePath,
	pub(crate) evidence_id: RepositoryEvidenceId,
}

/// Closed acquisition refusal. Only a successful acquisition yields core positive evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AcquisitionFailure {
	InvalidPath,
	MissingRepository,
	UnsafeOwner,
	Replaced,
	ForeignRepository,
	UnsupportedRepository,
	TargetOccupied,
	GitUnavailable,
	GitFailed,
	OutputLimit,
	TimedOut,
	Inconclusive,
}

/// Deterministic classification for one consumed execution attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExecutionFailure {
	AlreadyAttempted,
	InvalidDescriptor,
	UnsupportedContract,
	PathUnavailable,
	UnsafeOwner,
	Replaced,
	ForeignRepository,
	UnsupportedRepository,
	TargetOccupied,
	PrivateIndexConflict,
	GitUnavailable,
	SpawnFailed,
	StdinFailed,
	TimedOut,
	OutputLimit,
	Exited(i32),
	Signaled(i32),
	UnexpectedOutput,
	PreconditionMismatch,
}

/// Result of the only external attempt for one operation ID. Failure never authorizes retry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExecutionAttempt {
	CompletedInvocation,
	ConsumedWithoutInvocation(ExecutionFailure),
	InvocationFailed(ExecutionFailure),
}

/// Sole in-process owner of managed-repository effects.
pub(crate) struct ManagedRepositoryExecutor {
	git: PinnedGit,
	attempted: BTreeSet<RepositoryOperationId>,
}

impl ManagedRepositoryExecutor {
	/// Verify the accepted Git bytes and neutral process paths without inspecting a repository.
	pub(crate) fn open() -> Result<Self, ExecutionFailure> {
		Ok(Self { git: PinnedGit::open()?, attempted: BTreeSet::new() })
	}

	/// Acquire strictly read-only positive evidence for Allocate.
	pub(crate) fn acquire_allocation(
		&self,
		request: AllocationAcquisitionRequest<'_>,
	) -> Result<PositiveAllocationEvidence, AcquisitionFailure> {
		let repository_path = request.admission.repository_path.as_path();
		let repository_pin = DirectoryPin::acquire(repository_path).map_err(map_acquisition_path)?;
		validate_repository_owner(&repository_pin).map_err(map_acquisition_path)?;
		let layout = RepositoryLayout::inspect(repository_path).map_err(map_acquisition_path)?;

		self.verify_repository_policy(&layout, repository_path)
			.map_err(map_acquisition_git)?;
		let head = self.read_revision(&layout, repository_path, OsStr::new("HEAD"))
			.map_err(map_acquisition_git)?;
		if head.as_str() != request.admission.admitted_base.as_str() {
			return Err(AcquisitionFailure::ForeignRepository);
		}

		let target_path = request.vacant_worktree_path.as_path();
		if !path_is_missing(target_path).map_err(map_acquisition_path)? {
			return Err(AcquisitionFailure::TargetOccupied);
		}
		let target_parent = vacant_target(target_path).map_err(map_acquisition_path)?;
		validate_repository_owner(&target_parent).map_err(map_acquisition_path)?;

		repository_pin.revalidate().map_err(map_acquisition_path)?;
		layout.revalidate().map_err(map_acquisition_path)?;
		target_parent.revalidate().map_err(map_acquisition_path)?;
		if !path_is_missing(target_path).map_err(map_acquisition_path)? {
			return Err(AcquisitionFailure::TargetOccupied);
		}
		self.git.revalidate().map_err(map_acquisition_git)?;

		Ok(PositiveAllocationEvidence {
			evidence_id: request.evidence_id,
			admitted_identity: request.admission.admitted_identity.clone(),
			admitted_base: request.admission.admitted_base.clone(),
			repository_path: request.admission.repository_path.clone(),
			vacant_worktree_path: request.vacant_worktree_path.clone(),
		})
	}

	/// Consume the one in-memory Register attempt for this daemon generation.
	pub(crate) fn execute_register(
		&mut self,
		descriptor: &CanonicalOperationDescriptor,
	) -> ExecutionAttempt {
		if let Err(error) = self.consume_descriptor(descriptor, RepositoryOperationKind::Register) {
			return ExecutionAttempt::ConsumedWithoutInvocation(error);
		}
		let CanonicalOperationPayload::Register { expected_head, target } = &descriptor.payload else {
			return ExecutionAttempt::ConsumedWithoutInvocation(ExecutionFailure::InvalidDescriptor);
		};
		if target.repository_path != descriptor.repository_absolute_path
			|| target.worktree_path != descriptor.worktree_absolute_path
			|| target.repository_id != descriptor.repository_id
			|| target.worktree_id != descriptor.worktree_id
		{
			return ExecutionAttempt::ConsumedWithoutInvocation(ExecutionFailure::InvalidDescriptor);
		}

		match path_is_missing(descriptor.worktree_absolute_path.as_path()) {
			Ok(true) => {},
			Ok(false) => {
				return ExecutionAttempt::ConsumedWithoutInvocation(ExecutionFailure::TargetOccupied)
			},
			Err(error) => {
				return ExecutionAttempt::ConsumedWithoutInvocation(ExecutionFailure::from(error))
			},
		}
		let prepared = match self.prepare_operation(descriptor, false) {
			Ok(prepared) => prepared,
			Err(error) => return ExecutionAttempt::ConsumedWithoutInvocation(error),
		};
		if prepared.head.as_str() != expected_head.as_str() {
			return ExecutionAttempt::ConsumedWithoutInvocation(
				ExecutionFailure::PreconditionMismatch,
			);
		}

		let arguments = vec![
			OsString::from("worktree"),
			OsString::from("add"),
			OsString::from("--no-checkout"),
			OsString::from("--detach"),
			OsString::from("--lock"),
			OsString::from("--no-relative-paths"),
			descriptor.worktree_absolute_path.as_path().as_os_str().to_owned(),
			OsString::from(expected_head.as_str()),
		];
		let result = self.run_git(&prepared.layout, None, &arguments, None, MAX_GIT_OUTPUT_BYTES);
		let attempt = self.finish_effect(prepared, result);
		if attempt != ExecutionAttempt::CompletedInvocation {
			return attempt;
		}
		match self.inspect_registered_target(descriptor) {
			Ok(target)
				if self
					.read_revision(
						&target.layout,
						descriptor.worktree_absolute_path.as_path(),
						OsStr::new("HEAD"),
					)
					.is_ok_and(|head| head.as_str() == expected_head.as_str())
					&& registration_directory_is_clean(
						descriptor.worktree_absolute_path.as_path(),
					)
					&& target.revalidate().is_ok() =>
			{
				ExecutionAttempt::CompletedInvocation
			},
			Err(ReadbackFailure::Replaced) => {
				ExecutionAttempt::InvocationFailed(ExecutionFailure::Replaced)
			},
			_ => ExecutionAttempt::InvocationFailed(ExecutionFailure::UnexpectedOutput),
		}
	}

	/// Consume the distinct Registered-to-Ready attempt without advancing HEAD.
	pub(crate) fn execute_worktree_ready(
		&mut self,
		descriptor: &CanonicalOperationDescriptor,
	) -> ExecutionAttempt {
		if let Err(error) =
			self.consume_descriptor(descriptor, RepositoryOperationKind::WorktreeReady)
		{
			return ExecutionAttempt::ConsumedWithoutInvocation(error);
		}
		let CanonicalOperationPayload::WorktreeReady { expected_head, policy } = &descriptor.payload
		else {
			return ExecutionAttempt::ConsumedWithoutInvocation(ExecutionFailure::InvalidDescriptor);
		};
		if *policy != WorktreeReadyPolicy::ExactCleanWorktree {
			return ExecutionAttempt::ConsumedWithoutInvocation(ExecutionFailure::InvalidDescriptor);
		}

		let prepared = match self.prepare_operation(descriptor, true) {
			Ok(prepared) => prepared,
			Err(error) => return ExecutionAttempt::ConsumedWithoutInvocation(error),
		};
		if prepared.head.as_str() != expected_head.as_str() {
			return ExecutionAttempt::ConsumedWithoutInvocation(
				ExecutionFailure::PreconditionMismatch,
			);
		}
		let index = prepared.layout.private_index();
		if adjacent_lock_exists(&index).unwrap_or(true) {
			return ExecutionAttempt::ConsumedWithoutInvocation(
				ExecutionFailure::PrivateIndexConflict,
			);
		}
		let arguments = vec![
			OsString::from("read-tree"),
			OsString::from("--reset"),
			OsString::from("-u"),
			OsString::from(expected_head.as_str()),
		];
		let result = self.run_git(
			&prepared.layout,
			Some(&index),
			&arguments,
			None,
			MAX_GIT_OUTPUT_BYTES,
		);
		let attempt = self.finish_effect(prepared, result);
		if attempt == ExecutionAttempt::CompletedInvocation
			&& (FilePin::acquire(&index).is_err() || adjacent_lock_exists(&index).unwrap_or(true))
		{
			ExecutionAttempt::InvocationFailed(ExecutionFailure::PrivateIndexConflict)
		} else {
			attempt
		}
	}

	/// Consume the exact H-to-H-prime Commit attempt using only the service-private index.
	pub(crate) fn execute_commit(
		&mut self,
		descriptor: &CanonicalOperationDescriptor,
	) -> ExecutionAttempt {
		if let Err(error) = self.consume_descriptor(descriptor, RepositoryOperationKind::Commit) {
			return ExecutionAttempt::ConsumedWithoutInvocation(error);
		}
		let CanonicalOperationPayload::Commit { expected_head, next_head, intent } =
			&descriptor.payload
		else {
			return ExecutionAttempt::ConsumedWithoutInvocation(ExecutionFailure::InvalidDescriptor);
		};
		if !valid_reference(intent.target_reference.as_str()) || !valid_commit_intent(intent) {
			return ExecutionAttempt::ConsumedWithoutInvocation(ExecutionFailure::InvalidDescriptor);
		}

		let prepared = match self.prepare_operation(descriptor, true) {
			Ok(prepared) => prepared,
			Err(error) => return ExecutionAttempt::ConsumedWithoutInvocation(error),
		};
		let index = prepared.layout.private_index();
		let _index_pin = match FilePin::acquire(&index) {
			Ok(pin) => pin,
			Err(_) => {
				return ExecutionAttempt::ConsumedWithoutInvocation(
					ExecutionFailure::PrivateIndexConflict,
				)
			},
		};
		if adjacent_lock_exists(&index).unwrap_or(true) {
			return ExecutionAttempt::ConsumedWithoutInvocation(
				ExecutionFailure::PrivateIndexConflict,
			);
		}
		let reference = match self.read_revision(
			&prepared.layout,
			descriptor.worktree_absolute_path.as_path(),
			OsStr::new(intent.target_reference.as_str()),
		) {
			Ok(reference) => reference,
			Err(error) => return ExecutionAttempt::ConsumedWithoutInvocation(error),
		};
		if reference.as_str() != expected_head.as_str() {
			return ExecutionAttempt::ConsumedWithoutInvocation(
				ExecutionFailure::PreconditionMismatch,
			);
		}

		let add = vec![OsString::from("add"), OsString::from("--all"), OsString::from("--")];
		if let Err(error) = self.run_git(&prepared.layout, Some(&index), &add, None, MAX_GIT_OUTPUT_BYTES)
		{
			return self.finish_effect(prepared, Err(error));
		}
		if !safe_private_index(&index) || adjacent_lock_exists(&index).unwrap_or(true) {
			return self.finish_effect(
				prepared,
				Err(ExecutionFailure::PrivateIndexConflict),
			);
		}
		let write_tree = vec![OsString::from("write-tree")];
		let tree = match self.run_git(
			&prepared.layout,
			Some(&index),
			&write_tree,
			None,
			MAX_COMMIT_OUTPUT_BYTES,
		) {
			Ok(output) => match parse_single_revision(&output.stdout) {
				Some(tree) if tree == intent.tree.as_str() => tree.to_owned(),
				_ => return self.finish_effect(prepared, Err(ExecutionFailure::UnexpectedOutput)),
			},
			Err(error) => return self.finish_effect(prepared, Err(error)),
		};
		if !safe_private_index(&index) || adjacent_lock_exists(&index).unwrap_or(true) {
			return self.finish_effect(
				prepared,
				Err(ExecutionFailure::PrivateIndexConflict),
			);
		}

		let commit_tree = vec![
			OsString::from("commit-tree"),
			OsString::from(tree),
			OsString::from("-p"),
			OsString::from(expected_head.as_str()),
			OsString::from("-F"),
			OsString::from("-"),
		];
		let commit_environment = commit_environment(intent);
		let commit = match self.run_git_with_environment(
			&prepared.layout,
			Some(&index),
			&commit_tree,
			Some(intent.message.as_str().as_bytes()),
			MAX_COMMIT_OUTPUT_BYTES,
			&commit_environment,
		) {
			Ok(output) => match parse_single_revision(&output.stdout) {
				Some(commit) if commit == next_head.as_str() => commit.to_owned(),
				_ => return self.finish_effect(prepared, Err(ExecutionFailure::UnexpectedOutput)),
			},
			Err(error) => return self.finish_effect(prepared, Err(error)),
		};
		if !safe_private_index(&index) || adjacent_lock_exists(&index).unwrap_or(true) {
			return self.finish_effect(
				prepared,
				Err(ExecutionFailure::PrivateIndexConflict),
			);
		}

		let update_ref = vec![
			OsString::from("update-ref"),
			OsString::from("--no-deref"),
			OsString::from(intent.target_reference.as_str()),
			OsString::from(commit),
			OsString::from(expected_head.as_str()),
		];
		let result = self.run_git(
			&prepared.layout,
			Some(&index),
			&update_ref,
			None,
			MAX_GIT_OUTPUT_BYTES,
		);
		let attempt = self.finish_effect(prepared, result);
		// `git add` may publish the private index by lock-and-rename, so replacement by the
		// admitted Git operation is expected here. Reacquire and verify the resulting private file.
		if !safe_private_index(&index) || adjacent_lock_exists(&index).unwrap_or(true) {
			ExecutionAttempt::InvocationFailed(ExecutionFailure::PrivateIndexConflict)
		} else {
			attempt
		}
	}

	/// Restart-safe registration readback. It has no call path to execution.
	pub(crate) fn read_registration(
		&self,
		request: &RegistrationReadbackRequest,
		evidence_id: RepositoryEvidenceId,
	) -> RegistrationEvidence {
		let descriptor = &request.descriptor;
		let CanonicalOperationPayload::Register { expected_head, target } = &descriptor.payload else {
			return RegistrationEvidence::Foreign;
		};
		if descriptor.kind != RepositoryOperationKind::Register
			|| target.repository_path != descriptor.repository_absolute_path
			|| target.worktree_path != descriptor.worktree_absolute_path
		{
			return RegistrationEvidence::Foreign;
		}
		let source = match self.inspect_readback_source(descriptor) {
			Ok(source) => source,
			Err(ReadbackFailure::Replaced) => return RegistrationEvidence::Replaced,
			Err(ReadbackFailure::Foreign) => return RegistrationEvidence::Foreign,
			Err(ReadbackFailure::Dirty) => return RegistrationEvidence::Dirty,
			Err(ReadbackFailure::Unavailable) => return RegistrationEvidence::Unavailable,
			Err(ReadbackFailure::Missing | ReadbackFailure::Incomplete) => {
				return RegistrationEvidence::Inconclusive;
			},
		};
		let source_head = match self.read_revision(
			&source.layout,
			descriptor.repository_absolute_path.as_path(),
			OsStr::new("HEAD"),
		) {
			Ok(head) => head,
			Err(ExecutionFailure::Replaced) => return RegistrationEvidence::Replaced,
			Err(_) => return RegistrationEvidence::Inconclusive,
		};
		if source_head.as_str() != expected_head.as_str() {
			return RegistrationEvidence::Stale;
		}
		if path_is_missing(descriptor.worktree_absolute_path.as_path()).unwrap_or(false) {
			return if registration_admin_for_path(
				&source.layout.common_dir,
				descriptor.worktree_absolute_path.as_path(),
			)
			.unwrap_or(false)
			{
				RegistrationEvidence::MissingReciprocal
			} else {
				RegistrationEvidence::NoEffect
			};
		}
		let target_layout = match RepositoryLayout::inspect(descriptor.worktree_absolute_path.as_path()) {
			Ok(layout) => layout,
			Err(PathFailure::Replaced) => return RegistrationEvidence::Replaced,
			Err(PathFailure::Missing) => return RegistrationEvidence::MissingReciprocal,
			Err(_) => return RegistrationEvidence::Inconclusive,
		};
		if target_layout.common_dir.identity() != source.layout.common_dir.identity()
			|| target_layout.backlink.as_deref()
				!= Some(descriptor.worktree_absolute_path.as_path().join(".git").as_path())
		{
			return RegistrationEvidence::MissingReciprocal;
		}
		if !safe_regular_file(&target_layout.locked_marker(), false) {
			return RegistrationEvidence::MissingReciprocal;
		}
		if !registration_directory_is_clean(descriptor.worktree_absolute_path.as_path()) {
			return RegistrationEvidence::Dirty;
		}
		let head = match self.read_revision(
			&target_layout,
			descriptor.worktree_absolute_path.as_path(),
			OsStr::new("HEAD"),
		) {
			Ok(head) => head,
			Err(ExecutionFailure::Replaced) => return RegistrationEvidence::Replaced,
			Err(_) => return RegistrationEvidence::Inconclusive,
		};
		if head.as_str() != expected_head.as_str() {
			return RegistrationEvidence::Stale;
		}
		if source.revalidate().is_err() || target_layout.revalidate().is_err() {
			return RegistrationEvidence::Replaced;
		}

		RegistrationEvidence::ExactReciprocal(ExactRegistrationEvidence {
			scope: readback_scope(descriptor, evidence_id),
			repository_names_worktree: descriptor.worktree_id.clone(),
			worktree_names_repository: descriptor.repository_id.clone(),
			unchanged_head: expected_head.clone(),
		})
	}

	/// Restart-safe WorktreeReady readback. It never prepares or repairs the index.
	pub(crate) fn read_worktree_ready(
		&self,
		request: &WorktreeReadyReadbackRequest,
		evidence_id: RepositoryEvidenceId,
	) -> WorktreeReadyEvidence {
		let descriptor = &request.descriptor;
		let CanonicalOperationPayload::WorktreeReady { expected_head, policy } = &descriptor.payload
		else {
			return WorktreeReadyEvidence::Foreign;
		};
		if descriptor.kind != RepositoryOperationKind::WorktreeReady
			|| *policy != WorktreeReadyPolicy::ExactCleanWorktree
		{
			return WorktreeReadyEvidence::Foreign;
		}
		let target = match self.inspect_registered_target(descriptor) {
			Ok(target) => target,
			Err(ReadbackFailure::Missing) => return WorktreeReadyEvidence::NoEffect,
			Err(ReadbackFailure::Incomplete) => return WorktreeReadyEvidence::Incomplete,
			Err(ReadbackFailure::Replaced) => return WorktreeReadyEvidence::Replaced,
			Err(ReadbackFailure::Foreign) => return WorktreeReadyEvidence::Foreign,
			Err(ReadbackFailure::Dirty) => return WorktreeReadyEvidence::Dirty,
			Err(ReadbackFailure::Unavailable) => return WorktreeReadyEvidence::Unavailable,
		};
		let head = match self.read_revision(
			&target.layout,
			descriptor.worktree_absolute_path.as_path(),
			OsStr::new("HEAD"),
		) {
			Ok(head) => head,
			Err(ExecutionFailure::Replaced) => return WorktreeReadyEvidence::Replaced,
			Err(error) => return worktree_readback_process_failure(error),
		};
		if head.as_str() != expected_head.as_str() {
			return if head.as_str() == descriptor.admitted_base.as_str()
				&& expected_head.as_str() != descriptor.admitted_base.as_str()
			{
				WorktreeReadyEvidence::Rollback
			} else {
				WorktreeReadyEvidence::Stale
			};
		}
		let index = target.layout.private_index();
		let index_pin = match FilePin::acquire(&index) {
			Ok(pin) => pin,
			Err(PathFailure::Missing) => {
				return if registration_directory_is_clean(
					descriptor.worktree_absolute_path.as_path(),
				) {
					WorktreeReadyEvidence::NoEffect
				} else {
					WorktreeReadyEvidence::Incomplete
				};
			},
			Err(PathFailure::Replaced) => return WorktreeReadyEvidence::Replaced,
			Err(PathFailure::UnsafeOwner) => return WorktreeReadyEvidence::Dirty,
			Err(_) => return WorktreeReadyEvidence::Incomplete,
		};
		match self.clean_against(
			&target.layout,
			descriptor.worktree_absolute_path.as_path(),
			&index,
			expected_head,
		) {
			Ok(true) => {},
			Ok(false) => return WorktreeReadyEvidence::Dirty,
			Err(ExecutionFailure::Replaced) => return WorktreeReadyEvidence::Replaced,
			Err(error) => return worktree_readback_process_failure(error),
		}
		if target.revalidate().is_err() || index_pin.revalidate().is_err() {
			return WorktreeReadyEvidence::Replaced;
		}

		WorktreeReadyEvidence::Exact(ExactWorktreeReadyEvidence {
			scope: readback_scope(descriptor, evidence_id),
			unchanged_head: expected_head.clone(),
		})
	}

	/// Restart-safe Commit readback. It observes one exact ref/commit/content advance only.
	pub(crate) fn read_commit(
		&self,
		request: &CommitReadbackRequest,
		evidence_id: RepositoryEvidenceId,
	) -> CommitEvidence {
		let descriptor = &request.descriptor;
		let CanonicalOperationPayload::Commit { expected_head, next_head, intent } =
			&descriptor.payload
		else {
			return CommitEvidence::Foreign;
		};
		if descriptor.kind != RepositoryOperationKind::Commit
			|| !valid_reference(intent.target_reference.as_str())
			|| !valid_commit_intent(intent)
		{
			return CommitEvidence::Foreign;
		}
		let target = match self.inspect_registered_target(descriptor) {
			Ok(target) => target,
			Err(ReadbackFailure::Missing) => return CommitEvidence::NoEffect,
			Err(ReadbackFailure::Incomplete) => return CommitEvidence::Incomplete,
			Err(ReadbackFailure::Replaced) => return CommitEvidence::Replaced,
			Err(ReadbackFailure::Foreign) => return CommitEvidence::Foreign,
			Err(ReadbackFailure::Dirty) => return CommitEvidence::Dirty,
			Err(ReadbackFailure::Unavailable) => return CommitEvidence::Unavailable,
		};
		let observed = match self.read_revision(
			&target.layout,
			descriptor.worktree_absolute_path.as_path(),
			OsStr::new(intent.target_reference.as_str()),
		) {
			Ok(observed) => observed,
			Err(ExecutionFailure::Replaced) => return CommitEvidence::Replaced,
			Err(error) => return commit_readback_process_failure(error),
		};
		if observed.as_str() == expected_head.as_str() {
			return CommitEvidence::NoEffect;
		}
		if observed.as_str() == descriptor.admitted_base.as_str()
			&& expected_head.as_str() != descriptor.admitted_base.as_str()
		{
			return CommitEvidence::Rollback;
		}
		if observed.as_str() != next_head.as_str() {
			return CommitEvidence::Foreign;
		}
		let raw_commit = match self.read_object(
			&target.layout,
			descriptor.worktree_absolute_path.as_path(),
			next_head,
		) {
			Ok(bytes) => bytes,
			Err(ExecutionFailure::Replaced) => return CommitEvidence::Replaced,
			Err(_) => return CommitEvidence::Incomplete,
		};
		if raw_commit != expected_commit_bytes(intent, expected_head) {
			return CommitEvidence::Foreign;
		}
		let index = target.layout.private_index();
		let index_pin = match FilePin::acquire(&index) {
			Ok(pin) => pin,
			Err(PathFailure::Replaced) => return CommitEvidence::Replaced,
			Err(PathFailure::UnsafeOwner) => return CommitEvidence::Dirty,
			Err(_) => return CommitEvidence::Incomplete,
		};
		match self.clean_against(
			&target.layout,
			descriptor.worktree_absolute_path.as_path(),
			&index,
			next_head,
		) {
			Ok(true) => {},
			Ok(false) => return CommitEvidence::Dirty,
			Err(ExecutionFailure::Replaced) => return CommitEvidence::Replaced,
			Err(error) => return commit_readback_process_failure(error),
		}
		if target.revalidate().is_err() || index_pin.revalidate().is_err() {
			return CommitEvidence::Replaced;
		}

		CommitEvidence::Exact(ExactCommitEvidence {
			scope: readback_scope(descriptor, evidence_id),
			target_reference: intent.target_reference.clone(),
			intent: intent.clone(),
			predecessor_head: expected_head.clone(),
			completed_head: next_head.clone(),
		})
	}

	fn consume_descriptor(
		&mut self,
		descriptor: &CanonicalOperationDescriptor,
		expected_kind: RepositoryOperationKind,
	) -> Result<(), ExecutionFailure> {
		if !self.attempted.insert(descriptor.operation_id.clone()) {
			return Err(ExecutionFailure::AlreadyAttempted);
		}
		if descriptor.schema != OperationDescriptorVersion::V1
			|| descriptor.executor_contract.get() != EXECUTOR_CONTRACT_V1
		{
			return Err(ExecutionFailure::UnsupportedContract);
		}
		if descriptor.kind != expected_kind || descriptor.payload.kind() != expected_kind {
			return Err(ExecutionFailure::InvalidDescriptor);
		}
		if descriptor.repository_absolute_path == descriptor.worktree_absolute_path {
			return Err(ExecutionFailure::InvalidDescriptor);
		}
		Ok(())
	}

	fn prepare_operation(
		&self,
		descriptor: &CanonicalOperationDescriptor,
		require_registered_target: bool,
	) -> Result<PreparedOperation, ExecutionFailure> {
		let repository_path = descriptor.repository_absolute_path.as_path();
		let repository_pin = DirectoryPin::acquire(repository_path)?;
		validate_repository_owner(&repository_pin)?;
		let source_layout = RepositoryLayout::inspect(repository_path)?;
		self.verify_repository_policy(&source_layout, repository_path)?;

		let (path_pin, layout, worktree_path) = if require_registered_target {
			let target = self.inspect_registered_target(descriptor).map_err(map_readback_execution)?;
			(target.path_pin, target.layout, descriptor.worktree_absolute_path.as_path().to_owned())
		} else {
			let parent = vacant_target(descriptor.worktree_absolute_path.as_path())?;
			validate_repository_owner(&parent)?;
			(parent, source_layout, repository_path.to_owned())
		};
		let head = self.read_revision(&layout, &worktree_path, OsStr::new("HEAD"))?;
		self.git.revalidate()?;

		Ok(PreparedOperation { repository_pin, path_pin, layout, head })
	}

	fn finish_effect(
		&self,
		prepared: PreparedOperation,
		result: Result<GitOutput, ExecutionFailure>,
	) -> ExecutionAttempt {
		let identity_result = prepared
			.repository_pin
			.revalidate()
			.and_then(|()| prepared.path_pin.revalidate())
			.and_then(|()| prepared.layout.revalidate())
			.map_err(ExecutionFailure::from)
			.and_then(|()| self.git.revalidate());
		if let Err(error) = identity_result {
			return ExecutionAttempt::InvocationFailed(error);
		}
		match result {
			Ok(_) => ExecutionAttempt::CompletedInvocation,
			Err(error) => ExecutionAttempt::InvocationFailed(error),
		}
	}

	fn inspect_readback_source(
		&self,
		descriptor: &CanonicalOperationDescriptor,
	) -> Result<InspectedTarget, ReadbackFailure> {
		let path = descriptor.repository_absolute_path.as_path();
		let path_pin = DirectoryPin::acquire(path).map_err(ReadbackFailure::from)?;
		validate_repository_owner(&path_pin).map_err(ReadbackFailure::from)?;
		let layout = RepositoryLayout::inspect(path).map_err(ReadbackFailure::from)?;
		self.verify_repository_policy(&layout, path).map_err(ReadbackFailure::from)?;
		Ok(InspectedTarget { path_pin, layout })
	}

	fn inspect_registered_target(
		&self,
		descriptor: &CanonicalOperationDescriptor,
	) -> Result<InspectedTarget, ReadbackFailure> {
		let source = self.inspect_readback_source(descriptor)?;
		let path = descriptor.worktree_absolute_path.as_path();
		let path_pin = DirectoryPin::acquire(path).map_err(ReadbackFailure::from)?;
		validate_repository_owner(&path_pin).map_err(ReadbackFailure::from)?;
		let layout = RepositoryLayout::inspect(path).map_err(ReadbackFailure::from)?;
		if layout.common_dir.identity() != source.layout.common_dir.identity()
			|| layout.backlink.as_deref() != Some(path.join(".git").as_path())
		{
			return Err(ReadbackFailure::Foreign);
		}
		if !safe_regular_file(&layout.locked_marker(), false) {
			return Err(ReadbackFailure::Incomplete);
		}
		self.verify_repository_policy(&layout, path).map_err(ReadbackFailure::from)?;
		Ok(InspectedTarget { path_pin, layout })
	}

	fn verify_repository_policy(
		&self,
		layout: &RepositoryLayout,
		worktree_path: &Path,
	) -> Result<(), ExecutionFailure> {
		if !safe_owned_regular_file(&layout.common_dir.path.join("config")) {
			return Err(ExecutionFailure::UnsupportedRepository);
		}
		let worktree_config = layout.git_dir.path.join("config.worktree");
		if worktree_config.exists() && !safe_owned_regular_file(&worktree_config) {
			return Err(ExecutionFailure::UnsupportedRepository);
		}
		let packed_refs = layout.common_dir.path.join("packed-refs");
		if packed_refs.exists() && !safe_owned_regular_file(&packed_refs) {
			return Err(ExecutionFailure::UnsupportedRepository);
		}
		for forbidden in [
			layout.common_dir.path.join("shallow"),
			layout.common_dir.path.join("info/grafts"),
			layout.common_dir.path.join("objects/info/alternates"),
			layout.common_dir.path.join("objects/info/http-alternates"),
			layout.common_dir.path.join("refs/replace"),
			layout.common_dir.path.join("modules"),
		] {
			if forbidden.exists() {
				return Err(ExecutionFailure::UnsupportedRepository);
			}
		}
		verify_hooks(&layout.common_dir.path.join("hooks"))?;
		verify_info_attributes(&layout.common_dir.path.join("info/attributes"))?;

		let config = vec![
			OsString::from("config"),
			OsString::from("--local"),
			OsString::from("--no-includes"),
			OsString::from("--null"),
			OsString::from("--list"),
		];
		let output = self.run_git(layout, None, &config, None, MAX_CONFIG_OUTPUT_BYTES)?;
		verify_local_config(&output.stdout)?;

		let tree = vec![
			OsString::from("ls-tree"),
			OsString::from("-r"),
			OsString::from("-z"),
			OsString::from("HEAD"),
		];
		let output = self.run_git(layout, None, &tree, None, MAX_GIT_OUTPUT_BYTES)?;
		let attributes = verify_tree_inventory(&output.stdout)?;
		for object in attributes {
			let blob = vec![
				OsString::from("cat-file"),
				OsString::from("blob"),
				OsString::from(object),
			];
			let output = self.run_git(layout, None, &blob, None, MAX_METADATA_FILE_BYTES)?;
			verify_attributes(&output.stdout)?;
		}
		layout.revalidate()?;
		if !worktree_path.is_absolute() {
			return Err(ExecutionFailure::PathUnavailable);
		}
		Ok(())
	}

	fn read_revision(
		&self,
		layout: &RepositoryLayout,
		worktree_path: &Path,
		reference: &OsStr,
	) -> Result<RepositoryContentRevision, ExecutionFailure> {
		let arguments = vec![
			OsString::from("rev-parse"),
			OsString::from("--verify"),
			OsString::from("--end-of-options"),
			reference.to_owned(),
		];
		let output = self.run_git_at(layout, worktree_path, None, &arguments, None, 256)?;
		let revision = parse_single_revision(&output.stdout).ok_or(ExecutionFailure::UnexpectedOutput)?;
		RepositoryContentRevision::new(revision.to_owned())
			.map_err(|_| ExecutionFailure::UnexpectedOutput)
	}

	fn read_object(
		&self,
		layout: &RepositoryLayout,
		worktree_path: &Path,
		revision: &RepositoryContentRevision,
	) -> Result<Vec<u8>, ExecutionFailure> {
		let arguments = vec![
			OsString::from("cat-file"),
			OsString::from("commit"),
			OsString::from(revision.as_str()),
		];
		Ok(self
			.run_git_at(
				layout,
				worktree_path,
				None,
				&arguments,
				None,
				MAX_COMMIT_OUTPUT_BYTES,
			)?
			.stdout)
	}

	fn clean_against(
		&self,
		layout: &RepositoryLayout,
		worktree_path: &Path,
		index: &Path,
		revision: &RepositoryContentRevision,
	) -> Result<bool, ExecutionFailure> {
		let cached = vec![
			OsString::from("diff-index"),
			OsString::from("--cached"),
			OsString::from("--quiet"),
			OsString::from("--no-ext-diff"),
			OsString::from(revision.as_str()),
			OsString::from("--"),
		];
		if !self.run_git_quiet(layout, worktree_path, index, &cached)? {
			return Ok(false);
		}
		let files = vec![
			OsString::from("diff-files"),
			OsString::from("--quiet"),
			OsString::from("--no-ext-diff"),
			OsString::from("--"),
		];
		if !self.run_git_quiet(layout, worktree_path, index, &files)? {
			return Ok(false);
		}
		let others = vec![
			OsString::from("ls-files"),
			OsString::from("--others"),
			OsString::from("-z"),
			OsString::from("--"),
		];
		let output = self.run_git_at(
			layout,
			worktree_path,
			Some(index),
			&others,
			None,
			MAX_GIT_OUTPUT_BYTES,
		)?;
		Ok(output.stdout.is_empty())
	}

	fn run_git_quiet(
		&self,
		layout: &RepositoryLayout,
		worktree_path: &Path,
		index: &Path,
		arguments: &[OsString],
	) -> Result<bool, ExecutionFailure> {
		match self.run_git_raw(layout, worktree_path, Some(index), arguments, None, 1, &[]) {
			Ok(output) if output.status.success() => Ok(true),
			Ok(output) if output.status.code() == Some(1) => Ok(false),
			Ok(output) => Err(classify_status(output.status)),
			Err(error) => Err(error),
		}
	}

	fn run_git(
		&self,
		layout: &RepositoryLayout,
		index: Option<&Path>,
		arguments: &[OsString],
		stdin: Option<&[u8]>,
		output_limit: usize,
	) -> Result<GitOutput, ExecutionFailure> {
		self.run_git_at(layout, layout.worktree_path.as_path(), index, arguments, stdin, output_limit)
	}

	fn run_git_at(
		&self,
		layout: &RepositoryLayout,
		worktree_path: &Path,
		index: Option<&Path>,
		arguments: &[OsString],
		stdin: Option<&[u8]>,
		output_limit: usize,
	) -> Result<GitOutput, ExecutionFailure> {
		let output = self.run_git_raw(
			layout,
			worktree_path,
			index,
			arguments,
			stdin,
			output_limit,
			&[],
		)?;
		if output.status.success() {
			Ok(output)
		} else {
			Err(classify_status(output.status))
		}
	}

	fn run_git_with_environment(
		&self,
		layout: &RepositoryLayout,
		index: Option<&Path>,
		arguments: &[OsString],
		stdin: Option<&[u8]>,
		output_limit: usize,
		environment: &[(OsString, OsString)],
	) -> Result<GitOutput, ExecutionFailure> {
		let output = self.run_git_raw(
			layout,
			layout.worktree_path.as_path(),
			index,
			arguments,
			stdin,
			output_limit,
			environment,
		)?;
		if output.status.success() {
			Ok(output)
		} else {
			Err(classify_status(output.status))
		}
	}

	#[allow(clippy::too_many_arguments)]
	fn run_git_raw(
		&self,
		layout: &RepositoryLayout,
		worktree_path: &Path,
		index: Option<&Path>,
		arguments: &[OsString],
		stdin: Option<&[u8]>,
		output_limit: usize,
		extra_environment: &[(OsString, OsString)],
	) -> Result<GitOutput, ExecutionFailure> {
		self.git.revalidate()?;
		layout.revalidate()?;
		let mut command = self.git.command();
		command
			.current_dir(NEUTRAL_CWD)
			.env_clear()
			.env("LC_ALL", "C")
			.env("TZ", "UTC")
			.env("HOME", NEUTRAL_CWD)
			.env("XDG_CONFIG_HOME", NEUTRAL_CWD)
			.env("PATH", "/usr/bin:/bin")
			.env("GIT_CONFIG_NOSYSTEM", "1")
			.env("GIT_CONFIG_SYSTEM", "/dev/null")
			.env("GIT_CONFIG_GLOBAL", "/dev/null")
			.env("GIT_TERMINAL_PROMPT", "0")
			.env("GCM_INTERACTIVE", "never")
			.env("GIT_ASKPASS", DISABLED_EXECUTABLE)
			.env("SSH_ASKPASS", DISABLED_EXECUTABLE)
			.env("GIT_SSH_COMMAND", DISABLED_EXECUTABLE)
			.env("GIT_EDITOR", DISABLED_EXECUTABLE)
			.env("GIT_SEQUENCE_EDITOR", DISABLED_EXECUTABLE)
			.env("GIT_PAGER", "")
			.env("PAGER", "")
			.env("GIT_OPTIONAL_LOCKS", "0")
			.env("GIT_DIR", &layout.git_dir.path)
			.env("GIT_COMMON_DIR", &layout.common_dir.path)
			.env("GIT_WORK_TREE", worktree_path)
			.stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
			.stdout(Stdio::piped())
			.stderr(Stdio::piped());
		if let Some(index) = index {
			command.env("GIT_INDEX_FILE", index);
		}
		for (key, value) in extra_environment {
			command.env(key, value);
		}
		for (key, value) in fixed_config() {
			command.arg("-c").arg(format!("{key}={value}"));
		}
		command
			.arg(format!("--git-dir={}", layout.git_dir.path.display()))
			.arg(format!("--work-tree={}", worktree_path.display()))
			.args(arguments);
		apply_child_limits(&mut command);
		let output = run_bounded(command, stdin, output_limit, GIT_TIMEOUT)?;
		layout.revalidate()?;
		self.git.revalidate()?;
		Ok(output)
	}
}

struct PreparedOperation {
	repository_pin: DirectoryPin,
	path_pin: DirectoryPin,
	layout: RepositoryLayout,
	head: RepositoryContentRevision,
}

struct InspectedTarget {
	path_pin: DirectoryPin,
	layout: RepositoryLayout,
}

impl InspectedTarget {
	fn revalidate(&self) -> Result<(), PathFailure> {
		self.path_pin.revalidate()?;
		self.layout.revalidate()
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReadbackFailure {
	Missing,
	Incomplete,
	Replaced,
	Foreign,
	Dirty,
	Unavailable,
}

impl From<PathFailure> for ReadbackFailure {
	fn from(error: PathFailure) -> Self {
		match error {
			PathFailure::Missing => Self::Missing,
			PathFailure::Replaced => Self::Replaced,
			PathFailure::UnsafeOwner | PathFailure::Invalid | PathFailure::Io => Self::Unavailable,
		}
	}
}

impl From<ExecutionFailure> for ReadbackFailure {
	fn from(error: ExecutionFailure) -> Self {
		match error {
			ExecutionFailure::Replaced => Self::Replaced,
			ExecutionFailure::ForeignRepository | ExecutionFailure::UnsupportedRepository => Self::Foreign,
			ExecutionFailure::PathUnavailable => Self::Missing,
			ExecutionFailure::PrivateIndexConflict => Self::Dirty,
			_ => Self::Unavailable,
		}
	}
}

fn map_readback_execution(error: ReadbackFailure) -> ExecutionFailure {
	match error {
		ReadbackFailure::Missing => ExecutionFailure::PathUnavailable,
		ReadbackFailure::Replaced => ExecutionFailure::Replaced,
		ReadbackFailure::Foreign => ExecutionFailure::ForeignRepository,
		ReadbackFailure::Dirty => ExecutionFailure::PrivateIndexConflict,
		ReadbackFailure::Incomplete | ReadbackFailure::Unavailable => {
			ExecutionFailure::UnsupportedRepository
		},
	}
}

fn readback_scope(
	descriptor: &CanonicalOperationDescriptor,
	evidence_id: RepositoryEvidenceId,
) -> ExactRepositoryReadbackScope {
	ExactRepositoryReadbackScope {
		evidence_id,
		operation_id: descriptor.operation_id.clone(),
		admitted_identity: descriptor.admitted_identity.clone(),
		admitted_base: descriptor.admitted_base.clone(),
		repository_id: descriptor.repository_id.clone(),
		allocation_id: descriptor.allocation_id.clone(),
		worktree_id: descriptor.worktree_id.clone(),
		repository_path: descriptor.repository_absolute_path.clone(),
		worktree_path: descriptor.worktree_absolute_path.clone(),
	}
}

fn fixed_config() -> &'static [(&'static str, &'static str)] {
	&[
		("advice.detachedHead", "false"),
		("commit.gpgSign", "false"),
		("core.attributesFile", "/dev/null"),
		("core.autocrlf", "false"),
		("core.fsmonitor", "false"),
		("core.hooksPath", "/var/empty/decodex-no-hooks"),
		("core.safecrlf", "true"),
		("core.sparseCheckout", "false"),
		("core.sparseCheckoutCone", "false"),
		("core.untrackedCache", "false"),
		("credential.helper", ""),
		("credential.interactive", "never"),
		("diff.external", ""),
		("gc.auto", "0"),
		("maintenance.auto", "false"),
		("protocol.allow", "never"),
		("protocol.file.allow", "never"),
		("submodule.recurse", "false"),
		("tag.gpgSign", "false"),
	]
}

fn verify_local_config(bytes: &[u8]) -> Result<(), ExecutionFailure> {
	const ALLOWED: &[&str] = &[
		"core.bare",
		"core.filemode",
		"core.ignorecase",
		"core.logallrefupdates",
		"core.precomposeunicode",
		"core.protecthfs",
		"core.protectntfs",
		"core.repositoryformatversion",
		"extensions.objectformat",
		"extensions.refstorage",
		"extensions.worktreeconfig",
	];
	let mut seen = BTreeSet::new();
	for record in bytes.split(|byte| *byte == 0).filter(|record| !record.is_empty()) {
		let split = record.iter().position(|byte| *byte == b'\n').unwrap_or(record.len());
		let key = std::str::from_utf8(&record[..split])
			.map_err(|_| ExecutionFailure::UnsupportedRepository)?
			.to_ascii_lowercase();
		let value = std::str::from_utf8(record.get(split + 1..).unwrap_or_default())
			.map_err(|_| ExecutionFailure::UnsupportedRepository)?;
		if key.starts_with("include.")
			|| key.starts_with("includeif.")
			|| !ALLOWED.contains(&key.as_str())
			|| !allowed_config_value(&key, value)
			|| !seen.insert(key)
		{
			return Err(ExecutionFailure::UnsupportedRepository);
		}
	}
	if !seen.contains("core.repositoryformatversion") || !seen.contains("core.bare") {
		return Err(ExecutionFailure::UnsupportedRepository);
	}
	Ok(())
}

fn allowed_config_value(key: &str, value: &str) -> bool {
	match key {
		"core.repositoryformatversion" => matches!(value, "0" | "1"),
		"core.bare" => value == "false",
		"core.logallrefupdates" => matches!(value, "true" | "false" | "always"),
		"extensions.objectformat" => matches!(value, "sha1" | "sha256"),
		"extensions.refstorage" => matches!(value, "files" | "reftable"),
		"extensions.worktreeconfig" => value == "true",
		_ => matches!(value, "true" | "false"),
	}
}

fn verify_hooks(path: &Path) -> Result<(), ExecutionFailure> {
	match fs::read_dir(path) {
		Ok(entries) => {
			for entry in entries {
				let entry = entry.map_err(|_| ExecutionFailure::UnsupportedRepository)?;
				let name = entry.file_name();
				let name = name.to_str().ok_or(ExecutionFailure::UnsupportedRepository)?;
				let metadata = fs::symlink_metadata(entry.path())
					.map_err(|_| ExecutionFailure::UnsupportedRepository)?;
				if !name.ends_with(".sample") || !metadata.is_file() || metadata.file_type().is_symlink() {
					return Err(ExecutionFailure::UnsupportedRepository);
				}
			}
			Ok(())
		},
		Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
		Err(_) => Err(ExecutionFailure::UnsupportedRepository),
	}
}

fn verify_info_attributes(path: &Path) -> Result<(), ExecutionFailure> {
	match read_nofollow(path, MAX_METADATA_FILE_BYTES) {
		Ok(bytes) if bytes.iter().all(u8::is_ascii_whitespace) => Ok(()),
		Err(PathFailure::Missing) => Ok(()),
		_ => Err(ExecutionFailure::UnsupportedRepository),
	}
}

fn verify_tree_inventory(bytes: &[u8]) -> Result<Vec<String>, ExecutionFailure> {
	let mut attributes = Vec::new();
	for record in bytes.split(|byte| *byte == 0).filter(|record| !record.is_empty()) {
		let tab = record.iter().position(|byte| *byte == b'\t')
			.ok_or(ExecutionFailure::UnexpectedOutput)?;
		let header = std::str::from_utf8(&record[..tab])
			.map_err(|_| ExecutionFailure::UnexpectedOutput)?;
		let mut fields = header.split(' ');
		let mode = fields.next().ok_or(ExecutionFailure::UnexpectedOutput)?;
		let kind = fields.next().ok_or(ExecutionFailure::UnexpectedOutput)?;
		let object = fields.next().ok_or(ExecutionFailure::UnexpectedOutput)?;
		if fields.next().is_some() || mode == "160000" || kind == "commit" {
			return Err(ExecutionFailure::UnsupportedRepository);
		}
		let path = &record[tab + 1..];
		if path == b".gitmodules" || path.ends_with(b"/.gitmodules") {
			return Err(ExecutionFailure::UnsupportedRepository);
		}
		if path == b".gitattributes" || path.ends_with(b"/.gitattributes") {
			if attributes.len() == MAX_ATTRIBUTE_FILES {
				return Err(ExecutionFailure::OutputLimit);
			}
			attributes.push(object.to_owned());
		}
	}
	Ok(attributes)
}

fn verify_attributes(bytes: &[u8]) -> Result<(), ExecutionFailure> {
	let text = std::str::from_utf8(bytes).map_err(|_| ExecutionFailure::UnsupportedRepository)?;
	for line in text.lines() {
		let line = line.trim();
		if line.is_empty() || line.starts_with('#') {
			continue;
		}
		for attribute in line.split_ascii_whitespace().skip(1) {
			let name = attribute
				.trim_start_matches(['-', '!'])
				.split_once('=')
				.map_or(attribute.trim_start_matches(['-', '!']), |(name, _)| name);
			if !matches!(
				name,
				"binary" | "eol" | "export-ignore" | "export-subst" | "ident" | "text" | "whitespace"
			) {
				return Err(ExecutionFailure::UnsupportedRepository);
			}
		}
	}
	Ok(())
}

fn valid_reference(reference: &str) -> bool {
	if reference == "HEAD" {
		return true;
	}
	reference.starts_with("refs/heads/")
		&& !reference.ends_with('/')
		&& !reference.contains("..")
		&& !reference.contains("@{")
		&& !reference.bytes().any(|byte| {
			byte.is_ascii_control()
				|| byte.is_ascii_whitespace()
				|| matches!(byte, b'~' | b'^' | b':' | b'?' | b'*' | b'[' | b'\\')
		})
		&& reference.split('/').all(|part| {
			!part.is_empty() && !part.starts_with('.') && !part.ends_with('.') && !part.ends_with(".lock")
		})
}

fn valid_commit_intent(intent: &CanonicalCommitIntent) -> bool {
	intent.message.as_str().ends_with('\n')
		&& [&intent.author, &intent.committer].into_iter().all(|actor| {
			!actor.name.as_str().chars().any(|character| matches!(character, '<' | '>'))
				&& !actor.email.as_str().chars().any(|character| matches!(character, '<' | '>'))
				&& !actor.email.as_str().bytes().any(|byte| byte.is_ascii_whitespace())
		})
}

fn worktree_readback_process_failure(error: ExecutionFailure) -> WorktreeReadyEvidence {
	match error {
		ExecutionFailure::GitUnavailable
		| ExecutionFailure::SpawnFailed
		| ExecutionFailure::TimedOut => WorktreeReadyEvidence::Unavailable,
		ExecutionFailure::Replaced => WorktreeReadyEvidence::Replaced,
		_ => WorktreeReadyEvidence::Inconclusive,
	}
}

fn commit_readback_process_failure(error: ExecutionFailure) -> CommitEvidence {
	match error {
		ExecutionFailure::GitUnavailable
		| ExecutionFailure::SpawnFailed
		| ExecutionFailure::TimedOut => CommitEvidence::Unavailable,
		ExecutionFailure::Replaced => CommitEvidence::Replaced,
		_ => CommitEvidence::Inconclusive,
	}
}

fn commit_environment(intent: &CanonicalCommitIntent) -> Vec<(OsString, OsString)> {
	vec![
		(OsString::from("GIT_AUTHOR_NAME"), OsString::from(intent.author.name.as_str())),
		(OsString::from("GIT_AUTHOR_EMAIL"), OsString::from(intent.author.email.as_str())),
		(OsString::from("GIT_AUTHOR_DATE"), OsString::from(git_date(&intent.author))),
		(
			OsString::from("GIT_COMMITTER_NAME"),
			OsString::from(intent.committer.name.as_str()),
		),
		(
			OsString::from("GIT_COMMITTER_EMAIL"),
			OsString::from(intent.committer.email.as_str()),
		),
		(OsString::from("GIT_COMMITTER_DATE"), OsString::from(git_date(&intent.committer))),
	]
}

fn git_date(actor: &decodex_core::RepositoryCommitActor) -> String {
	format!("{} {}", actor.timestamp_seconds, git_offset(actor.utc_offset_minutes))
}

fn git_offset(minutes: i16) -> String {
	let sign = if minutes < 0 { '-' } else { '+' };
	let absolute = i32::from(minutes).abs();
	format!("{sign}{:02}{:02}", absolute / 60, absolute % 60)
}

fn expected_commit_bytes(
	intent: &CanonicalCommitIntent,
	parent: &RepositoryContentRevision,
) -> Vec<u8> {
	format!(
		"tree {}\nparent {}\nauthor {} <{}> {} {}\ncommitter {} <{}> {} {}\n\n{}",
		intent.tree.as_str(),
		parent.as_str(),
		intent.author.name.as_str(),
		intent.author.email.as_str(),
		intent.author.timestamp_seconds,
		git_offset(intent.author.utc_offset_minutes),
		intent.committer.name.as_str(),
		intent.committer.email.as_str(),
		intent.committer.timestamp_seconds,
		git_offset(intent.committer.utc_offset_minutes),
		intent.message.as_str(),
	)
	.into_bytes()
}

fn parse_single_revision(bytes: &[u8]) -> Option<&str> {
	let text = std::str::from_utf8(bytes).ok()?;
	let revision = text.strip_suffix('\n').unwrap_or(text);
	if !matches!(revision.len(), 40 | 64)
		|| !revision.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
	{
		return None;
	}
	Some(revision)
}

struct GitOutput {
	status: ExitStatus,
	stdout: Vec<u8>,
	#[allow(dead_code)]
	stderr: Vec<u8>,
}

fn run_bounded(
	mut command: Command,
	stdin: Option<&[u8]>,
	limit: usize,
	timeout: Duration,
) -> Result<GitOutput, ExecutionFailure> {
	let mut child = command.spawn().map_err(|_| ExecutionFailure::SpawnFailed)?;
	let stdout = child.stdout.take().ok_or(ExecutionFailure::SpawnFailed)?;
	let stderr = child.stderr.take().ok_or(ExecutionFailure::SpawnFailed)?;
	let exceeded = Arc::new(AtomicBool::new(false));
	let stdout_reader = bounded_reader(stdout, limit, Arc::clone(&exceeded));
	let stderr_reader = bounded_reader(stderr, limit, Arc::clone(&exceeded));
	if let Some(bytes) = stdin {
		let Some(mut writer) = child.stdin.take() else {
			terminate_process_group(&mut child);
			let _ = child.wait();
			let _ = stdout_reader.join();
			let _ = stderr_reader.join();
			return Err(ExecutionFailure::StdinFailed);
		};
		if writer.write_all(bytes).is_err() {
			terminate_process_group(&mut child);
			let _ = child.wait();
			let _ = stdout_reader.join();
			let _ = stderr_reader.join();
			return Err(ExecutionFailure::StdinFailed);
		}
	}
	drop(child.stdin.take());

	let deadline = Instant::now() + timeout;
	let status = loop {
		if exceeded.load(Ordering::Acquire) {
			terminate_process_group(&mut child);
			break child.wait().map_err(|_| ExecutionFailure::SpawnFailed)?;
		}
		if let Some(status) = child.try_wait().map_err(|_| ExecutionFailure::SpawnFailed)? {
			break status;
		}
		if Instant::now() >= deadline {
			terminate_process_group(&mut child);
			let _ = child.wait();
			let _ = stdout_reader.join();
			let _ = stderr_reader.join();
			return Err(ExecutionFailure::TimedOut);
		}
		thread::sleep(POLL_INTERVAL);
	};
	let stdout = stdout_reader.join().map_err(|_| ExecutionFailure::SpawnFailed)??;
	let stderr = stderr_reader.join().map_err(|_| ExecutionFailure::SpawnFailed)??;
	if exceeded.load(Ordering::Acquire) {
		return Err(ExecutionFailure::OutputLimit);
	}
	Ok(GitOutput { status, stdout, stderr })
}

fn bounded_reader(
	mut reader: impl Read + Send + 'static,
	limit: usize,
	exceeded: Arc<AtomicBool>,
) -> thread::JoinHandle<Result<Vec<u8>, ExecutionFailure>> {
	thread::spawn(move || {
		let mut output = Vec::with_capacity(limit.min(64 * 1_024));
		let mut buffer = [0_u8; 8 * 1_024];
		loop {
			let count = reader.read(&mut buffer).map_err(|_| ExecutionFailure::GitUnavailable)?;
			if count == 0 {
				break;
			}
			if output.len().saturating_add(count) > limit {
				exceeded.store(true, Ordering::Release);
			} else {
				output.extend_from_slice(&buffer[..count]);
			}
		}
		Ok(output)
	})
}

fn apply_child_limits(command: &mut Command) {
	// SAFETY: this closure runs after fork and before exec, performs only async-signal-safe libc
	// calls, and returns an io error without touching shared Rust state.
	unsafe {
		command.pre_exec(|| {
			libc::umask(0o077);
			if libc::setpgid(0, 0) == -1 {
				return Err(io::Error::last_os_error());
			}
			for (resource, current, maximum) in [
				(libc::RLIMIT_CPU, 30, 30),
				(libc::RLIMIT_FSIZE, 64 * 1_024 * 1_024, 64 * 1_024 * 1_024),
				(libc::RLIMIT_NOFILE, 64, 64),
			] {
				let limit = libc::rlimit { rlim_cur: current, rlim_max: maximum };
				if libc::setrlimit(resource, &limit) == -1 {
					return Err(io::Error::last_os_error());
				}
			}
			Ok(())
		});
	}
}

fn terminate_process_group(child: &mut std::process::Child) {
	let process_group = -(child.id() as i32);
	// SAFETY: the negative PID addresses only the process group created in `pre_exec`.
	unsafe {
		libc::kill(process_group, libc::SIGKILL);
	}
}

fn classify_status(status: ExitStatus) -> ExecutionFailure {
	match (status.code(), status.signal()) {
		(Some(code), _) => ExecutionFailure::Exited(code),
		(None, Some(signal)) => ExecutionFailure::Signaled(signal),
		(None, None) => ExecutionFailure::GitUnavailable,
	}
}

struct PinnedGit {
	path: PathBuf,
	file: File,
	identity: ObjectIdentity,
}

impl PinnedGit {
	fn open() -> Result<Self, ExecutionFailure> {
		verify_disabled_executable()?;
		let path = PathBuf::from(PINNED_GIT_PATH);
		let file = open_nofollow_file(&path).map_err(ExecutionFailure::from)?;
		let metadata = file.metadata().map_err(|_| ExecutionFailure::GitUnavailable)?;
		if !metadata.is_file()
			|| metadata.uid() != 0
			|| metadata.mode() & 0o022 != 0
			|| metadata.permissions().mode() & 0o111 == 0
		{
			return Err(ExecutionFailure::GitUnavailable);
		}
		let digest = digest_file(&file)?;
		if hex_digest(&digest) != PINNED_GIT_SHA256 {
			return Err(ExecutionFailure::GitUnavailable);
		}
		let _accepted_version = PINNED_GIT_VERSION;
		Ok(Self { path, identity: ObjectIdentity::from_metadata(&metadata), file })
	}

	fn command(&self) -> Command {
		Command::new(&self.path)
	}

	fn revalidate(&self) -> Result<(), ExecutionFailure> {
		let retained = self.file.metadata().map_err(|_| ExecutionFailure::GitUnavailable)?;
		if ObjectIdentity::from_metadata(&retained) != self.identity {
			return Err(ExecutionFailure::Replaced);
		}
		let reopened = open_nofollow_file(&self.path).map_err(ExecutionFailure::from)?;
		let current = reopened.metadata().map_err(|_| ExecutionFailure::GitUnavailable)?;
		if ObjectIdentity::from_metadata(&current) != self.identity {
			return Err(ExecutionFailure::Replaced);
		}
		Ok(())
	}
}

fn digest_file(file: &File) -> Result<[u8; 32], ExecutionFailure> {
	let mut source = file.try_clone().map_err(|_| ExecutionFailure::GitUnavailable)?;
	let mut hasher = Sha256::new();
	let mut buffer = [0_u8; 64 * 1_024];
	loop {
		let count = source.read(&mut buffer).map_err(|_| ExecutionFailure::GitUnavailable)?;
		if count == 0 {
			break;
		}
		hasher.update(&buffer[..count]);
	}
	Ok(hasher.finalize().into())
}

fn hex_digest(digest: &[u8; 32]) -> String {
	let mut output = String::with_capacity(64);
	for byte in digest {
		use std::fmt::Write as _;
		let _ = write!(output, "{byte:02x}");
	}
	output
}

fn verify_disabled_executable() -> Result<(), ExecutionFailure> {
	let file = open_nofollow_file(Path::new(DISABLED_EXECUTABLE))
		.map_err(ExecutionFailure::from)?;
	let metadata = file.metadata().map_err(|_| ExecutionFailure::GitUnavailable)?;
	if !metadata.is_file()
		|| metadata.uid() != 0
		|| metadata.mode() & 0o022 != 0
		|| metadata.permissions().mode() & 0o111 == 0
	{
		return Err(ExecutionFailure::GitUnavailable);
	}
	Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ObjectIdentity {
	device: u64,
	inode: u64,
	uid: u32,
	mode: u32,
}

impl ObjectIdentity {
	fn from_metadata(metadata: &Metadata) -> Self {
		Self {
			device: metadata.dev(),
			inode: metadata.ino(),
			uid: metadata.uid(),
			mode: metadata.mode(),
		}
	}
}

struct DirectoryPin {
	path: PathBuf,
	components: Vec<(File, ObjectIdentity)>,
}

struct FilePin {
	path: PathBuf,
	file: File,
	identity: ObjectIdentity,
}

impl FilePin {
	fn acquire(path: &Path) -> Result<Self, PathFailure> {
		let file = open_nofollow_file(path)?;
		let metadata = file.metadata().map_err(|_| PathFailure::Io)?;
		// SAFETY: `geteuid` has no arguments and no failure mode.
		let effective_uid = unsafe { libc::geteuid() };
		if !metadata.is_file() || metadata.uid() != effective_uid || metadata.mode() & 0o077 != 0 {
			return Err(PathFailure::UnsafeOwner);
		}
		Ok(Self {
			path: path.to_owned(),
			identity: ObjectIdentity::from_metadata(&metadata),
			file,
		})
	}

	fn revalidate(&self) -> Result<(), PathFailure> {
		let retained = self.file.metadata().map_err(|_| PathFailure::Replaced)?;
		if ObjectIdentity::from_metadata(&retained) != self.identity {
			return Err(PathFailure::Replaced);
		}
		let current = Self::acquire(&self.path)?;
		if current.identity != self.identity {
			return Err(PathFailure::Replaced);
		}
		Ok(())
	}
}

impl DirectoryPin {
	fn acquire(path: &Path) -> Result<Self, PathFailure> {
		let parts = normalized_absolute_components(path)?;
		let root = open_directory_absolute(Path::new("/"))?;
		let root_identity = ObjectIdentity::from_metadata(&root.metadata().map_err(|_| PathFailure::Io)?);
		let mut components = vec![(root, root_identity)];
		for part in parts {
			let parent = &components.last().expect("root descriptor exists").0;
			let child = openat_directory(parent, part)?;
			let identity = ObjectIdentity::from_metadata(&child.metadata().map_err(|_| PathFailure::Io)?);
			components.push((child, identity));
		}
		Ok(Self { path: path.to_owned(), components })
	}

	fn identity(&self) -> ObjectIdentity {
		self.components.last().expect("directory pin is never empty").1
	}

	fn revalidate(&self) -> Result<(), PathFailure> {
		for (file, expected) in &self.components {
			let metadata = file.metadata().map_err(|_| PathFailure::Replaced)?;
			if ObjectIdentity::from_metadata(&metadata) != *expected || !metadata.is_dir() {
				return Err(PathFailure::Replaced);
			}
		}
		let current = Self::acquire(&self.path)?;
		if current.components.len() != self.components.len()
			|| current
				.components
				.iter()
				.zip(&self.components)
				.any(|(current, expected)| current.1 != expected.1)
		{
			return Err(PathFailure::Replaced);
		}
		Ok(())
	}
}

struct RepositoryLayout {
	worktree_path: PathBuf,
	worktree: DirectoryPin,
	git_dir: DirectoryPin,
	common_dir: DirectoryPin,
	objects_dir: DirectoryPin,
	refs_dir: Option<DirectoryPin>,
	backlink: Option<PathBuf>,
}

impl RepositoryLayout {
	fn inspect(worktree_path: &Path) -> Result<Self, PathFailure> {
		let worktree = DirectoryPin::acquire(worktree_path)?;
		let dot_git = worktree_path.join(".git");
		let metadata = metadata_nofollow(&dot_git)?;
		let (git_dir_path, backlink_required) = if metadata.is_dir() {
			(dot_git.clone(), false)
		} else if metadata.is_file() {
			if !safe_owned_regular_file(&dot_git) {
				return Err(PathFailure::UnsafeOwner);
			}
			(parse_gitdir_file(&dot_git)?, true)
		} else {
			return Err(PathFailure::Invalid);
		};
		let git_dir = DirectoryPin::acquire(&git_dir_path)?;
		validate_repository_owner(&git_dir)?;
		let commondir_file = git_dir_path.join("commondir");
		let commondir_path = match read_nofollow(&commondir_file, 4_096) {
			Ok(bytes) => resolve_metadata_path(&git_dir_path, parse_metadata_line(&bytes)?)?,
			Err(PathFailure::Missing) => git_dir_path.clone(),
			Err(error) => return Err(error),
		};
		if commondir_file.exists() && !safe_owned_regular_file(&commondir_file) {
			return Err(PathFailure::UnsafeOwner);
		}
		let common_dir = DirectoryPin::acquire(&commondir_path)?;
		validate_repository_owner(&common_dir)?;
		let objects_dir = DirectoryPin::acquire(&commondir_path.join("objects"))?;
		validate_repository_owner(&objects_dir)?;
		let refs_path = commondir_path.join("refs");
		let refs_dir = if path_is_missing(&refs_path)? {
			None
		} else {
			let refs = DirectoryPin::acquire(&refs_path)?;
			validate_repository_owner(&refs)?;
			Some(refs)
		};
		let backlink = if backlink_required {
			let gitdir_file = git_dir_path.join("gitdir");
			if !safe_owned_regular_file(&gitdir_file) {
				return Err(PathFailure::UnsafeOwner);
			}
			let bytes = read_nofollow(&gitdir_file, 4_096)?;
			Some(resolve_metadata_path(&git_dir_path, parse_metadata_line(&bytes)?)?)
		} else {
			None
		};
		if backlink_required && backlink.as_deref() != Some(dot_git.as_path()) {
			return Err(PathFailure::Invalid);
		}
		if !safe_owned_regular_file(&git_dir_path.join("HEAD")) {
			return Err(PathFailure::UnsafeOwner);
		}
		Ok(Self {
			worktree_path: worktree_path.to_owned(),
			worktree,
			git_dir,
			common_dir,
			objects_dir,
			refs_dir,
			backlink,
		})
	}

	fn private_index(&self) -> PathBuf {
		self.git_dir.path.join(PRIVATE_INDEX_NAME)
	}

	fn locked_marker(&self) -> PathBuf {
		self.git_dir.path.join("locked")
	}

	fn revalidate(&self) -> Result<(), PathFailure> {
		self.worktree.revalidate()?;
		self.git_dir.revalidate()?;
		self.common_dir.revalidate()?;
		self.objects_dir.revalidate()?;
		if let Some(refs_dir) = &self.refs_dir {
			refs_dir.revalidate()?;
		}
		let current = Self::inspect(&self.worktree_path)?;
		if current.git_dir.identity() != self.git_dir.identity()
			|| current.common_dir.identity() != self.common_dir.identity()
			|| current.objects_dir.identity() != self.objects_dir.identity()
			|| current.refs_dir.as_ref().map(DirectoryPin::identity)
				!= self.refs_dir.as_ref().map(DirectoryPin::identity)
			|| current.backlink != self.backlink
		{
			return Err(PathFailure::Replaced);
		}
		Ok(())
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PathFailure {
	Missing,
	Invalid,
	UnsafeOwner,
	Replaced,
	Io,
}

impl From<PathFailure> for ExecutionFailure {
	fn from(error: PathFailure) -> Self {
		match error {
			PathFailure::Missing | PathFailure::Invalid | PathFailure::Io => Self::PathUnavailable,
			PathFailure::UnsafeOwner => Self::UnsafeOwner,
			PathFailure::Replaced => Self::Replaced,
		}
	}
}

fn map_acquisition_path(error: PathFailure) -> AcquisitionFailure {
	match error {
		PathFailure::Missing => AcquisitionFailure::MissingRepository,
		PathFailure::Invalid => AcquisitionFailure::InvalidPath,
		PathFailure::UnsafeOwner => AcquisitionFailure::UnsafeOwner,
		PathFailure::Replaced => AcquisitionFailure::Replaced,
		PathFailure::Io => AcquisitionFailure::Inconclusive,
	}
}

fn map_acquisition_git(error: ExecutionFailure) -> AcquisitionFailure {
	match error {
		ExecutionFailure::TimedOut => AcquisitionFailure::TimedOut,
		ExecutionFailure::OutputLimit => AcquisitionFailure::OutputLimit,
		ExecutionFailure::Replaced => AcquisitionFailure::Replaced,
		ExecutionFailure::ForeignRepository | ExecutionFailure::PreconditionMismatch => {
			AcquisitionFailure::ForeignRepository
		},
		ExecutionFailure::UnsupportedRepository => AcquisitionFailure::UnsupportedRepository,
		ExecutionFailure::GitUnavailable | ExecutionFailure::SpawnFailed => {
			AcquisitionFailure::GitUnavailable
		},
		ExecutionFailure::Exited(_) | ExecutionFailure::Signaled(_) => AcquisitionFailure::GitFailed,
		_ => AcquisitionFailure::Inconclusive,
	}
}

fn normalized_absolute_components(path: &Path) -> Result<Vec<&OsStr>, PathFailure> {
	let mut components = path.components();
	if !matches!(components.next(), Some(Component::RootDir)) {
		return Err(PathFailure::Invalid);
	}
	let mut parts = Vec::new();
	for component in components {
		match component {
			Component::Normal(part) if !part.is_empty() => parts.push(part),
			_ => return Err(PathFailure::Invalid),
		}
	}
	if parts.is_empty() {
		return Err(PathFailure::Invalid);
	}
	Ok(parts)
}

fn open_directory_absolute(path: &Path) -> Result<File, PathFailure> {
	let c_path = CString::new(path.as_os_str().as_bytes()).map_err(|_| PathFailure::Invalid)?;
	// SAFETY: `c_path` is NUL terminated; returned descriptor is uniquely owned by `File`.
	let descriptor = unsafe {
		libc::open(
			c_path.as_ptr(),
			libc::O_RDONLY | libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW,
		)
	};
	file_from_descriptor(descriptor)
}

fn openat_directory(parent: &File, name: &OsStr) -> Result<File, PathFailure> {
	let c_name = CString::new(name.as_bytes()).map_err(|_| PathFailure::Invalid)?;
	// SAFETY: `parent` is an open directory and `c_name` is one NUL-terminated component.
	let descriptor = unsafe {
		libc::openat(
			parent.as_raw_fd(),
			c_name.as_ptr(),
			libc::O_RDONLY | libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW,
		)
	};
	file_from_descriptor(descriptor)
}

fn file_from_descriptor(descriptor: i32) -> Result<File, PathFailure> {
	if descriptor == -1 {
		return Err(path_errno(io::Error::last_os_error()));
	}
	// SAFETY: `descriptor` is a newly opened descriptor and ownership transfers exactly once.
	Ok(unsafe { File::from_raw_fd(descriptor) })
}

fn open_nofollow_file(path: &Path) -> Result<File, PathFailure> {
	let parent = path.parent().ok_or(PathFailure::Invalid)?;
	let name = path.file_name().ok_or(PathFailure::Invalid)?;
	let parent = DirectoryPin::acquire(parent)?;
	let c_name = CString::new(name.as_bytes()).map_err(|_| PathFailure::Invalid)?;
	// SAFETY: the retained parent is a directory and the final component cannot be followed.
	let descriptor = unsafe {
		libc::openat(
			parent.components.last().expect("directory pin has root").0.as_raw_fd(),
			c_name.as_ptr(),
			libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
		)
	};
	let file = file_from_descriptor(descriptor)?;
	parent.revalidate()?;
	Ok(file)
}

fn metadata_nofollow(path: &Path) -> Result<Metadata, PathFailure> {
	let metadata = fs::symlink_metadata(path).map_err(path_io)?;
	if metadata.file_type().is_symlink() {
		return Err(PathFailure::Invalid);
	}
	Ok(metadata)
}

fn read_nofollow(path: &Path, limit: usize) -> Result<Vec<u8>, PathFailure> {
	let mut file = open_nofollow_file(path)?;
	let metadata = file.metadata().map_err(|_| PathFailure::Io)?;
	if !metadata.is_file() || metadata.len() > limit as u64 {
		return Err(PathFailure::Invalid);
	}
	let mut bytes = Vec::with_capacity(metadata.len() as usize);
	file.read_to_end(&mut bytes).map_err(|_| PathFailure::Io)?;
	Ok(bytes)
}

fn parse_gitdir_file(path: &Path) -> Result<PathBuf, PathFailure> {
	let bytes = read_nofollow(path, 4_096)?;
	let line = parse_metadata_line(&bytes)?;
	let value = line.strip_prefix("gitdir: ").ok_or(PathFailure::Invalid)?;
	let git_dir = PathBuf::from(value);
	if !git_dir.is_absolute() {
		return Err(PathFailure::Invalid);
	}
	normalize_absolute(&git_dir)
}

fn parse_metadata_line(bytes: &[u8]) -> Result<&str, PathFailure> {
	let text = std::str::from_utf8(bytes).map_err(|_| PathFailure::Invalid)?;
	let line = text.strip_suffix('\n').unwrap_or(text);
	if line.is_empty() || line.contains(['\n', '\r', '\0']) {
		return Err(PathFailure::Invalid);
	}
	Ok(line)
}

fn resolve_metadata_path(base: &Path, value: &str) -> Result<PathBuf, PathFailure> {
	let value = Path::new(value);
	if value.is_absolute() {
		normalize_absolute(value)
	} else {
		normalize_absolute(&base.join(value))
	}
}

fn normalize_absolute(path: &Path) -> Result<PathBuf, PathFailure> {
	let mut normalized = PathBuf::from("/");
	for component in path.components() {
		match component {
			Component::RootDir => normalized = PathBuf::from("/"),
			Component::Normal(part) => normalized.push(part),
			Component::ParentDir => {
				if !normalized.pop() {
					return Err(PathFailure::Invalid);
				}
			},
			Component::CurDir => {},
			Component::Prefix(_) => return Err(PathFailure::Invalid),
		}
	}
	if normalized == Path::new("/") {
		return Err(PathFailure::Invalid);
	}
	Ok(normalized)
}

fn validate_repository_owner(pin: &DirectoryPin) -> Result<(), PathFailure> {
	let identity = pin.identity();
	// SAFETY: `geteuid` has no arguments and no failure mode.
	let effective_uid = unsafe { libc::geteuid() };
	if identity.uid != effective_uid || identity.mode & 0o022 != 0 {
		return Err(PathFailure::UnsafeOwner);
	}
	Ok(())
}

fn vacant_target(path: &Path) -> Result<DirectoryPin, PathFailure> {
	let parent = path.parent().ok_or(PathFailure::Invalid)?;
	if path.file_name().is_none() || !path_is_missing(path)? {
		return Err(PathFailure::Invalid);
	}
	DirectoryPin::acquire(parent)
}

fn path_is_missing(path: &Path) -> Result<bool, PathFailure> {
	match fs::symlink_metadata(path) {
		Ok(_) => Ok(false),
		Err(error) if error.kind() == ErrorKind::NotFound => Ok(true),
		Err(_) => Err(PathFailure::Io),
	}
}

fn adjacent_lock_exists(index: &Path) -> Result<bool, PathFailure> {
	let mut lock = index.as_os_str().to_owned();
	lock.push(".lock");
	path_is_missing(Path::new(&lock)).map(|missing| !missing)
}

fn safe_private_index(path: &Path) -> bool {
	safe_regular_file(path, true)
}

fn safe_regular_file(path: &Path, require_private: bool) -> bool {
	let Ok(file) = open_nofollow_file(path) else {
		return false;
	};
	let Ok(metadata) = file.metadata() else {
		return false;
	};
	// SAFETY: `geteuid` has no arguments and no failure mode.
	let effective_uid = unsafe { libc::geteuid() };
	metadata.is_file()
		&& metadata.uid() == effective_uid
		&& (!require_private || metadata.mode() & 0o077 == 0)
}

fn safe_owned_regular_file(path: &Path) -> bool {
	let Ok(file) = open_nofollow_file(path) else {
		return false;
	};
	let Ok(metadata) = file.metadata() else {
		return false;
	};
	// SAFETY: `geteuid` has no arguments and no failure mode.
	let effective_uid = unsafe { libc::geteuid() };
	metadata.is_file() && metadata.uid() == effective_uid && metadata.mode() & 0o022 == 0
}

fn registration_directory_is_clean(path: &Path) -> bool {
	let Ok(entries) = fs::read_dir(path) else {
		return false;
	};
	let mut names = Vec::new();
	for entry in entries {
		let Ok(entry) = entry else {
			return false;
		};
		names.push(entry.file_name());
	}
	names == [OsString::from(".git")]
}

fn registration_admin_for_path(common_dir: &DirectoryPin, worktree_path: &Path) -> Result<bool, PathFailure> {
	let root = common_dir.path.join("worktrees");
	let entries = match fs::read_dir(root) {
		Ok(entries) => entries,
		Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
		Err(_) => return Err(PathFailure::Io),
	};
	let expected = worktree_path.join(".git");
	for entry in entries {
		let entry = entry.map_err(|_| PathFailure::Io)?;
		let path = entry.path().join("gitdir");
		if let Ok(bytes) = read_nofollow(&path, 4_096) {
			let value = resolve_metadata_path(entry.path().as_path(), parse_metadata_line(&bytes)?)?;
			if value == expected {
				return Ok(true);
			}
		}
	}
	Ok(false)
}

fn path_errno(error: io::Error) -> PathFailure {
	match error.raw_os_error() {
		Some(libc::ENOENT) => PathFailure::Missing,
		Some(code) if code == libc::ELOOP || code == libc::ENOTDIR => PathFailure::Invalid,
		_ => PathFailure::Io,
	}
}

fn path_io(error: io::Error) -> PathFailure {
	if error.kind() == ErrorKind::NotFound { PathFailure::Missing } else { PathFailure::Io }
}
