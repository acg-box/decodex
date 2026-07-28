//! Trusted-host managed-repository acquisition, effects, and exact readback.
//!
//! This module is deliberately crate-private. PostgreSQL and the future repository saga own
//! durable authority. The executor accepts only fresh affine dispatch receipts plus the complete
//! persisted admission descriptor, remembers
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
	fs::{self, File, Metadata, OpenOptions},
	io::{self, ErrorKind, Read, Write},
	os::unix::{
		ffi::OsStrExt as _,
		fs::{MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _},
		io::{AsRawFd as _, FromRawFd as _},
		process::{CommandExt as _, ExitStatusExt as _},
	},
	path::{Component, Path, PathBuf},
	process::{Command, ExitStatus, Stdio},
	thread,
	time::{Duration, Instant},
};

use decodex_core::{
	CanonicalCommitIntent, CanonicalOperationDescriptor, CanonicalOperationPayload, CommitEvidence,
	CommitReadbackRequest, ExactCommitEvidence, ExactRegistrationEvidence,
	ExactRepositoryReadbackScope, ExactWorktreeReadyEvidence, OperationDescriptorVersion,
	PersistedAbsolutePath, PositiveAllocationEvidence, RegistrationEvidence,
	RegistrationReadbackRequest, RepositoryAdmissionDescriptor,
	RepositoryAdmissionDescriptorVersion, RepositoryAdmissionFacts, RepositoryAdmittedGitLayout,
	RepositoryContentRevision, RepositoryEvidenceId, RepositoryGitRegistrationRole,
	RepositoryObservationPath, RepositoryObservedObjectType, RepositoryOperationId,
	RepositoryOperationKind, RepositoryPathObservation, RepositoryRegistrationId,
	WorktreeReadyEvidence, WorktreeReadyPolicy, WorktreeReadyReadbackRequest,
};
use decodex_postgres::RepositoryDispatchReceipt;
use sha2::{Digest as _, Sha256};

/// The only executor interpretation accepted by this source tree.
pub(crate) const EXECUTOR_CONTRACT_V1: u16 = 1;

