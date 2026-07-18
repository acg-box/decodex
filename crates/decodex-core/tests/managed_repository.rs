use std::{
	collections::{BTreeMap, BTreeSet},
	path::{Path, PathBuf},
};

use decodex_core::{
	AdmittedRepositoryIdentity, AggregateCheckpoint, AllocateRepositoryCommand,
	AllocationAvailabilityFacts, AssignmentResolution, BeginCommitCommand,
	BeginRegistrationCommand, BeginWorktreeReadyCommand, CanonicalCommitIntent, CommitEvidence,
	CommitReconciliation, ExactCommitEvidence, ExactRegistrationEvidence,
	ExactRepositoryReadbackScope, ExactWorktreeReadyEvidence, ExecutorContractVersion,
	ManagedRepositoryFacts, ManagedRepositoryId, ManagedRepositoryPhase, ManagedWorktreeId,
	PersistedAbsolutePath, PositiveAllocationEvidence, ProjectId, RegistrationEvidence,
	RegistrationReconciliation, RepositoryAdmissionDescriptor, RepositoryAdmissionFacts,
	RepositoryAdmittedGitLayout, RepositoryAllocationId, RepositoryAmbiguity,
	RepositoryAuthorityTip, RepositoryCommitActor, RepositoryCommitActorEmail,
	RepositoryCommitActorName, RepositoryCommitMessage, RepositoryContentRevision,
	RepositoryEvidenceId, RepositoryGitRegistrationRole, RepositoryObservationPath,
	RepositoryObservedObjectType, RepositoryOperationId, RepositoryOperationState,
	RepositoryPathObservation, RepositoryPathRegistrationRole, RepositoryReferenceName,
	RepositoryRegistrationId, WorktreeReadyEvidence, WorktreeReadyPolicy,
	WorktreeReadyReconciliation, commit_readback_request, decide_allocate, decide_begin_commit,
	decide_begin_registration, decide_begin_worktree_ready, decide_commit_readback,
	decide_registration_readback, decide_worktree_ready_readback, registration_readback_request,
	resolve_operation_assignment, worktree_ready_readback_request,
};

const PROJECT_ID: &str = "11000000-0000-4000-8000-000000000001";
const REPOSITORY_ID: &str = "12000000-0000-4000-8000-000000000001";
const ALLOCATION_ID: &str = "13000000-0000-4000-8000-000000000001";
const WORKTREE_ID: &str = "14000000-0000-4000-8000-000000000001";
const BASE: &str = "1111111111111111111111111111111111111111";
const NEXT: &str = "2222222222222222222222222222222222222222";

fn path(value: impl Into<PathBuf>) -> PersistedAbsolutePath {
	PersistedAbsolutePath::new(value.into()).expect("fixture path is canonical")
}

fn add_directory_chain(
	roles: &mut BTreeMap<
		PathBuf,
		(RepositoryObservedObjectType, BTreeSet<RepositoryPathRegistrationRole>),
	>,
	endpoint: &Path,
	component_role: RepositoryPathRegistrationRole,
	endpoint_role: RepositoryPathRegistrationRole,
) {
	let mut current = PathBuf::from("/");
	roles
		.entry(current.clone())
		.or_insert_with(|| (RepositoryObservedObjectType::Directory, BTreeSet::new()))
		.1
		.insert(component_role);
	let components = endpoint.components().skip(1).collect::<Vec<_>>();
	for (index, component) in components.iter().enumerate() {
		current.push(component.as_os_str());
		let role = if index + 1 == components.len() { endpoint_role } else { component_role };
		roles
			.entry(current.clone())
			.or_insert_with(|| (RepositoryObservedObjectType::Directory, BTreeSet::new()))
			.1
			.insert(role);
	}
}

