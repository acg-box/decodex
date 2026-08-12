use std::{
	collections::{BTreeMap, BTreeSet},
	path::{Path, PathBuf},
};

use decodex_core::{
	AdmittedRepositoryIdentity, BeginCommitCommand, BeginRegistrationCommand,
	BeginWorktreeReadyCommand, CanonicalCommitIntent, CommitEvidence, ExactCommitEvidence,
	ExactRegistrationEvidence, ExactRepositoryReadbackScope, ExactWorktreeReadyEvidence,
	ExecutorContractVersion, ManagedRepositoryFacts, ManagedRepositoryId, ManagedRepositoryPhase,
	ManagedWorktreeId, PersistedAbsolutePath, PositiveAllocationEvidence, ProjectId,
	RegistrationEvidence, RepositoryAdmissionDescriptor, RepositoryAdmissionFacts,
	RepositoryAdmittedGitLayout, RepositoryAllocationId, RepositoryCommitActor,
	RepositoryCommitActorEmail, RepositoryCommitActorName, RepositoryCommitMessage,
	RepositoryContentRevision, RepositoryEvidenceId, RepositoryGitRegistrationRole,
	RepositoryObservationPath, RepositoryObservedObjectType, RepositoryOperationId,
	RepositoryOperationState, RepositoryPathObservation, RepositoryPathRegistrationRole,
	RepositoryReferenceName, WorktreeReadyEvidence, WorktreeReadyPolicy,
};
use decodex_postgres::{
	AllocateRepositoryCommand, PostgresStore, RepositoryAdmissionOutcome,
	RepositoryDispatchFenceOutcome, RepositoryPreparationOutcome, RepositoryReadbackEvidence,
	RepositoryReadbackWork, RepositoryReconciliationOutcome, StoreError,
};

use super::{expected_peer_uid, owner_runtime_configs, project_request};

const PROJECT_ID: &str = "41000000-0000-4000-8000-000000000001";
const LEAD_ID: &str = "42000000-0000-4000-8000-000000000001";
const BASE: &str = "1111111111111111111111111111111111111111";
const NEXT: &str = "2222222222222222222222222222222222222222";

fn uuid(prefix: u8, value: usize) -> String {
	format!("{prefix:02x}000000-0000-4000-8000-{value:012}")
}

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

fn descriptor(value: usize) -> RepositoryAdmissionDescriptor {
	let root = path(format!("/srv/decodex/acceptance/repository-{value}"));
	let git = path(root.as_path().join(".git"));
	let objects = path(git.as_path().join("objects"));
	let refs = path(git.as_path().join("refs"));
	let layout = RepositoryAdmittedGitLayout::new(
		RepositoryGitRegistrationRole::PrimaryWorktree,
		None,
		root.clone(),
		git.clone(),
		git.clone(),
		git.clone(),
		objects,
		Some(refs),
		None,
		None,
	);
	let mut roles = BTreeMap::<
		PathBuf,
		(RepositoryObservedObjectType, BTreeSet<RepositoryPathRegistrationRole>),
	>::new();
	for (endpoint, component, exact) in [
		(
			layout.repository_root(),
			RepositoryPathRegistrationRole::RepositoryRootComponent,
			RepositoryPathRegistrationRole::RepositoryRoot,
		),
		(
			layout.git_directory(),
			RepositoryPathRegistrationRole::GitDirectoryComponent,
			RepositoryPathRegistrationRole::GitDirectory,
		),
		(
			layout.common_directory(),
			RepositoryPathRegistrationRole::GitCommonDirectoryComponent,
			RepositoryPathRegistrationRole::GitCommonDirectory,
		),
		(
			layout.objects_directory(),
			RepositoryPathRegistrationRole::GitObjectsDirectoryComponent,
			RepositoryPathRegistrationRole::GitObjectsDirectory,
		),
		(
			layout.refs_directory().expect("fixture has refs"),
			RepositoryPathRegistrationRole::GitRefsDirectoryComponent,
			RepositoryPathRegistrationRole::GitRefsDirectory,
		),
	] {
		add_directory_chain(&mut roles, endpoint.as_path(), component, exact);
	}
	roles
		.entry(layout.worktree_git_entry().as_path().to_owned())
		.or_insert_with(|| (RepositoryObservedObjectType::Directory, BTreeSet::new()))
		.1
		.insert(RepositoryPathRegistrationRole::WorktreeGitEntry);
	let observations = roles
		.into_iter()
		.enumerate()
		.map(|(index, (observed_path, (object_type, roles)))| {
			RepositoryPathObservation::new(
				RepositoryObservationPath::new(observed_path).expect("path is canonical"),
				roles.into_iter().collect(),
				1,
				u64::try_from(index + 1).expect("inode fits"),
				object_type,
				501,
				0o755,
			)
			.expect("observation is canonical")
		})
		.collect();
	RepositoryAdmissionDescriptor::new_v1(
		ProjectId::new(PROJECT_ID).expect("project ID is canonical"),
		ManagedRepositoryId::new(uuid(0x43, value)).expect("repository ID is canonical"),
		AdmittedRepositoryIdentity::new(format!("fixture-device-{value}"))
			.expect("identity is canonical"),
		RepositoryContentRevision::new(BASE).expect("base is canonical"),
		root,
		layout,
		observations,
	)
	.expect("descriptor is canonical")
}