const PINNED_GIT_PATH: &str = "/nix/store/01258rj9fvamcl4bf7yjffysmwyvd72i-git-2.54.0/bin/git";
const PINNED_GIT_VERSION: &str = "git version 2.54.0";
const PINNED_GIT_SHA256: &str = "b743c5b502287883caee7d2042f2b0400d58672f3f97ecead0a63e6fed7eaa46";
const NEUTRAL_CWD: &str = "/var/empty";
const DISABLED_EXECUTABLE: &str = "/usr/bin/false";
const PRIVATE_INDEX_NAME: &str = "decodex-index";
const PRIVATE_INDEX_OWNER_NAME: &str = "decodex-index.owner";
const MAX_GIT_OUTPUT_BYTES: usize = 4 * 1_024 * 1_024;
const MAX_CONFIG_OUTPUT_BYTES: usize = 64 * 1_024;
const MAX_COMMIT_OUTPUT_BYTES: usize = 64 * 1_024;
const MAX_METADATA_FILE_BYTES: usize = 64 * 1_024;
const MAX_GIT_EXECUTABLE_BYTES: u64 = 128 * 1_024 * 1_024;
const GIT_TIMEOUT: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_millis(10);
const MAX_PATH_COMPONENTS: usize = 128;
const MAX_WALK_FILES: usize = 100_000;
const MAX_WALK_DEPTH: usize = 128;
const MAX_WALK_BYTES: u64 = 4 * 1_024 * 1_024 * 1_024;
const WALK_TIMEOUT: Duration = Duration::from_secs(10);

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
		let expected_admission = request.admission.descriptor();
		let repository_path = expected_admission.repository_path().as_path();
		let repository_pin =
			DirectoryPin::acquire(repository_path).map_err(map_acquisition_path)?;
		validate_repository_owner(&repository_pin).map_err(map_acquisition_path)?;
		let layout = self.reacquire_admission(expected_admission).map_err(map_acquisition_git)?;

		self.verify_repository_policy(&layout, repository_path).map_err(map_acquisition_git)?;
		let head = self
			.read_revision(&layout, repository_path, OsStr::new("HEAD"))
			.map_err(map_acquisition_git)?;
		if head.as_str() != expected_admission.admitted_base().as_str() {
			return Err(AcquisitionFailure::ForeignRepository);
		}

		let target_path = request.vacant_worktree_path.as_path();
		if !path_is_missing(target_path).map_err(map_acquisition_path)? {
			return Err(AcquisitionFailure::TargetOccupied);
		}
		let target_parent = vacant_target(target_path).map_err(map_acquisition_path)?;
		validate_repository_owner(&target_parent).map_err(map_acquisition_path)?;
		require_allocation_registration_vacancy(&layout.common_dir, target_path)?;

		repository_pin.revalidate().map_err(map_acquisition_path)?;
		layout.revalidate().map_err(map_acquisition_path)?;
		target_parent.revalidate().map_err(map_acquisition_path)?;
		if !path_is_missing(target_path).map_err(map_acquisition_path)? {
			return Err(AcquisitionFailure::TargetOccupied);
		}
		require_allocation_registration_vacancy(&layout.common_dir, target_path)?;
		self.git.revalidate().map_err(map_acquisition_git)?;

		Ok(PositiveAllocationEvidence::new(
			request.evidence_id,
			expected_admission.clone(),
			request.vacant_worktree_path.clone(),
		))
	}

	/// Consume the one in-memory Register attempt for this daemon generation.
	pub(crate) fn execute_register(
		&mut self,
		receipt: RepositoryDispatchReceipt,
		admission: &RepositoryAdmissionFacts,
	) -> ExecutionAttempt {
		let descriptor =
			match self.consume_receipt(receipt, admission, RepositoryOperationKind::Register) {
				Ok(descriptor) => descriptor,
				Err(error) => return ExecutionAttempt::ConsumedWithoutInvocation(error),
			};
		let descriptor = &descriptor;
		let CanonicalOperationPayload::Register { expected_head, target } = &descriptor.payload
		else {
			return ExecutionAttempt::ConsumedWithoutInvocation(
				ExecutionFailure::InvalidDescriptor,
			);
		};
		if target.repository_path != descriptor.repository_absolute_path
			|| target.worktree_path != descriptor.worktree_absolute_path
			|| target.repository_id != descriptor.repository_id
			|| target.worktree_id != descriptor.worktree_id
		{
			return ExecutionAttempt::ConsumedWithoutInvocation(
				ExecutionFailure::InvalidDescriptor,
			);
		}

		match path_is_missing(descriptor.worktree_absolute_path.as_path()) {
			Ok(true) => {},
			Ok(false) => {
				return ExecutionAttempt::ConsumedWithoutInvocation(
					ExecutionFailure::TargetOccupied,
				);
			},
			Err(error) => {
				return ExecutionAttempt::ConsumedWithoutInvocation(ExecutionFailure::from(error));
			},
		}
		let prepared = match self.prepare_operation(descriptor, admission.descriptor(), false) {
			Ok(prepared) => prepared,
			Err(error) => return ExecutionAttempt::ConsumedWithoutInvocation(error),
		};
		if prepared.head.as_str() != expected_head.as_str() {
			return ExecutionAttempt::ConsumedWithoutInvocation(
				ExecutionFailure::PreconditionMismatch,
			);
		}
		match inspect_registration_vacancy(
			&prepared.layout.common_dir,
			descriptor.worktree_absolute_path.as_path(),
		) {
			Ok(RegistrationVacancy::Vacant) => {},
			Ok(RegistrationVacancy::Replaced) => {
				return ExecutionAttempt::ConsumedWithoutInvocation(ExecutionFailure::Replaced);
			},
			Ok(RegistrationVacancy::Incomplete | RegistrationVacancy::Foreign) => {
				return ExecutionAttempt::ConsumedWithoutInvocation(
					ExecutionFailure::TargetOccupied,
				);
			},
			Err(error) => return ExecutionAttempt::ConsumedWithoutInvocation(error.into()),
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
		if let Err(error) = self.authorize_effect(admission.descriptor(), &prepared) {
			return ExecutionAttempt::ConsumedWithoutInvocation(error);
		}
		let result = self.run_git(&prepared.layout, None, &arguments, None, MAX_GIT_OUTPUT_BYTES);
		let attempt = self.finish_effect(prepared, admission.descriptor(), result);
		if attempt != ExecutionAttempt::CompletedInvocation {
			return attempt;
		}
		match self.inspect_registered_target(descriptor, admission.descriptor()) {
			Ok(target)
				if self
					.read_detached_head(&target.layout, descriptor.worktree_absolute_path.as_path())
					.is_ok_and(|head| head.as_str() == expected_head.as_str())
					&& registration_directory_is_clean(
						descriptor.worktree_absolute_path.as_path(),
					) && target.revalidate().is_ok() =>
				ExecutionAttempt::CompletedInvocation,
			Err(ReadbackFailure::Replaced) =>
				ExecutionAttempt::InvocationFailed(ExecutionFailure::Replaced),
			_ => ExecutionAttempt::InvocationFailed(ExecutionFailure::UnexpectedOutput),
		}
	}

	/// Consume the distinct Registered-to-Ready attempt without advancing HEAD.
	pub(crate) fn execute_worktree_ready(
		&mut self,
		receipt: RepositoryDispatchReceipt,
		admission: &RepositoryAdmissionFacts,
	) -> ExecutionAttempt {
		let descriptor = match self.consume_receipt(
			receipt,
			admission,
			RepositoryOperationKind::WorktreeReady,
		) {
			Ok(descriptor) => descriptor,
			Err(error) => return ExecutionAttempt::ConsumedWithoutInvocation(error),
		};
		let descriptor = &descriptor;
		let CanonicalOperationPayload::WorktreeReady { expected_head, policy } =
			&descriptor.payload
		else {
			return ExecutionAttempt::ConsumedWithoutInvocation(
				ExecutionFailure::InvalidDescriptor,
			);
		};
		if *policy != WorktreeReadyPolicy::ExactCleanWorktree {
			return ExecutionAttempt::ConsumedWithoutInvocation(
				ExecutionFailure::InvalidDescriptor,
			);
		}

		let prepared = match self.prepare_operation(descriptor, admission.descriptor(), true) {
			Ok(prepared) => prepared,
			Err(error) => return ExecutionAttempt::ConsumedWithoutInvocation(error),
		};
		if prepared.head.as_str() != expected_head.as_str() {
			return ExecutionAttempt::ConsumedWithoutInvocation(
				ExecutionFailure::PreconditionMismatch,
			);
		}
		let index = prepared.layout.private_index();
		if !path_is_missing(&index).unwrap_or(false)
			|| adjacent_lock_exists(&index).unwrap_or(true)
			|| !path_is_missing(&prepared.layout.private_index_owner()).unwrap_or(false)
		{
			return ExecutionAttempt::ConsumedWithoutInvocation(
				ExecutionFailure::PrivateIndexConflict,
			);
		}
		if create_private_index_owner(&prepared.layout, descriptor).is_err() {
			return ExecutionAttempt::InvocationFailed(ExecutionFailure::PrivateIndexConflict);
		}
		let arguments = vec![
			OsString::from("read-tree"),
			OsString::from("--reset"),
			OsString::from("-u"),
			OsString::from(expected_head.as_str()),
		];
		if let Err(error) = self.authorize_effect(admission.descriptor(), &prepared) {
			return ExecutionAttempt::InvocationFailed(error);
		}
		let result =
			self.run_git(&prepared.layout, Some(&index), &arguments, None, MAX_GIT_OUTPUT_BYTES);
		let attempt = self.finish_effect(prepared, admission.descriptor(), result);
		if attempt != ExecutionAttempt::CompletedInvocation {
			return attempt;
		}
		match self.inspect_registered_target(descriptor, admission.descriptor()) {
			Ok(target)
				if private_index_owner_matches(&target.layout, descriptor)
					&& FilePin::acquire(&index).is_ok()
					&& !adjacent_lock_exists(&index).unwrap_or(true)
					&& self
						.read_detached_head(
							&target.layout,
							descriptor.worktree_absolute_path.as_path(),
						)
						.is_ok_and(|head| head.as_str() == expected_head.as_str())
					&& self
						.clean_against(
							&target.layout,
							descriptor.worktree_absolute_path.as_path(),
							&index,
							expected_head,
						)
						.is_ok_and(|clean| clean)
					&& target.revalidate().is_ok() =>
				ExecutionAttempt::CompletedInvocation,
			Err(ReadbackFailure::Replaced) =>
				ExecutionAttempt::InvocationFailed(ExecutionFailure::Replaced),
			_ => ExecutionAttempt::InvocationFailed(ExecutionFailure::UnexpectedOutput),
		}
	}

	/// Consume the exact H-to-H-prime Commit attempt using only the service-private index.
	#[allow(clippy::too_many_lines)] // One closed fail-closed commit authority sequence.
	pub(crate) fn execute_commit(
		&mut self,
		receipt: RepositoryDispatchReceipt,
		admission: &RepositoryAdmissionFacts,
	) -> ExecutionAttempt {
		let descriptor =
			match self.consume_receipt(receipt, admission, RepositoryOperationKind::Commit) {
				Ok(descriptor) => descriptor,
				Err(error) => return ExecutionAttempt::ConsumedWithoutInvocation(error),
			};
		let descriptor = &descriptor;
		let CanonicalOperationPayload::Commit { expected_head, next_head, intent } =
			&descriptor.payload
		else {
			return ExecutionAttempt::ConsumedWithoutInvocation(
				ExecutionFailure::InvalidDescriptor,
			);
		};
		if intent.target_reference.as_str() != "HEAD" || !valid_commit_intent(intent) {
			return ExecutionAttempt::ConsumedWithoutInvocation(
				ExecutionFailure::InvalidDescriptor,
			);
		}

		let prepared = match self.prepare_operation(descriptor, admission.descriptor(), true) {
			Ok(prepared) => prepared,
			Err(error) => return ExecutionAttempt::ConsumedWithoutInvocation(error),
		};
		let index = prepared.layout.private_index();
		if !private_index_owner_matches(&prepared.layout, descriptor) {
			return ExecutionAttempt::ConsumedWithoutInvocation(
				ExecutionFailure::PrivateIndexConflict,
			);
		}
		let _index_pin = match FilePin::acquire(&index) {
			Ok(pin) => pin,
			Err(_) => {
				return ExecutionAttempt::ConsumedWithoutInvocation(
					ExecutionFailure::PrivateIndexConflict,
				);
			},
		};
		if adjacent_lock_exists(&index).unwrap_or(true) {
			return ExecutionAttempt::ConsumedWithoutInvocation(
				ExecutionFailure::PrivateIndexConflict,
			);
		}
		let reference = match self
			.read_detached_head(&prepared.layout, descriptor.worktree_absolute_path.as_path())
		{
			Ok(reference) => reference,
			Err(error) => return ExecutionAttempt::ConsumedWithoutInvocation(error),
		};
		if reference.as_str() != expected_head.as_str() {
			return ExecutionAttempt::ConsumedWithoutInvocation(
				ExecutionFailure::PreconditionMismatch,
			);
		}
		match self.object_exists(
			&prepared.layout,
			descriptor.worktree_absolute_path.as_path(),
			next_head,
		) {
			Ok(false) => {},
			_ => {
				return ExecutionAttempt::ConsumedWithoutInvocation(
					ExecutionFailure::PreconditionMismatch,
				);
			},
		}

		let add = vec![OsString::from("add"), OsString::from("--all"), OsString::from("--")];
		if let Err(error) = self.authorize_effect(admission.descriptor(), &prepared) {
			return ExecutionAttempt::ConsumedWithoutInvocation(error);
		}
		if let Err(error) =
			self.run_git(&prepared.layout, Some(&index), &add, None, MAX_GIT_OUTPUT_BYTES)
		{
			return self.finish_effect(prepared, admission.descriptor(), Err(error));
		}
		if !safe_private_index(&index) || adjacent_lock_exists(&index).unwrap_or(true) {
			return self.finish_effect(
				prepared,
				admission.descriptor(),
				Err(ExecutionFailure::PrivateIndexConflict),
			);
		}
		let write_tree = vec![OsString::from("write-tree")];
		if let Err(error) = self.authorize_effect(admission.descriptor(), &prepared) {
			return self.finish_effect(prepared, admission.descriptor(), Err(error));
		}
		let tree = match self.run_git(
			&prepared.layout,
			Some(&index),
			&write_tree,
			None,
			MAX_COMMIT_OUTPUT_BYTES,
		) {
			Ok(output) => match parse_single_revision(&output.stdout) {
				Some(tree) if tree == intent.tree.as_str() => tree.to_owned(),
				_ => {
					return self.finish_effect(
						prepared,
						admission.descriptor(),
						Err(ExecutionFailure::UnexpectedOutput),
					);
				},
			},
			Err(error) => return self.finish_effect(prepared, admission.descriptor(), Err(error)),
		};
		if !safe_private_index(&index) || adjacent_lock_exists(&index).unwrap_or(true) {
			return self.finish_effect(
				prepared,
				admission.descriptor(),
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
		if let Err(error) = self.authorize_effect(admission.descriptor(), &prepared) {
			return self.finish_effect(prepared, admission.descriptor(), Err(error));
		}
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
				_ => {
					return self.finish_effect(
						prepared,
						admission.descriptor(),
						Err(ExecutionFailure::UnexpectedOutput),
					);
				},
			},
			Err(error) => return self.finish_effect(prepared, admission.descriptor(), Err(error)),
		};
		if !safe_private_index(&index) || adjacent_lock_exists(&index).unwrap_or(true) {
			return self.finish_effect(
				prepared,
				admission.descriptor(),
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
		if let Err(error) = self.authorize_effect(admission.descriptor(), &prepared) {
			return self.finish_effect(prepared, admission.descriptor(), Err(error));
		}
		let result =
			self.run_git(&prepared.layout, Some(&index), &update_ref, None, MAX_GIT_OUTPUT_BYTES);
		let attempt = self.finish_effect(prepared, admission.descriptor(), result);
		// `git add` may publish the private index by lock-and-rename, so replacement by the
		// admitted Git operation is expected here. Reacquire and verify the resulting private file.
		if !safe_private_index(&index) || adjacent_lock_exists(&index).unwrap_or(true) {
			ExecutionAttempt::InvocationFailed(ExecutionFailure::PrivateIndexConflict)
		} else if attempt == ExecutionAttempt::CompletedInvocation {
			match self.inspect_registered_target(descriptor, admission.descriptor()) {
				Ok(target)
					if self
						.read_detached_head(
							&target.layout,
							descriptor.worktree_absolute_path.as_path(),
						)
						.is_ok_and(|head| head.as_str() == next_head.as_str()) =>
					attempt,
				Err(ReadbackFailure::Replaced) =>
					ExecutionAttempt::InvocationFailed(ExecutionFailure::Replaced),
				_ => ExecutionAttempt::InvocationFailed(ExecutionFailure::UnexpectedOutput),
			}
		} else {
			attempt
		}
	}

	/// Restart-safe registration readback. It has no call path to execution.
	pub(crate) fn read_registration(
		&self,
		request: &RegistrationReadbackRequest,
		admission: &RepositoryAdmissionFacts,
		evidence_id: RepositoryEvidenceId,
	) -> RegistrationEvidence {
		let descriptor = &request.descriptor;
		let CanonicalOperationPayload::Register { expected_head, target } = &descriptor.payload
		else {
			return RegistrationEvidence::Foreign;
		};
		if descriptor.kind != RepositoryOperationKind::Register
			|| target.repository_path != descriptor.repository_absolute_path
			|| target.worktree_path != descriptor.worktree_absolute_path
			|| !descriptor_matches_admission(descriptor, admission.descriptor())
		{
			return RegistrationEvidence::Foreign;
		}
		let source = match self.inspect_readback_source(descriptor, admission.descriptor()) {
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
			return match inspect_registration_vacancy(
				&source.layout.common_dir,
				descriptor.worktree_absolute_path.as_path(),
			) {
				Ok(RegistrationVacancy::Vacant) => RegistrationEvidence::NoEffect,
				Ok(RegistrationVacancy::Incomplete) => RegistrationEvidence::MissingReciprocal,
				Ok(RegistrationVacancy::Foreign) => RegistrationEvidence::Foreign,
				Ok(RegistrationVacancy::Replaced) => RegistrationEvidence::Replaced,
				Err(PathFailure::Replaced) => RegistrationEvidence::Replaced,
				Err(_) => RegistrationEvidence::Inconclusive,
			};
		}
		let target_layout =
			match RepositoryLayout::inspect(descriptor.worktree_absolute_path.as_path()) {
				Ok(layout) => layout,
				Err(PathFailure::Replaced) => return RegistrationEvidence::Replaced,
				Err(PathFailure::Missing) => return RegistrationEvidence::MissingReciprocal,
				Err(_) => return RegistrationEvidence::Inconclusive,
			};
		if target_layout.common_dir.identity() != source.layout.common_dir.identity()
			|| target_layout.backlink.as_deref()
				!= Some(descriptor.worktree_absolute_path.as_path().join(".git").as_path())
			|| !target_layout.has_exact_linked_admin_child()
			|| !expected_registration_admin(
				&source.layout.common_dir.path,
				descriptor.worktree_absolute_path.as_path(),
			)
			.is_ok_and(|expected| target_layout.git_dir.path == expected)
		{
			return RegistrationEvidence::MissingReciprocal;
		}
		if !safe_owned_regular_file(&target_layout.locked_marker()) {
			return RegistrationEvidence::MissingReciprocal;
		}
		if !registration_directory_is_clean(descriptor.worktree_absolute_path.as_path()) {
			return RegistrationEvidence::Dirty;
		}
		let head = match self
			.read_detached_head(&target_layout, descriptor.worktree_absolute_path.as_path())
		{
			Ok(head) => head,
			Err(ExecutionFailure::Replaced) => return RegistrationEvidence::Replaced,
			Err(_) => return RegistrationEvidence::Inconclusive,
		};
		if head.as_str() != expected_head.as_str() {
			return RegistrationEvidence::Stale;
		}
		if source.revalidate().is_err()
			|| target_layout.revalidate().is_err()
			|| self.reacquire_admission(admission.descriptor()).is_err()
		{
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
		admission: &RepositoryAdmissionFacts,
		evidence_id: RepositoryEvidenceId,
	) -> WorktreeReadyEvidence {
		let descriptor = &request.descriptor;
		let CanonicalOperationPayload::WorktreeReady { expected_head, policy } =
			&descriptor.payload
		else {
			return WorktreeReadyEvidence::Foreign;
		};
		if descriptor.kind != RepositoryOperationKind::WorktreeReady
			|| *policy != WorktreeReadyPolicy::ExactCleanWorktree
			|| !descriptor_matches_admission(descriptor, admission.descriptor())
		{
			return WorktreeReadyEvidence::Foreign;
		}
		let target = match self.inspect_registered_target(descriptor, admission.descriptor()) {
			Ok(target) => target,
			Err(ReadbackFailure::Missing) => return WorktreeReadyEvidence::NoEffect,
			Err(ReadbackFailure::Incomplete) => return WorktreeReadyEvidence::Incomplete,
			Err(ReadbackFailure::Replaced) => return WorktreeReadyEvidence::Replaced,
			Err(ReadbackFailure::Foreign) => return WorktreeReadyEvidence::Foreign,
			Err(ReadbackFailure::Dirty) => return WorktreeReadyEvidence::Dirty,
			Err(ReadbackFailure::Unavailable) => return WorktreeReadyEvidence::Unavailable,
		};
		let head = match self
			.read_detached_head(&target.layout, descriptor.worktree_absolute_path.as_path())
		{
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
		let owner = target.layout.private_index_owner();
		let owner_missing = path_is_missing(&owner).unwrap_or(false);
		let index_lock = adjacent_lock_exists(&index).unwrap_or(true);
		let index_pin = match FilePin::acquire(&index) {
			Ok(pin) => pin,
			Err(PathFailure::Missing) => {
				return if owner_missing
					&& !index_lock && registration_directory_is_clean(
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
		if owner_missing || index_lock || !private_index_owner_matches(&target.layout, descriptor) {
			return WorktreeReadyEvidence::Dirty;
		}
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
		if target.revalidate().is_err()
			|| index_pin.revalidate().is_err()
			|| self.reacquire_admission(admission.descriptor()).is_err()
		{
			return WorktreeReadyEvidence::Replaced;
		}

		WorktreeReadyEvidence::Exact(ExactWorktreeReadyEvidence {
			scope: readback_scope(descriptor, evidence_id),
			unchanged_head: expected_head.clone(),
		})
	}

	/// Restart-safe Commit readback. It observes one exact ref/commit/content advance only.
	#[allow(clippy::too_many_lines)] // One closed exact commit readback sequence.
	pub(crate) fn read_commit(
		&self,
		request: &CommitReadbackRequest,
		admission: &RepositoryAdmissionFacts,
		evidence_id: RepositoryEvidenceId,
	) -> CommitEvidence {
		let descriptor = &request.descriptor;
		let CanonicalOperationPayload::Commit { expected_head, next_head, intent } =
			&descriptor.payload
		else {
			return CommitEvidence::Foreign;
		};
		if descriptor.kind != RepositoryOperationKind::Commit
			|| intent.target_reference.as_str() != "HEAD"
			|| !valid_commit_intent(intent)
			|| !descriptor_matches_admission(descriptor, admission.descriptor())
		{
			return CommitEvidence::Foreign;
		}
		let target = match self.inspect_registered_target(descriptor, admission.descriptor()) {
			Ok(target) => target,
			Err(ReadbackFailure::Missing) => return CommitEvidence::Incomplete,
			Err(ReadbackFailure::Incomplete) => return CommitEvidence::Incomplete,
			Err(ReadbackFailure::Replaced) => return CommitEvidence::Replaced,
			Err(ReadbackFailure::Foreign) => return CommitEvidence::Foreign,
			Err(ReadbackFailure::Dirty) => return CommitEvidence::Dirty,
			Err(ReadbackFailure::Unavailable) => return CommitEvidence::Unavailable,
		};
		let index = target.layout.private_index();
		let owner = target.layout.private_index_owner();
		let index_pin = match FilePin::acquire(&index) {
			Ok(pin) => pin,
			Err(PathFailure::Replaced) => return CommitEvidence::Replaced,
			Err(PathFailure::UnsafeOwner) => return CommitEvidence::Dirty,
			Err(_) => return CommitEvidence::Incomplete,
		};
		if path_is_missing(&owner).unwrap_or(false)
			|| !private_index_owner_matches(&target.layout, descriptor)
		{
			return CommitEvidence::Dirty;
		}
		let locks_present = adjacent_lock_exists(&index).unwrap_or(true)
			|| !path_is_missing(&target.layout.git_dir.path.join("HEAD.lock")).unwrap_or(false)
			|| object_temporary_evidence(&target.layout.objects_dir.path).unwrap_or(true);
		let observed = match self
			.read_detached_head(&target.layout, descriptor.worktree_absolute_path.as_path())
		{
			Ok(observed) => observed,
			Err(ExecutionFailure::Replaced) => return CommitEvidence::Replaced,
			Err(error) => return commit_readback_process_failure(error),
		};
		let next_object_exists = match self.object_exists(
			&target.layout,
			descriptor.worktree_absolute_path.as_path(),
			next_head,
		) {
			Ok(exists) => exists,
			Err(ExecutionFailure::Replaced) => return CommitEvidence::Replaced,
			Err(error) => return commit_readback_process_failure(error),
		};
		let exact_next_object = if next_object_exists {
			match self.read_object(
				&target.layout,
				descriptor.worktree_absolute_path.as_path(),
				next_head,
			) {
				Ok(bytes) if bytes == expected_commit_bytes(intent, expected_head) => true,
				Ok(_) => return CommitEvidence::Foreign,
				Err(ExecutionFailure::Replaced) => return CommitEvidence::Replaced,
				Err(_) => return CommitEvidence::Incomplete,
			}
		} else {
			false
		};
		if observed.as_str() == expected_head.as_str() {
			if locks_present || exact_next_object {
				return CommitEvidence::Incomplete;
			}
			return match self.clean_against(
				&target.layout,
				descriptor.worktree_absolute_path.as_path(),
				&index,
				expected_head,
			) {
				Ok(true)
					if target.revalidate().is_ok()
						&& index_pin.revalidate().is_ok()
						&& self.reacquire_admission(admission.descriptor()).is_ok() =>
					CommitEvidence::NoEffect,
				Ok(true) | Err(ExecutionFailure::Replaced) => CommitEvidence::Replaced,
				Ok(false) => CommitEvidence::Dirty,
				Err(error) => commit_readback_process_failure(error),
			};
		}
		if observed.as_str() == descriptor.admitted_base.as_str()
			&& expected_head.as_str() != descriptor.admitted_base.as_str()
		{
			return CommitEvidence::Rollback;
		}
		if observed.as_str() != next_head.as_str() {
			return CommitEvidence::Foreign;
		}
		if locks_present || !exact_next_object {
			return CommitEvidence::Incomplete;
		}
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
		if target.revalidate().is_err()
			|| index_pin.revalidate().is_err()
			|| self.reacquire_admission(admission.descriptor()).is_err()
		{
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

	fn consume_receipt(
		&mut self,
		receipt: RepositoryDispatchReceipt,
		admission: &RepositoryAdmissionFacts,
		expected_kind: RepositoryOperationKind,
	) -> Result<CanonicalOperationDescriptor, ExecutionFailure> {
		let descriptor = receipt.into_descriptor();
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
		if !descriptor_matches_admission(&descriptor, admission.descriptor()) {
			return Err(ExecutionFailure::ForeignRepository);
		}
		self.reacquire_admission(admission.descriptor())?;
		Ok(descriptor)
	}

	fn prepare_operation(
		&self,
		descriptor: &CanonicalOperationDescriptor,
		admission: &RepositoryAdmissionDescriptor,
		require_registered_target: bool,
	) -> Result<PreparedOperation, ExecutionFailure> {
		let repository_path = descriptor.repository_absolute_path.as_path();
		let repository_pin = DirectoryPin::acquire(repository_path)?;
		validate_repository_owner(&repository_pin)?;
		let source_layout = self.reacquire_admission(admission)?;
		self.verify_repository_policy(&source_layout, repository_path)?;

		let (path_pin, layout, worktree_path) = if require_registered_target {
			let target = self
				.inspect_registered_target(descriptor, admission)
				.map_err(map_readback_execution)?;
			(target.path_pin, target.layout, descriptor.worktree_absolute_path.as_path().to_owned())
		} else {
			let parent = vacant_target(descriptor.worktree_absolute_path.as_path())?;
			validate_repository_owner(&parent)?;
			(parent, source_layout, repository_path.to_owned())
		};
		let head = if require_registered_target {
			self.read_detached_head(&layout, &worktree_path)?
		} else {
			self.read_revision(&layout, &worktree_path, OsStr::new("HEAD"))?
		};
		self.git.revalidate()?;

		Ok(PreparedOperation { repository_pin, path_pin, layout, head })
	}

	fn finish_effect(
		&self,
		prepared: PreparedOperation,
		admission: &RepositoryAdmissionDescriptor,
		result: Result<GitOutput, ExecutionFailure>,
	) -> ExecutionAttempt {
		let identity_result = prepared
			.repository_pin
			.revalidate()
			.and_then(|()| prepared.path_pin.revalidate())
			.and_then(|()| prepared.layout.revalidate())
			.map_err(ExecutionFailure::from)
			.and_then(|()| self.git.revalidate())
			.and_then(|()| self.reacquire_admission(admission).map(|_| ()));
		if let Err(error) = identity_result {
			return ExecutionAttempt::InvocationFailed(error);
		}
		match result {
			Ok(_) => ExecutionAttempt::CompletedInvocation,
			Err(error) => ExecutionAttempt::InvocationFailed(error),
		}
	}

	fn authorize_effect(
		&self,
		admission: &RepositoryAdmissionDescriptor,
		prepared: &PreparedOperation,
	) -> Result<(), ExecutionFailure> {
		let source = self.reacquire_admission(admission)?;
		self.verify_repository_policy(&source, admission.repository_path().as_path())?;
		prepared.repository_pin.revalidate()?;
		prepared.path_pin.revalidate()?;
		prepared.layout.revalidate()?;
		self.git.revalidate()
	}

	fn inspect_readback_source(
		&self,
		descriptor: &CanonicalOperationDescriptor,
		admission: &RepositoryAdmissionDescriptor,
	) -> Result<InspectedTarget, ReadbackFailure> {
		let path = descriptor.repository_absolute_path.as_path();
		let path_pin = DirectoryPin::acquire(path).map_err(ReadbackFailure::from)?;
		validate_repository_owner(&path_pin).map_err(ReadbackFailure::from)?;
		if !descriptor_matches_admission(descriptor, admission) {
			return Err(ReadbackFailure::Foreign);
		}
		let layout = self.reacquire_admission(admission).map_err(ReadbackFailure::from)?;
		self.verify_repository_policy(&layout, path).map_err(ReadbackFailure::from)?;
		Ok(InspectedTarget { path_pin, layout })
	}

	fn inspect_registered_target(
		&self,
		descriptor: &CanonicalOperationDescriptor,
		admission: &RepositoryAdmissionDescriptor,
	) -> Result<InspectedTarget, ReadbackFailure> {
		let source = self.inspect_readback_source(descriptor, admission)?;
		let path = descriptor.worktree_absolute_path.as_path();
		let path_pin = DirectoryPin::acquire(path).map_err(ReadbackFailure::from)?;
		validate_repository_owner(&path_pin).map_err(ReadbackFailure::from)?;
		let layout = RepositoryLayout::inspect(path).map_err(ReadbackFailure::from)?;
		if layout.common_dir.identity() != source.layout.common_dir.identity()
			|| layout.backlink.as_deref() != Some(path.join(".git").as_path())
			|| !layout.has_exact_linked_admin_child()
			|| !expected_registration_admin(&source.layout.common_dir.path, path)
				.is_ok_and(|expected| layout.git_dir.path == expected)
		{
			return Err(ReadbackFailure::Foreign);
		}
		if !safe_owned_regular_file(&layout.locked_marker()) {
			return Err(ReadbackFailure::Incomplete);
		}
		self.verify_repository_policy(&layout, path).map_err(ReadbackFailure::from)?;
		Ok(InspectedTarget { path_pin, layout })
	}

	/// Reconstruct every observable field through the validated core constructor and require exact
	/// equality with the complete persisted descriptor. Opaque admission identity is copied only as
	/// constructor input; it cannot mask any changed path, role, layout, stat fact, or digest.
	fn reacquire_admission(
		&self,
		expected: &RepositoryAdmissionDescriptor,
	) -> Result<RepositoryLayout, ExecutionFailure> {
		if expected.version() != RepositoryAdmissionDescriptorVersion::V1 {
			return Err(ExecutionFailure::UnsupportedContract);
		}
		let layout = RepositoryLayout::inspect(expected.repository_path().as_path())?;
		let (registration_role, registration_id, common_file, backlink_file) =
			if layout.backlink.is_some() {
				if !layout.has_exact_linked_admin_child() {
					return Err(ExecutionFailure::UnsupportedRepository);
				}
				let name = layout
					.git_dir
					.path
					.file_name()
					.and_then(OsStr::to_str)
					.ok_or(ExecutionFailure::UnsupportedRepository)?;
				let registration_id = RepositoryRegistrationId::new(name.to_owned())
					.map_err(|_| ExecutionFailure::UnsupportedRepository)?;
				(
					RepositoryGitRegistrationRole::LinkedWorktree,
					Some(registration_id),
					Some(persisted_path(layout.git_dir.path.join("commondir"))?),
					Some(persisted_path(layout.git_dir.path.join("gitdir"))?),
				)
			} else {
				(RepositoryGitRegistrationRole::PrimaryWorktree, None, None, None)
			};
		let observed_layout = RepositoryAdmittedGitLayout::new(
			registration_role,
			registration_id,
			persisted_path(layout.worktree_path.clone())?,
			persisted_path(layout.worktree_path.join(".git"))?,
			persisted_path(layout.git_dir.path.clone())?,
			persisted_path(layout.common_dir.path.clone())?,
			persisted_path(layout.objects_dir.path.clone())?,
			layout.refs_dir.as_ref().map(|refs| persisted_path(refs.path.clone())).transpose()?,
			common_file,
			backlink_file,
		);
		let observations =
			expected.observations().iter().map(observe_path).collect::<Result<Vec<_>, _>>()?;
		let observed = RepositoryAdmissionDescriptor::new_v1(
			expected.project_id().clone(),
			expected.repository_id().clone(),
			expected.admitted_identity().clone(),
			expected.admitted_base().clone(),
			persisted_path(layout.worktree_path.clone())?,
			observed_layout,
			observations,
		)
		.map_err(|_| ExecutionFailure::UnsupportedRepository)?;
		if &observed != expected || !observed.verify_digest(expected.digest()) {
			return Err(ExecutionFailure::Replaced);
		}
		layout.revalidate()?;
		Ok(layout)
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
		if !path_is_missing(&worktree_config).unwrap_or(false) {
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
		verify_worktree_inventory(worktree_path)?;

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
		verify_tree_inventory(&output.stdout)?;
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
		let revision =
			parse_single_revision(&output.stdout).ok_or(ExecutionFailure::UnexpectedOutput)?;
		RepositoryContentRevision::new(revision.to_owned())
			.map_err(|_| ExecutionFailure::UnexpectedOutput)
	}

	fn read_detached_head(
		&self,
		layout: &RepositoryLayout,
		worktree_path: &Path,
	) -> Result<RepositoryContentRevision, ExecutionFailure> {
		let bytes = read_nofollow(&layout.git_dir.path.join("HEAD"), 256)?;
		let direct =
			parse_single_revision(&bytes).ok_or(ExecutionFailure::UnsupportedRepository)?;
		let observed = self.read_revision(layout, worktree_path, OsStr::new("HEAD"))?;
		if observed.as_str() != direct {
			return Err(ExecutionFailure::Replaced);
		}
		Ok(observed)
	}

	fn object_exists(
		&self,
		layout: &RepositoryLayout,
		worktree_path: &Path,
		revision: &RepositoryContentRevision,
	) -> Result<bool, ExecutionFailure> {
		let arguments = vec![
			OsString::from("cat-file"),
			OsString::from("-e"),
			OsString::from(format!("{}^{{commit}}", revision.as_str())),
		];
		match self.run_git_raw(layout, worktree_path, None, &arguments, None, 1, &[]) {
			Ok(output) if output.status.success() => Ok(true),
			Ok(output) if output.status.code() == Some(1) => Ok(false),
			Ok(output) => Err(classify_status(output.status)),
			Err(error) => Err(error),
		}
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
			.run_git_at(layout, worktree_path, None, &arguments, None, MAX_COMMIT_OUTPUT_BYTES)?
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
		self.run_git_at(
			layout,
			layout.worktree_path.as_path(),
			index,
			arguments,
			stdin,
			output_limit,
		)
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
		let output =
			self.run_git_raw(layout, worktree_path, index, arguments, stdin, output_limit, &[])?;
		if output.status.success() { Ok(output) } else { Err(classify_status(output.status)) }
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
		if output.status.success() { Ok(output) } else { Err(classify_status(output.status)) }
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
			.env("GIT_NO_REPLACE_OBJECTS", "1")
			.env("GIT_LITERAL_PATHSPECS", "1")
			.env("GIT_DISCOVERY_ACROSS_FILESYSTEM", "0")
			.env("GIT_CEILING_DIRECTORIES", "/")
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
			ExecutionFailure::ForeignRepository | ExecutionFailure::UnsupportedRepository =>
				Self::Foreign,
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
		ReadbackFailure::Incomplete | ReadbackFailure::Unavailable =>
			ExecutionFailure::UnsupportedRepository,
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

fn descriptor_matches_admission(
	descriptor: &CanonicalOperationDescriptor,
	admission: &RepositoryAdmissionDescriptor,
) -> bool {
	&descriptor.project_id == admission.project_id()
		&& &descriptor.repository_id == admission.repository_id()
		&& &descriptor.admitted_identity == admission.admitted_identity()
		&& &descriptor.admitted_base == admission.admitted_base()
		&& &descriptor.admission_descriptor_digest == admission.digest()
		&& &descriptor.repository_absolute_path == admission.repository_path()
		&& admission.verify_digest(&descriptor.admission_descriptor_digest)
}

fn persisted_path(path: PathBuf) -> Result<PersistedAbsolutePath, ExecutionFailure> {
	PersistedAbsolutePath::new(path).map_err(|_| ExecutionFailure::UnsupportedRepository)
}

fn observe_path(
	expected: &RepositoryPathObservation,
) -> Result<RepositoryPathObservation, ExecutionFailure> {
	let path = expected.path().as_path();
	let (metadata, object_type) = match expected.object_type() {
		RepositoryObservedObjectType::Directory => {
			let pin = if path == Path::new("/") {
				let file = open_directory_absolute(path)?;
				file.metadata().map_err(|_| ExecutionFailure::PathUnavailable)?
			} else {
				DirectoryPin::acquire(path)?
					.components
					.last()
					.expect("directory pin contains root")
					.0
					.metadata()
					.map_err(|_| ExecutionFailure::PathUnavailable)?
			};
			(pin, RepositoryObservedObjectType::Directory)
		},
		RepositoryObservedObjectType::RegularFile => {
			let file = open_nofollow_file(path)?;
			let metadata = file.metadata().map_err(|_| ExecutionFailure::PathUnavailable)?;
			if !metadata.is_file() {
				return Err(ExecutionFailure::Replaced);
			}
			(metadata, RepositoryObservedObjectType::RegularFile)
		},
	};
	RepositoryPathObservation::new(
		RepositoryObservationPath::new(path.to_owned())
			.map_err(|_| ExecutionFailure::UnsupportedRepository)?,
		expected.roles().to_vec(),
		metadata.dev(),
		metadata.ino(),
		object_type,
		metadata.uid(),
		metadata.mode() & 0o7777,
	)
	.map_err(|_| ExecutionFailure::Replaced)
}

fn fixed_config() -> &'static [(&'static str, &'static str)] {
	&[
		("advice.detachedHead", "false"),
		("commit.gpgSign", "false"),
		("core.editor", DISABLED_EXECUTABLE),
		("core.attributesFile", "/dev/null"),
		("core.autocrlf", "false"),
		("core.fsmonitor", "false"),
		("core.hooksPath", "/var/empty/decodex-no-hooks"),
		("core.pager", "false"),
		("core.protectHFS", "true"),
		("core.protectNTFS", "true"),
		("core.safecrlf", "true"),
		("core.sparseCheckout", "false"),
		("core.sparseCheckoutCone", "false"),
		("core.untrackedCache", "false"),
		("credential.helper", ""),
		("credential.interactive", "never"),
		("diff.external", ""),
		("gc.auto", "0"),
		("maintenance.auto", "false"),
		("merge.renormalize", "false"),
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
		"core.repositoryformatversion" => value == "0",
		"core.bare" => value == "false",
		"core.logallrefupdates" => matches!(value, "true" | "false" | "always"),
		"core.protecthfs" | "core.protectntfs" => value == "true",
		"extensions.objectformat" => matches!(value, "sha1" | "sha256"),
		_ => matches!(value, "true" | "false"),
	}
}

fn verify_hooks(path: &Path) -> Result<(), ExecutionFailure> {
	match fs::read_dir(path) {
		Ok(entries) => {
			let started = Instant::now();
			for (count, entry) in entries.enumerate() {
				if count >= MAX_WALK_FILES || started.elapsed() > WALK_TIMEOUT {
					return Err(ExecutionFailure::OutputLimit);
				}
				let entry = entry.map_err(|_| ExecutionFailure::UnsupportedRepository)?;
				let name = entry.file_name();
				let name = name.to_str().ok_or(ExecutionFailure::UnsupportedRepository)?;
				let metadata = fs::symlink_metadata(entry.path())
					.map_err(|_| ExecutionFailure::UnsupportedRepository)?;
				if !name.ends_with(".sample")
					|| !metadata.is_file()
					|| metadata.file_type().is_symlink()
				{
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

fn verify_tree_inventory(bytes: &[u8]) -> Result<(), ExecutionFailure> {
	let mut files = 0_usize;
	for record in bytes.split(|byte| *byte == 0).filter(|record| !record.is_empty()) {
		files = files.checked_add(1).ok_or(ExecutionFailure::OutputLimit)?;
		if files > MAX_WALK_FILES {
			return Err(ExecutionFailure::OutputLimit);
		}
		let tab = record
			.iter()
			.position(|byte| *byte == b'\t')
			.ok_or(ExecutionFailure::UnexpectedOutput)?;
		let header =
			std::str::from_utf8(&record[..tab]).map_err(|_| ExecutionFailure::UnexpectedOutput)?;
		let mut fields = header.split(' ');
		let mode = fields.next().ok_or(ExecutionFailure::UnexpectedOutput)?;
		let kind = fields.next().ok_or(ExecutionFailure::UnexpectedOutput)?;
		let _object = fields.next().ok_or(ExecutionFailure::UnexpectedOutput)?;
		if fields.next().is_some() || matches!(mode, "120000" | "160000") || kind == "commit" {
			return Err(ExecutionFailure::UnsupportedRepository);
		}
		let path = &record[tab + 1..];
		if unsafe_git_tree_path(path) {
			return Err(ExecutionFailure::UnsupportedRepository);
		}
		if path == b".gitmodules" || path.ends_with(b"/.gitmodules") {
			return Err(ExecutionFailure::UnsupportedRepository);
		}
		if path == b".gitattributes" || path.ends_with(b"/.gitattributes") {
			return Err(ExecutionFailure::UnsupportedRepository);
		}
	}
	Ok(())
}

fn unsafe_git_tree_path(path: &[u8]) -> bool {
	path.is_empty()
		|| !path.is_ascii()
		|| path.split(|byte| *byte == b'/').any(|component| {
			if component.is_empty()
				|| component == b"."
				|| component == b".."
				|| component.contains(&b'\\')
				|| component.contains(&b':')
				|| component.last().is_some_and(|byte| matches!(byte, b'.' | b' '))
			{
				return true;
			}
			let folded = component.iter().map(u8::to_ascii_lowercase).collect::<Vec<_>>();
			folded == b".git" || folded.starts_with(b".git~") || folded.starts_with(b"git~")
		})
}

fn verify_worktree_inventory(root: &Path) -> Result<(), ExecutionFailure> {
	let started = Instant::now();
	let mut pending = vec![(root.to_owned(), 0_usize)];
	let mut files = 0_usize;
	let mut bytes = 0_u64;
	while let Some((directory, depth)) = pending.pop() {
		if started.elapsed() > WALK_TIMEOUT || depth > MAX_WALK_DEPTH {
			return Err(ExecutionFailure::TimedOut);
		}
		let entries = fs::read_dir(&directory).map_err(|_| ExecutionFailure::PathUnavailable)?;
		for entry in entries {
			let entry = entry.map_err(|_| ExecutionFailure::PathUnavailable)?;
			files = files.checked_add(1).ok_or(ExecutionFailure::OutputLimit)?;
			if files > MAX_WALK_FILES || started.elapsed() > WALK_TIMEOUT {
				return Err(ExecutionFailure::OutputLimit);
			}
			let name = entry.file_name();
			if depth == 0 && name == OsStr::new(".git") {
				continue;
			}
			if name == OsStr::new(".gitattributes") || name == OsStr::new(".gitmodules") {
				return Err(ExecutionFailure::UnsupportedRepository);
			}
			let metadata = fs::symlink_metadata(entry.path())
				.map_err(|_| ExecutionFailure::PathUnavailable)?;
			if metadata.file_type().is_symlink() || (!metadata.is_dir() && !metadata.is_file()) {
				return Err(ExecutionFailure::UnsupportedRepository);
			}
			bytes = bytes.checked_add(metadata.len()).ok_or(ExecutionFailure::OutputLimit)?;
			if bytes > MAX_WALK_BYTES {
				return Err(ExecutionFailure::OutputLimit);
			}
			if metadata.is_dir() {
				pending.push((entry.path(), depth + 1));
			}
		}
	}
	Ok(())
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
		(OsString::from("GIT_COMMITTER_NAME"), OsString::from(intent.committer.name.as_str())),
		(OsString::from("GIT_COMMITTER_EMAIL"), OsString::from(intent.committer.email.as_str())),
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
	let mut stdout = match child.stdout.take() {
		Some(stdout) => stdout,
		None => {
			terminate_process_group(&mut child);
			let _ = child.wait();
			return Err(ExecutionFailure::SpawnFailed);
		},
	};
	let mut stderr = match child.stderr.take() {
		Some(stderr) => stderr,
		None => {
			terminate_process_group(&mut child);
			let _ = child.wait();
			return Err(ExecutionFailure::SpawnFailed);
		},
	};
	let mut stdin_pipe = child.stdin.take();
	if stdin.is_none() {
		stdin_pipe = None;
	}
	for descriptor in [
		stdout.as_raw_fd(),
		stderr.as_raw_fd(),
		stdin_pipe.as_ref().map_or(-1, |pipe| pipe.as_raw_fd()),
	] {
		if descriptor != -1 && set_nonblocking(descriptor).is_err() {
			return abort_bounded(&mut child, &mut stdin_pipe, ExecutionFailure::SpawnFailed);
		}
	}
	let input = stdin.unwrap_or_default();
	let mut input_offset = 0_usize;
	let mut stdout_bytes = Vec::with_capacity(limit.min(64 * 1_024));
	let mut stderr_bytes = Vec::with_capacity(limit.min(64 * 1_024));
	let mut stdout_eof = false;
	let mut stderr_eof = false;
	let deadline = Instant::now() + timeout;
	loop {
		if Instant::now() >= deadline {
			return abort_bounded(&mut child, &mut stdin_pipe, ExecutionFailure::TimedOut);
		}
		if !stdout_eof {
			stdout_eof = match drain_nonblocking(&mut stdout, &mut stdout_bytes, limit, deadline) {
				Ok(eof) => eof,
				Err(error) => return abort_bounded(&mut child, &mut stdin_pipe, error),
			};
		}
		if !stderr_eof {
			stderr_eof = match drain_nonblocking(&mut stderr, &mut stderr_bytes, limit, deadline) {
				Ok(eof) => eof,
				Err(error) => return abort_bounded(&mut child, &mut stdin_pipe, error),
			};
		}
		if let Some(writer) = &mut stdin_pipe {
			match writer.write(&input[input_offset..]) {
				Ok(0) if input_offset != input.len() => {
					return abort_bounded(
						&mut child,
						&mut stdin_pipe,
						ExecutionFailure::StdinFailed,
					);
				},
				Ok(count) => input_offset += count,
				Err(error) if error.kind() == ErrorKind::WouldBlock => {},
				Err(error) if error.kind() == ErrorKind::Interrupted => {},
				Err(_) => {
					return abort_bounded(
						&mut child,
						&mut stdin_pipe,
						ExecutionFailure::StdinFailed,
					);
				},
			}
			if input_offset == input.len() {
				stdin_pipe = None;
			}
		}
		if stdout_eof && stderr_eof && stdin_pipe.is_none() {
			match child.try_wait() {
				Ok(Some(status)) => {
					return Ok(GitOutput { status, stdout: stdout_bytes, stderr: stderr_bytes });
				},
				Ok(None) => {},
				Err(_) => {
					let _ = child.wait();
					return Err(ExecutionFailure::SpawnFailed);
				},
			}
		}
		thread::sleep(POLL_INTERVAL);
	}
}

fn set_nonblocking(descriptor: i32) -> Result<(), ExecutionFailure> {
	// SAFETY: `descriptor` is a live uniquely owned child pipe; `fcntl` changes only its flags.
	unsafe {
		let flags = libc::fcntl(descriptor, libc::F_GETFL);
		if flags == -1 || libc::fcntl(descriptor, libc::F_SETFL, flags | libc::O_NONBLOCK) == -1 {
			return Err(ExecutionFailure::SpawnFailed);
		}
	}
	Ok(())
}

fn drain_nonblocking(
	reader: &mut impl Read,
	output: &mut Vec<u8>,
	limit: usize,
	deadline: Instant,
) -> Result<bool, ExecutionFailure> {
	let mut buffer = [0_u8; 8 * 1_024];
	loop {
		if Instant::now() >= deadline {
			return Err(ExecutionFailure::TimedOut);
		}
		match reader.read(&mut buffer) {
			Ok(0) => return Ok(true),
			Ok(count) => {
				if output.len().saturating_add(count) > limit {
					return Err(ExecutionFailure::OutputLimit);
				}
				output.extend_from_slice(&buffer[..count]);
			},
			Err(error) if error.kind() == ErrorKind::WouldBlock => return Ok(false),
			Err(error) if error.kind() == ErrorKind::Interrupted => {},
			Err(_) => return Err(ExecutionFailure::GitUnavailable),
		}
	}
}

fn abort_bounded(
	child: &mut std::process::Child,
	stdin_pipe: &mut Option<std::process::ChildStdin>,
	error: ExecutionFailure,
) -> Result<GitOutput, ExecutionFailure> {
	// Failure paths have not waited or reaped the leader, so its PID still reserves this process
	// group identity and the negative-PID signal cannot target a reused unrelated group.
	terminate_process_group(child);
	*stdin_pipe = None;
	let _ = child.wait();
	Err(error)
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
				(libc::RLIMIT_CORE, 0, 0),
				(libc::RLIMIT_CPU, 30, 30),
				(libc::RLIMIT_FSIZE, 64 * 1_024 * 1_024, 64 * 1_024 * 1_024),
				(libc::RLIMIT_NOFILE, 64, 64),
				(libc::RLIMIT_AS, 1024 * 1_024 * 1_024, 1024 * 1_024 * 1_024),
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
			|| metadata.len() > MAX_GIT_EXECUTABLE_BYTES
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
	let mut total = 0_u64;
	loop {
		let count = source.read(&mut buffer).map_err(|_| ExecutionFailure::GitUnavailable)?;
		if count == 0 {
			break;
		}
		total = total.checked_add(count as u64).ok_or(ExecutionFailure::GitUnavailable)?;
		if total > MAX_GIT_EXECUTABLE_BYTES {
			return Err(ExecutionFailure::GitUnavailable);
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
	let file =
		open_nofollow_file(Path::new(DISABLED_EXECUTABLE)).map_err(ExecutionFailure::from)?;
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
		Ok(Self { path: path.to_owned(), identity: ObjectIdentity::from_metadata(&metadata), file })
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
		let started = Instant::now();
		let root = open_directory_absolute(Path::new("/"))?;
		let root_identity =
			ObjectIdentity::from_metadata(&root.metadata().map_err(|_| PathFailure::Io)?);
		let mut components = vec![(root, root_identity)];
		for part in parts {
			if started.elapsed() > WALK_TIMEOUT {
				return Err(PathFailure::Io);
			}
			let parent = &components.last().expect("root descriptor exists").0;
			let child = openat_directory(parent, part)?;
			let identity =
				ObjectIdentity::from_metadata(&child.metadata().map_err(|_| PathFailure::Io)?);
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

	fn private_index_owner(&self) -> PathBuf {
		self.git_dir.path.join(PRIVATE_INDEX_OWNER_NAME)
	}

	fn locked_marker(&self) -> PathBuf {
		self.git_dir.path.join("locked")
	}

	fn has_exact_linked_admin_child(&self) -> bool {
		if self.backlink.is_none() {
			return false;
		}
		let Some(name) = self.git_dir.path.file_name() else {
			return false;
		};
		let expected_root = self.common_dir.path.join("worktrees");
		self.git_dir.path == expected_root.join(name)
			&& self.git_dir.path.parent() == Some(expected_root.as_path())
			&& DirectoryPin::acquire(&expected_root).is_ok_and(|parent| {
				parent.revalidate().is_ok()
					&& self
						.git_dir
						.components
						.get(self.git_dir.components.len().saturating_sub(2))
						.is_some_and(|component| component.1 == parent.identity())
			})
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
		ExecutionFailure::ForeignRepository | ExecutionFailure::PreconditionMismatch =>
			AcquisitionFailure::ForeignRepository,
		ExecutionFailure::UnsupportedRepository => AcquisitionFailure::UnsupportedRepository,
		ExecutionFailure::GitUnavailable | ExecutionFailure::SpawnFailed =>
			AcquisitionFailure::GitUnavailable,
		ExecutionFailure::Exited(_) | ExecutionFailure::Signaled(_) =>
			AcquisitionFailure::GitFailed,
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
	if parts.is_empty() || parts.len() > MAX_PATH_COMPONENTS {
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
	Read::by_ref(&mut file)
		.take(limit.saturating_add(1) as u64)
		.read_to_end(&mut bytes)
		.map_err(|_| PathFailure::Io)?;
	if bytes.len() > limit {
		return Err(PathFailure::Invalid);
	}
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
			Component::ParentDir =>
				if !normalized.pop() {
					return Err(PathFailure::Invalid);
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

fn private_index_owner_bytes(descriptor: &CanonicalOperationDescriptor) -> Vec<u8> {
	format!(
		"decodex-private-index-v1\n{}\n{}\n{}\n{}\n",
		descriptor.repository_id.as_str(),
		descriptor.allocation_id.as_str(),
		descriptor.worktree_id.as_str(),
		descriptor.worktree_absolute_path.as_path().display(),
	)
	.into_bytes()
}

fn create_private_index_owner(
	layout: &RepositoryLayout,
	descriptor: &CanonicalOperationDescriptor,
) -> Result<FilePin, PathFailure> {
	layout.revalidate()?;
	let path = layout.private_index_owner();
	let mut owner =
		OpenOptions::new().write(true).create_new(true).mode(0o600).open(&path).map_err(path_io)?;
	owner.write_all(&private_index_owner_bytes(descriptor)).map_err(|_| PathFailure::Io)?;
	owner.sync_all().map_err(|_| PathFailure::Io)?;
	layout.revalidate()?;
	FilePin::acquire(&path)
}

fn private_index_owner_matches(
	layout: &RepositoryLayout,
	descriptor: &CanonicalOperationDescriptor,
) -> bool {
	read_nofollow(&layout.private_index_owner(), MAX_METADATA_FILE_BYTES)
		.is_ok_and(|bytes| bytes == private_index_owner_bytes(descriptor))
}

fn object_temporary_evidence(objects: &Path) -> Result<bool, PathFailure> {
	let started = Instant::now();
	let mut pending = vec![(objects.to_owned(), 0_usize)];
	let mut count = 0_usize;
	while let Some((directory, depth)) = pending.pop() {
		if started.elapsed() > WALK_TIMEOUT {
			return Err(PathFailure::Io);
		}
		for entry in fs::read_dir(directory).map_err(path_io)? {
			let entry = entry.map_err(|_| PathFailure::Io)?;
			count = count.checked_add(1).ok_or(PathFailure::Io)?;
			if count > MAX_WALK_FILES {
				return Err(PathFailure::Io);
			}
			let metadata = fs::symlink_metadata(entry.path()).map_err(path_io)?;
			if metadata.file_type().is_symlink() {
				return Err(PathFailure::Invalid);
			}
			let name = entry.file_name();
			let bytes = name.as_bytes();
			if bytes.starts_with(b"tmp_obj_")
				|| bytes.starts_with(b"incoming-")
				|| bytes.ends_with(b".lock")
			{
				return Ok(true);
			}
			if metadata.is_dir() && depth < 2 {
				pending.push((entry.path(), depth + 1));
			}
		}
	}
	Ok(false)
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
	let started = Instant::now();
	for (count, entry) in entries.enumerate() {
		if count >= 2 || started.elapsed() > WALK_TIMEOUT {
			return false;
		}
		let Ok(entry) = entry else {
			return false;
		};
		names.push(entry.file_name());
	}
	names == [OsString::from(".git")]
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RegistrationVacancy {
	Vacant,
	Incomplete,
	Foreign,
	Replaced,
}

fn require_allocation_registration_vacancy(
	common_dir: &DirectoryPin,
	worktree_path: &Path,
) -> Result<(), AcquisitionFailure> {
	match inspect_registration_vacancy(common_dir, worktree_path).map_err(map_acquisition_path)? {
		RegistrationVacancy::Vacant => Ok(()),
		RegistrationVacancy::Replaced => Err(AcquisitionFailure::Replaced),
		RegistrationVacancy::Incomplete | RegistrationVacancy::Foreign =>
			Err(AcquisitionFailure::TargetOccupied),
	}
}

fn inspect_registration_vacancy(
	common_dir: &DirectoryPin,
	worktree_path: &Path,
) -> Result<RegistrationVacancy, PathFailure> {
	common_dir.revalidate()?;
	let expected_admin = expected_registration_admin(&common_dir.path, worktree_path)?;
	let expected_backlink = worktree_path.join(".git");
	let root = common_dir.path.join("worktrees");
	if path_is_missing(&root)? {
		return Ok(RegistrationVacancy::Vacant);
	}
	let root_pin = match DirectoryPin::acquire(&root) {
		Ok(pin) => pin,
		Err(PathFailure::Invalid | PathFailure::UnsafeOwner | PathFailure::Replaced) => {
			return Ok(RegistrationVacancy::Replaced);
		},
		Err(error) => return Err(error),
	};
	if validate_repository_owner(&root_pin).is_err() {
		return Ok(RegistrationVacancy::Replaced);
	}
	let entries = fs::read_dir(&root).map_err(path_io)?;
	let started = Instant::now();
	for (count, entry) in entries.enumerate() {
		if count >= MAX_WALK_FILES || started.elapsed() > WALK_TIMEOUT {
			return Err(PathFailure::Io);
		}
		let entry = entry.map_err(|_| PathFailure::Io)?;
		let path = entry.path();
		let metadata = fs::symlink_metadata(&path).map_err(path_io)?;
		if metadata.file_type().is_symlink() {
			return Ok(RegistrationVacancy::Replaced);
		}
		if !metadata.is_dir() {
			return Ok(RegistrationVacancy::Incomplete);
		}
		let admin_pin = match DirectoryPin::acquire(&path) {
			Ok(pin) => pin,
			Err(PathFailure::Invalid | PathFailure::UnsafeOwner | PathFailure::Replaced) => {
				return Ok(RegistrationVacancy::Replaced);
			},
			Err(error) => return Err(error),
		};
		if validate_repository_owner(&admin_pin).is_err() {
			return Ok(RegistrationVacancy::Replaced);
		}
		let gitdir = path.join("gitdir");
		if path_is_missing(&gitdir)? {
			return Ok(RegistrationVacancy::Incomplete);
		}
		if !safe_owned_regular_file(&gitdir) {
			return Ok(RegistrationVacancy::Replaced);
		}
		let backlink = match read_nofollow(&gitdir, 4_096)
			.and_then(|bytes| resolve_metadata_path(path.as_path(), parse_metadata_line(&bytes)?))
		{
			Ok(backlink) => backlink,
			Err(_) if path == expected_admin => return Ok(RegistrationVacancy::Foreign),
			Err(_) => return Ok(RegistrationVacancy::Incomplete),
		};
		if path == expected_admin {
			return Ok(if backlink == expected_backlink {
				RegistrationVacancy::Incomplete
			} else {
				RegistrationVacancy::Foreign
			});
		}
		if backlink == expected_backlink {
			return Ok(RegistrationVacancy::Incomplete);
		}
		admin_pin.revalidate()?;
	}
	root_pin.revalidate()?;
	common_dir.revalidate()?;
	Ok(RegistrationVacancy::Vacant)
}

fn expected_registration_admin(
	common_dir: &Path,
	worktree_path: &Path,
) -> Result<PathBuf, PathFailure> {
	let name = worktree_path.file_name().and_then(OsStr::to_str).ok_or(PathFailure::Invalid)?;
	RepositoryRegistrationId::new(name.to_owned()).map_err(|_| PathFailure::Invalid)?;
	Ok(common_dir.join("worktrees").join(name))
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