fn observations(layout: &RepositoryAdmittedGitLayout) -> Vec<RepositoryPathObservation> {
	let mut roles = BTreeMap::<
		PathBuf,
		(RepositoryObservedObjectType, BTreeSet<RepositoryPathRegistrationRole>),
	>::new();
	add_directory_chain(
		&mut roles,
		layout.repository_root().as_path(),
		RepositoryPathRegistrationRole::RepositoryRootComponent,
		RepositoryPathRegistrationRole::RepositoryRoot,
	);
	add_directory_chain(
		&mut roles,
		layout.git_directory().as_path(),
		RepositoryPathRegistrationRole::GitDirectoryComponent,
		RepositoryPathRegistrationRole::GitDirectory,
	);
	add_directory_chain(
		&mut roles,
		layout.common_directory().as_path(),
		RepositoryPathRegistrationRole::GitCommonDirectoryComponent,
		RepositoryPathRegistrationRole::GitCommonDirectory,
	);
	add_directory_chain(
		&mut roles,
		layout.objects_directory().as_path(),
		RepositoryPathRegistrationRole::GitObjectsDirectoryComponent,
		RepositoryPathRegistrationRole::GitObjectsDirectory,
	);
	if let Some(refs) = layout.refs_directory() {
		add_directory_chain(
			&mut roles,
			refs.as_path(),
			RepositoryPathRegistrationRole::GitRefsDirectoryComponent,
			RepositoryPathRegistrationRole::GitRefsDirectory,
		);
	}
	let git_entry_type = match layout.registration_role() {
		RepositoryGitRegistrationRole::PrimaryWorktree => RepositoryObservedObjectType::Directory,
		RepositoryGitRegistrationRole::LinkedWorktree => RepositoryObservedObjectType::RegularFile,
	};
	roles
		.entry(layout.worktree_git_entry().as_path().to_owned())
		.or_insert_with(|| (git_entry_type, BTreeSet::new()))
		.1
		.insert(RepositoryPathRegistrationRole::WorktreeGitEntry);
	for (candidate, role) in [
		(layout.common_directory_file(), RepositoryPathRegistrationRole::GitCommonDirectoryFile),
		(
			layout.git_directory_backlink_file(),
			RepositoryPathRegistrationRole::GitDirectoryBacklinkFile,
		),
	] {
		if let Some(candidate) = candidate {
			roles
				.entry(candidate.as_path().to_owned())
				.or_insert_with(|| (RepositoryObservedObjectType::RegularFile, BTreeSet::new()))
				.1
				.insert(role);
		}
	}

	roles
		.into_iter()
		.enumerate()
		.map(|(index, (observed_path, (object_type, roles)))| {
			RepositoryPathObservation::new(
				RepositoryObservationPath::new(observed_path).expect("observation path is canonical"),
				roles.into_iter().collect(),
				1,
				u64::try_from(index + 1).expect("fixture inode fits"),
				object_type,
				501,
				0o755,
			)
			.expect("observation is canonical")
		})
		.collect()
}

fn primary_descriptor(identity: &str) -> RepositoryAdmissionDescriptor {
	let root = path("/srv/decodex/repository");
	let git = path("/srv/decodex/repository/.git");
	let layout = RepositoryAdmittedGitLayout::new(
		RepositoryGitRegistrationRole::PrimaryWorktree,
		None,
		root.clone(),
		git.clone(),
		git.clone(),
		git.clone(),
		path("/srv/decodex/repository/.git/objects"),
		Some(path("/srv/decodex/repository/.git/refs")),
		None,
		None,
	);
	RepositoryAdmissionDescriptor::new_v1(
		ProjectId::new(PROJECT_ID).expect("project ID is canonical"),
		ManagedRepositoryId::new(REPOSITORY_ID).expect("repository ID is canonical"),
		AdmittedRepositoryIdentity::new(identity).expect("identity is canonical"),
		RepositoryContentRevision::new(BASE).expect("base is canonical"),
		root,
		layout.clone(),
		observations(&layout),
	)
	.expect("primary descriptor is canonical")
}