fn allocation(value: usize) -> AllocateRepositoryCommand {
	AllocateRepositoryCommand {
		allocation_id: RepositoryAllocationId::new(uuid(0x44, value))
			.expect("allocation ID is canonical"),
		worktree_id: ManagedWorktreeId::new(uuid(0x45, value)).expect("worktree ID is canonical"),
		worktree_path: path(format!("/srv/decodex/acceptance/worktree-{value}")),
	}
}

fn evidence(
	value: usize,
	descriptor: &RepositoryAdmissionDescriptor,
) -> PositiveAllocationEvidence {
	PositiveAllocationEvidence::new(
		RepositoryEvidenceId::new(uuid(0x46, value)).expect("evidence ID is canonical"),
		descriptor.clone(),
		allocation(value).worktree_path,
	)
}

fn operation_id(value: usize) -> RepositoryOperationId {
	RepositoryOperationId::new(uuid(0x47, value)).expect("operation ID is canonical")
}

fn scope(
	facts: &ManagedRepositoryFacts,
	operation_id: RepositoryOperationId,
	evidence_id: RepositoryEvidenceId,
) -> ExactRepositoryReadbackScope {
	let descriptor = facts.admission.descriptor();
	ExactRepositoryReadbackScope {
		evidence_id,
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

fn registration_evidence(
	facts: &ManagedRepositoryFacts,
	operation_id: RepositoryOperationId,
	evidence_id: RepositoryEvidenceId,
) -> RegistrationEvidence {
	RegistrationEvidence::ExactReciprocal(ExactRegistrationEvidence {
		scope: scope(facts, operation_id, evidence_id),
		repository_names_worktree: facts.worktree_id.clone(),
		worktree_names_repository: facts.admission.descriptor().repository_id().clone(),
		unchanged_head: facts.head.clone(),
	})
}

fn commit_intent() -> CanonicalCommitIntent {
	let actor = RepositoryCommitActor::new(
		RepositoryCommitActorName::new("PostgreSQL Acceptance").expect("name is canonical"),
		RepositoryCommitActorEmail::new("postgres@decodex.invalid").expect("email is canonical"),
		1_700_000_000,
		0,
	)
	.expect("actor is canonical");
	CanonicalCommitIntent {
		target_reference: RepositoryReferenceName::new("HEAD").expect("reference is canonical"),
		tree: RepositoryContentRevision::new("3333333333333333333333333333333333333333")
			.expect("tree is canonical"),
		message: RepositoryCommitMessage::new("PostgreSQL acceptance fixture\n")
			.expect("message is canonical"),
		author: actor.clone(),
		committer: actor,
	}
}

async fn ensure_project(store: &PostgresStore) -> Result<(), Box<dyn std::error::Error>> {
	if store.project(&ProjectId::new(PROJECT_ID)?).await?.is_none() {
		store
			.create_project(project_request(
				PROJECT_ID,
				LEAD_ID,
				"acg-box/managed-repository-acceptance",
				"/srv/decodex/acceptance",
			))
			.await?;
	}
	Ok(())
}

async fn admit_and_allocate(
	store: &PostgresStore,
	value: usize,
) -> Result<ManagedRepositoryFacts, Box<dyn std::error::Error>> {
	let descriptor = descriptor(value);
	let admission = RepositoryAdmissionFacts::new(descriptor.clone());
	store.admit_repository(&admission).await?;
	let command = allocation(value);
	Ok(store
		.allocate_repository(descriptor.repository_id(), &command, &evidence(value, &descriptor))
		.await?)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires the isolated PostgreSQL 18 frozen-tree harness"]
#[allow(clippy::too_many_lines)] // One representative durable authority lifecycle.
async fn postgres_managed_repository_authority_contract() -> Result<(), Box<dyn std::error::Error>>
{
	let (_, runtime) = owner_runtime_configs("DECODEX_TEST")?;
	let store =
		PostgresStore::connect_runtime_fixture(runtime.clone(), expected_peer_uid()).await?;
	ensure_project(&store).await?;
	let descriptor = descriptor(1);
	let admission = RepositoryAdmissionFacts::new(descriptor.clone());
	assert_eq!(store.admit_repository(&admission).await?, RepositoryAdmissionOutcome::Admitted);
	assert_eq!(
		store.admit_repository(&admission).await?,
		RepositoryAdmissionOutcome::ExistingExact
	);
	let command = allocation(1);
	let mut facts = store
		.allocate_repository(descriptor.repository_id(), &command, &evidence(1, &descriptor))
		.await?;
	assert_eq!(facts.phase, ManagedRepositoryPhase::Allocated);
	assert_eq!(facts.head.as_str(), BASE);

	let register_command = BeginRegistrationCommand {
		operation_id: operation_id(1),
		expected_checkpoint: facts.checkpoint.clone(),
		expected_head: facts.head.clone(),
		executor_contract: ExecutorContractVersion::new(1)?,
	};
	let prepared =
		store.prepare_registration(descriptor.repository_id(), &register_command).await?;
	let (operation, receipt) = match prepared {
		RepositoryPreparationOutcome::Prepared { operation, receipt } => (operation, receipt),
		RepositoryPreparationOutcome::ExistingExact(_, _) =>
			panic!("first operation was not fresh"),
	};
	assert!(matches!(operation.state, RepositoryOperationState::PossiblyEffected));
	assert!(matches!(
		store.prepare_registration(descriptor.repository_id(), &register_command).await?,
		RepositoryPreparationOutcome::ExistingExact(_, _)
	));
	let fenced = store
		.consume_repository_dispatch(receipt, |receipt, current| {
			(receipt.descriptor().operation_id.clone(), current.checkpoint.clone())
		})
		.await?;
	assert!(matches!(
		fenced,
		RepositoryDispatchFenceOutcome::Authorized { release_confirmed: true, .. }
	));
	assert!(matches!(
		store
			.reconcile_repository_readback(&operation_id(1), |_, _, _| {
				RepositoryReadbackEvidence::Registration(RegistrationEvidence::Unavailable)
			})
			.await?,
		RepositoryReconciliationOutcome::Pending(_)
	));
	assert_eq!(store.load_repository_restart_work(256).await?.len(), 1);
	let before_registration = store
		.read_managed_repository(descriptor.repository_id())
		.await?
		.expect("repository remains present");
	let terminal = store
		.reconcile_repository_readback(&operation_id(1), |work, _, evidence_id| {
			assert!(matches!(work, RepositoryReadbackWork::Registration(_)));
			RepositoryReadbackEvidence::Registration(registration_evidence(
				&before_registration,
				operation_id(1),
				evidence_id,
			))
		})
		.await?;
	let RepositoryReconciliationOutcome::Terminal { repository, .. } = terminal else {
		panic!("registration did not terminalize");
	};
	facts = repository;
	assert_eq!(facts.phase, ManagedRepositoryPhase::Registered);
	assert_eq!(facts.head.as_str(), BASE);

	let ready_command = BeginWorktreeReadyCommand {
		operation_id: operation_id(2),
		expected_checkpoint: facts.checkpoint.clone(),
		expected_head: facts.head.clone(),
		policy: WorktreeReadyPolicy::ExactCleanWorktree,
		executor_contract: ExecutorContractVersion::new(1)?,
	};
	let ready_operation =
		match store.prepare_worktree_ready(descriptor.repository_id(), &ready_command).await? {
			RepositoryPreparationOutcome::Prepared { operation, receipt } => {
				drop(receipt);
				operation
			},
			RepositoryPreparationOutcome::ExistingExact(_, _) =>
				panic!("ready operation was not fresh"),
		};
	let wrong_kind = store
		.reconcile_repository_readback(&operation_id(2), |_, _, _| {
			RepositoryReadbackEvidence::Commit(CommitEvidence::NoEffect)
		})
		.await;
	assert!(matches!(wrong_kind, Err(StoreError::Incompatible(_))));
	assert!(matches!(
		store
			.read_repository_operation(&operation_id(2))
			.await?
			.expect("operation remains durable")
			.state,
		RepositoryOperationState::PossiblyEffected
	));
	let before_ready = store
		.read_managed_repository(descriptor.repository_id())
		.await?
		.expect("repository remains present");
	let ready = store
		.reconcile_repository_readback(&operation_id(2), |work, _, evidence_id| {
			assert!(matches!(work, RepositoryReadbackWork::WorktreeReady(_)));
			RepositoryReadbackEvidence::WorktreeReady(WorktreeReadyEvidence::Exact(
				ExactWorktreeReadyEvidence {
					scope: scope(&before_ready, operation_id(2), evidence_id),
					unchanged_head: before_ready.head.clone(),
				},
			))
		})
		.await?;
	let RepositoryReconciliationOutcome::Terminal { repository, .. } = ready else {
		panic!("ready operation did not terminalize");
	};
	facts = repository;
	assert_eq!(facts.phase, ManagedRepositoryPhase::Ready);
	assert_eq!(facts.head.as_str(), BASE);
	assert_eq!(ready_operation.descriptor.operation_id, operation_id(2));

	let intent = commit_intent();
	let commit_command = BeginCommitCommand {
		operation_id: operation_id(3),
		expected_checkpoint: facts.checkpoint.clone(),
		expected_head: facts.head.clone(),
		next_head: RepositoryContentRevision::new(NEXT)?,
		intent: intent.clone(),
		executor_contract: ExecutorContractVersion::new(1)?,
	};
	match store.prepare_commit(descriptor.repository_id(), &commit_command).await? {
		RepositoryPreparationOutcome::Prepared { receipt, .. } => drop(receipt),
		RepositoryPreparationOutcome::ExistingExact(_, _) =>
			panic!("commit operation was not fresh"),
	}
	let before_commit = store
		.read_managed_repository(descriptor.repository_id())
		.await?
		.expect("repository remains present");
	let commit = store
		.reconcile_repository_readback(&operation_id(3), |work, _, evidence_id| {
			assert!(matches!(work, RepositoryReadbackWork::Commit(_)));
			RepositoryReadbackEvidence::Commit(CommitEvidence::Exact(ExactCommitEvidence {
				scope: scope(&before_commit, operation_id(3), evidence_id),
				target_reference: RepositoryReferenceName::new("HEAD")
					.expect("reference is canonical"),
				intent: intent.clone(),
				predecessor_head: RepositoryContentRevision::new(BASE).expect("base is canonical"),
				completed_head: RepositoryContentRevision::new(NEXT)
					.expect("next head is canonical"),
			}))
		})
		.await?;
	let RepositoryReconciliationOutcome::Terminal { repository, operation } = commit else {
		panic!("commit did not terminalize");
	};
	assert_eq!(repository.phase, ManagedRepositoryPhase::Ready);
	assert_eq!(repository.head.as_str(), NEXT);
	assert!(matches!(operation.state, RepositoryOperationState::Completed(_)));

	let conflict = store
		.prepare_registration(
			descriptor.repository_id(),
			&BeginRegistrationCommand {
				operation_id: operation_id(1),
				expected_checkpoint: repository.checkpoint.clone(),
				expected_head: repository.head.clone(),
				executor_contract: ExecutorContractVersion::new(1)?,
			},
		)
		.await;
	assert!(matches!(conflict, Err(StoreError::OperationIdConflict)));

	let concurrent_facts = admit_and_allocate(&store, 2).await?;
	let concurrent_command = BeginRegistrationCommand {
		operation_id: operation_id(4),
		expected_checkpoint: concurrent_facts.checkpoint.clone(),
		expected_head: concurrent_facts.head.clone(),
		executor_contract: ExecutorContractVersion::new(1)?,
	};
	let repository_id = concurrent_facts.admission.descriptor().repository_id().clone();
	let left_store = store.clone();
	let right_store = store.clone();
	let left_command = concurrent_command.clone();
	let right_command = concurrent_command;
	let (left, right) = tokio::join!(
		left_store.prepare_registration(&repository_id, &left_command),
		right_store.prepare_registration(&repository_id, &right_command),
	);
	let mut fresh = 0;
	let mut exact = 0;
	for outcome in [left?, right?] {
		match outcome {
			RepositoryPreparationOutcome::Prepared { receipt, .. } => {
				fresh += 1;
				drop(receipt);
			},
			RepositoryPreparationOutcome::ExistingExact(_, _) => exact += 1,
		}
	}
	assert_eq!((fresh, exact), (1, 1));
	store
		.reconcile_repository_readback(&operation_id(4), |_, _, _| {
			RepositoryReadbackEvidence::Registration(RegistrationEvidence::NoEffect)
		})
		.await?;

	let (runtime_client, connection) = runtime.connect(tokio_postgres::NoTls).await?;
	let connection_task = tokio::spawn(connection);
	assert!(
		runtime_client
			.execute(
				"UPDATE decodex.repository_operation_evidence SET kind='registration' \
			 WHERE operation_id=$1::text::uuid",
				&[&operation_id(3).as_str()],
			)
			.await
			.is_err()
	);
	drop(runtime_client);
	connection_task.await??;
	store.close();
	Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "requires the isolated PostgreSQL 18 frozen-tree restart-bound harness"]
async fn postgres_managed_repository_restart_backlog_bound()
-> Result<(), Box<dyn std::error::Error>> {
	let (_, runtime) = owner_runtime_configs("DECODEX_TEST")?;
	let store =
		PostgresStore::connect_runtime_fixture(runtime.clone(), expected_peer_uid()).await?;
	ensure_project(&store).await?;
	assert!(store.load_repository_restart_work(256).await?.is_empty());
	for value in 10..267 {
		let facts = admit_and_allocate(&store, value).await?;
		match store
			.prepare_registration(
				facts.admission.descriptor().repository_id(),
				&BeginRegistrationCommand {
					operation_id: operation_id(value),
					expected_checkpoint: facts.checkpoint.clone(),
					expected_head: facts.head.clone(),
					executor_contract: ExecutorContractVersion::new(1)?,
				},
			)
			.await?
		{
			RepositoryPreparationOutcome::Prepared { receipt, .. } => drop(receipt),
			RepositoryPreparationOutcome::ExistingExact(_, _) => panic!("operation was not fresh"),
		}
	}
	let first = store.load_repository_restart_work(256).await?;
	assert_eq!(first.len(), 256);
	for state in first {
		let operation_id = state.operation.descriptor.operation_id;
		store
			.reconcile_repository_readback(&operation_id, |work, _, _| match work {
				RepositoryReadbackWork::Registration(_) =>
					RepositoryReadbackEvidence::Registration(RegistrationEvidence::NoEffect),
				RepositoryReadbackWork::WorktreeReady(_) | RepositoryReadbackWork::Commit(_) => {
					panic!("restart fixture contains a non-registration operation")
				},
			})
			.await?;
	}
	let residual = store.load_repository_restart_work(1).await?;
	assert_eq!(residual.len(), 1);
	let residual_id = residual[0].operation.descriptor.operation_id.clone();
	store
		.reconcile_repository_readback(&residual_id, |_, _, _| {
			RepositoryReadbackEvidence::Registration(RegistrationEvidence::NoEffect)
		})
		.await?;
	assert!(store.load_repository_restart_work(1).await?.is_empty());
	store.close();
	Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the isolated PostgreSQL 18 populated restore harness"]
async fn postgres_managed_repository_restored_contract() -> Result<(), Box<dyn std::error::Error>> {
	let (_, runtime) = owner_runtime_configs("DECODEX_TEST")?;
	let store =
		PostgresStore::connect_runtime_fixture(runtime.clone(), expected_peer_uid()).await?;
	let repository = store
		.read_managed_repository(&ManagedRepositoryId::new(uuid(0x43, 1))?)
		.await?
		.expect("managed repository survived populated restore");
	assert_eq!(repository.phase, ManagedRepositoryPhase::Ready);
	assert_eq!(repository.head.as_str(), NEXT);
	let operation = store
		.read_repository_operation(&operation_id(3))
		.await?
		.expect("commit operation survived populated restore");
	assert!(matches!(operation.state, RepositoryOperationState::Completed(_)));
	assert!(store.load_repository_restart_work(1).await?.is_empty());
	store.close();
	Ok(())
}