fn linked_descriptor() -> RepositoryAdmissionDescriptor {
	let root = path("/srv/decodex/linked");
	let common = path("/srv/decodex/repository/.git");
	let git = path("/srv/decodex/repository/.git/worktrees/linked");
	let layout = RepositoryAdmittedGitLayout::new(
		RepositoryGitRegistrationRole::LinkedWorktree,
		Some(RepositoryRegistrationId::new("linked").expect("registration is canonical")),
		root.clone(),
		path("/srv/decodex/linked/.git"),
		git.clone(),
		common.clone(),
		path("/srv/decodex/repository/.git/objects"),
		Some(path("/srv/decodex/repository/.git/refs")),
		Some(path("/srv/decodex/repository/.git/worktrees/linked/commondir")),
		Some(path("/srv/decodex/repository/.git/worktrees/linked/gitdir")),
	);
	RepositoryAdmissionDescriptor::new_v1(
		ProjectId::new(PROJECT_ID).expect("project ID is canonical"),
		ManagedRepositoryId::new(REPOSITORY_ID).expect("repository ID is canonical"),
		AdmittedRepositoryIdentity::new("device:linked").expect("identity is canonical"),
		RepositoryContentRevision::new(BASE).expect("base is canonical"),
		root,
		layout.clone(),
		observations(&layout),
	)
	.expect("linked descriptor is canonical")
}

fn checkpoint(generation: u64, suffix: u8) -> AggregateCheckpoint {
	AggregateCheckpoint::new(
		generation,
		RepositoryAuthorityTip::new(format!("15000000-0000-4000-8000-{suffix:012}"))
			.expect("tip is canonical"),
	)
	.expect("checkpoint is canonical")
}

fn facts(phase: ManagedRepositoryPhase) -> ManagedRepositoryFacts {
	ManagedRepositoryFacts {
		admission: RepositoryAdmissionFacts::new(primary_descriptor("device:primary")),
		allocation_id: RepositoryAllocationId::new(ALLOCATION_ID)
			.expect("allocation ID is canonical"),
		worktree_id: ManagedWorktreeId::new(WORKTREE_ID).expect("worktree ID is canonical"),
		worktree_path: path("/srv/decodex/worktrees/one"),
		phase,
		head: RepositoryContentRevision::new(BASE).expect("head is canonical"),
		checkpoint: checkpoint(2, 2),
		active_operation: None,
	}
}

fn operation_id(suffix: u8) -> RepositoryOperationId {
	RepositoryOperationId::new(format!("16000000-0000-4000-8000-{suffix:012}"))
		.expect("operation ID is canonical")
}

fn evidence_id(suffix: u8) -> RepositoryEvidenceId {
	RepositoryEvidenceId::new(format!("17000000-0000-4000-8000-{suffix:012}"))
		.expect("evidence ID is canonical")
}

fn commit_intent() -> CanonicalCommitIntent {
	let actor = RepositoryCommitActor::new(
		RepositoryCommitActorName::new("Decodex Acceptance").expect("actor name is canonical"),
		RepositoryCommitActorEmail::new("acceptance@decodex.invalid")
			.expect("actor email is canonical"),
		1_700_000_000,
		0,
	)
	.expect("actor is canonical");
	CanonicalCommitIntent {
		target_reference: RepositoryReferenceName::new("HEAD").expect("reference is canonical"),
		tree: RepositoryContentRevision::new("3333333333333333333333333333333333333333")
			.expect("tree is canonical"),
		message: RepositoryCommitMessage::new("Acceptance fixture\n")
			.expect("message is canonical"),
		author: actor.clone(),
		committer: actor,
	}
}

fn readback_scope(
	facts: &ManagedRepositoryFacts,
	operation_id: RepositoryOperationId,
	suffix: u8,
) -> ExactRepositoryReadbackScope {
	let descriptor = facts.admission.descriptor();
	ExactRepositoryReadbackScope {
		evidence_id: evidence_id(suffix),
		operation_id,
		admitted_identity: descriptor.admitted_identity().clone(),
		admitted_base: descriptor.admitted_base().clone(),
		repository_id: descriptor.repository_id().clone(),
		allocation_id: facts.allocation_id.clone(),
		worktree_id: facts.worktree_id.clone(),
		repository_path: descriptor.repository_path().clone(),
		worktree_path: facts.worktree_path.clone(),
	}
}

#[test]
fn admission_descriptors_are_canonical_complete_and_layout_specific() {
	let primary = primary_descriptor("device:primary");
	let repeated = primary_descriptor("device:primary");
	let changed = primary_descriptor("device:replacement");
	let linked = linked_descriptor();

	assert_eq!(primary, repeated);
	assert_eq!(primary.canonical_bytes(), repeated.canonical_bytes());
	assert!(primary.verify_digest(primary.digest()));
	assert_ne!(primary.digest(), changed.digest());
	assert_eq!(
		linked.git_layout().registration_role(),
		RepositoryGitRegistrationRole::LinkedWorktree
	);
	assert_eq!(
		linked.git_layout().registration_id().map(RepositoryRegistrationId::as_str),
		Some("linked")
	);
}

#[test]
fn allocation_requires_exact_read_only_evidence_and_distinct_paths() {
	let admission = RepositoryAdmissionFacts::new(primary_descriptor("device:primary"));
	let command = AllocateRepositoryCommand {
		allocation_id: RepositoryAllocationId::new(ALLOCATION_ID)
			.expect("allocation ID is canonical"),
		worktree_id: ManagedWorktreeId::new(WORKTREE_ID).expect("worktree ID is canonical"),
		worktree_path: path("/srv/decodex/worktrees/one"),
	};
	let availability = AllocationAvailabilityFacts {
		allocation_id: command.allocation_id.clone(),
		worktree_id: command.worktree_id.clone(),
		worktree_path: command.worktree_path.clone(),
	};
	let evidence = PositiveAllocationEvidence::new(
		evidence_id(1),
		admission.descriptor().clone(),
		command.worktree_path.clone(),
	);
	let decision = decide_allocate(&admission, &availability, &command, &evidence)
		.expect("exact read-only allocation evidence is accepted");

	assert_eq!(decision.head.as_str(), BASE);
	let mut wrong_availability = availability.clone();
	wrong_availability.worktree_path = path("/srv/decodex/worktrees/other");
	assert!(decide_allocate(&admission, &wrong_availability, &command, &evidence).is_err());
	let occupied_source = AllocateRepositoryCommand {
		worktree_path: admission.descriptor().repository_path().clone(),
		..command
	};
	assert!(decide_allocate(&admission, &availability, &occupied_source, &evidence).is_err());
}

#[test]
fn global_operation_assignment_exact_repeat_never_dispatches_and_conflicts_are_permanent() {
	let facts = facts(ManagedRepositoryPhase::Allocated);
	let decision = decide_begin_registration(
		&facts,
		&BeginRegistrationCommand {
			operation_id: operation_id(1),
			expected_checkpoint: facts.checkpoint.clone(),
			expected_head: facts.head.clone(),
			executor_contract: ExecutorContractVersion::new(1).expect("contract is canonical"),
		},
	)
	.expect("registration can begin");

	assert!(matches!(
		resolve_operation_assignment(&decision.descriptor, None),
		AssignmentResolution::NewlyAssigned
	));
	assert!(matches!(
		resolve_operation_assignment(&decision.descriptor, Some(&decision.operation)),
		AssignmentResolution::ExistingExact(_, _)
	));
	let mut conflicting = decision.descriptor.clone();
	conflicting.executor_contract = ExecutorContractVersion::new(2).expect("contract is canonical");
	assert!(matches!(
		resolve_operation_assignment(&conflicting, Some(&decision.operation)),
		AssignmentResolution::OperationIdConflict
	));
	assert_eq!(
		registration_readback_request(&decision.operation)
			.expect("registration readback is transition-specific")
			.descriptor,
		decision.descriptor
	);
	assert!(worktree_ready_readback_request(&decision.operation).is_err());
	assert!(commit_readback_request(&decision.operation).is_err());
}

#[test]
fn register_ready_and_commit_reconcile_only_exact_transition_evidence() {
	let mut repository = facts(ManagedRepositoryPhase::Allocated);
	let registration = decide_begin_registration(
		&repository,
		&BeginRegistrationCommand {
			operation_id: operation_id(1),
			expected_checkpoint: repository.checkpoint.clone(),
			expected_head: repository.head.clone(),
			executor_contract: ExecutorContractVersion::new(1).expect("contract is canonical"),
		},
	)
	.expect("registration can begin");
	repository.active_operation = Some(registration.active_operation.clone());
	assert!(matches!(
		decide_registration_readback(
			&repository,
			&registration.operation,
			&RegistrationEvidence::Unavailable,
		)
			.expect("unavailable readback remains pending"),
		RegistrationReconciliation::Pending
	));
	assert!(matches!(
		decide_registration_readback(&repository, &registration.operation, &RegistrationEvidence::Dirty)
			.expect("dirty readback terminalizes ambiguity"),
		RegistrationReconciliation::Ambiguous {
			repository: decodex_core::RepositoryProjectionUpdate {
				phase: ManagedRepositoryPhase::Ambiguous(RepositoryAmbiguity::Dirty),
				..
			},
			..
		}
	));
	let exact_registration = RegistrationEvidence::ExactReciprocal(ExactRegistrationEvidence {
		scope: readback_scope(&repository, operation_id(1), 1),
		repository_names_worktree: repository.worktree_id.clone(),
		worktree_names_repository: repository.admission.descriptor().repository_id().clone(),
		unchanged_head: repository.head.clone(),
	});
	let RegistrationReconciliation::Completed { repository: update, operation, .. } =
		decide_registration_readback(&repository, &registration.operation, &exact_registration)
			.expect("exact reciprocal registration completes")
	else {
		panic!("registration did not complete");
	};
	assert_eq!(update.phase, ManagedRepositoryPhase::Registered);
	assert_eq!(update.head.as_str(), BASE);
	assert!(matches!(operation.state, RepositoryOperationState::Completed(_)));

	repository.phase = ManagedRepositoryPhase::Registered;
	repository.active_operation = None;
	repository.checkpoint = checkpoint(4, 4);
	let ready = decide_begin_worktree_ready(
		&repository,
		&BeginWorktreeReadyCommand {
			operation_id: operation_id(2),
			expected_checkpoint: repository.checkpoint.clone(),
			expected_head: repository.head.clone(),
			policy: WorktreeReadyPolicy::ExactCleanWorktree,
			executor_contract: ExecutorContractVersion::new(1).expect("contract is canonical"),
		},
	)
	.expect("ready can begin");
	repository.active_operation = Some(ready.active_operation.clone());
	let ready_evidence = WorktreeReadyEvidence::Exact(ExactWorktreeReadyEvidence {
		scope: readback_scope(&repository, operation_id(2), 2),
		unchanged_head: repository.head.clone(),
	});
	let WorktreeReadyReconciliation::Completed { repository: update, .. } =
		decide_worktree_ready_readback(&repository, &ready.operation, &ready_evidence)
			.expect("exact unchanged-head readiness completes")
	else {
		panic!("readiness did not complete");
	};
	assert_eq!(update.phase, ManagedRepositoryPhase::Ready);
	assert_eq!(update.head.as_str(), BASE);

	repository.phase = ManagedRepositoryPhase::Ready;
	repository.active_operation = None;
	repository.checkpoint = checkpoint(6, 6);
	let intent = commit_intent();
	let commit = decide_begin_commit(
		&repository,
		&BeginCommitCommand {
			operation_id: operation_id(3),
			expected_checkpoint: repository.checkpoint.clone(),
			expected_head: repository.head.clone(),
			next_head: RepositoryContentRevision::new(NEXT).expect("next head is canonical"),
			intent: intent.clone(),
			executor_contract: ExecutorContractVersion::new(1).expect("contract is canonical"),
		},
	)
	.expect("commit can begin");
	repository.active_operation = Some(commit.active_operation.clone());
	let exact_commit = CommitEvidence::Exact(ExactCommitEvidence {
		scope: readback_scope(&repository, operation_id(3), 3),
		target_reference: RepositoryReferenceName::new("HEAD").expect("reference is canonical"),
		intent,
		predecessor_head: repository.head.clone(),
		completed_head: RepositoryContentRevision::new(NEXT).expect("next head is canonical"),
	});
	let CommitReconciliation::Completed { repository: update, .. } =
		decide_commit_readback(&repository, &commit.operation, &exact_commit)
			.expect("exact one-head commit completes")
	else {
		panic!("commit did not complete");
	};
	assert_eq!(update.phase, ManagedRepositoryPhase::Ready);
	assert_eq!(update.head.as_str(), NEXT);
	assert!(matches!(
		decide_commit_readback(&repository, &commit.operation, &CommitEvidence::Rollback)
			.expect("rollback terminalizes ambiguity"),
		CommitReconciliation::Ambiguous {
			repository: decodex_core::RepositoryProjectionUpdate {
				phase: ManagedRepositoryPhase::Ambiguous(RepositoryAmbiguity::Rollback),
				..
			},
			..
		}
	));
}
