use tokio::task::JoinSet;
use tokio_postgres::{Client, Config};

use super::{expected_peer_uid, isolated_blob_store};
use decodex_core::{
	AccountOperationId, AccountProvider, AccountRegistryRoutingDecisionKind, CodexCapability,
	ContinuationCommandOutcome, ConversationId, CredentialBinding, CredentialFingerprint,
	CredentialStoreSchemaVersion, CredentialVersion, ExecutionConsumer, HistoryItemId,
	HistoryItemKind, HistoryMediaType, HistoryMetadata, ItemStatus, ManagedExecutionId,
	ManagedRunId, PossibleSideEffects, ProcessBootIdentity, ProcessControlKind,
	ProcessDeathEvidence, ProcessDeathEvidenceId, ProcessDeathEvidenceKind,
	ProcessExecutionAuthorization, ProcessExecutionEpochId, ProcessGenerationAccountBinding,
	ProcessGenerationId, ProcessGenerationIntent, ProcessIdentity, ProcessIsolationKind,
	ProcessRunnerIdentity, ProcessStartIdentity, ProviderIdentity, RoutingCapabilityState,
	RoutingCommandOutcome, RoutingDecisionKind, RoutingMemberDisposition, RuntimeSessionId,
	RuntimeSessionState, TurnId, TurnRole,
};
use decodex_postgres::{
	AccountId, AccountState, AdmitInitialQuickTaskTurn, BindQuickTaskContinuation,
	BindRuntimeSessionThreadOutcome, CommandIdentity, ContinuationPlanEffect, CreateConversation,
	CreateQuickTaskConversation, CreateQuickTaskRoutingSuccessor, CreateRuntimeSession,
	CreateRuntimeSessionAccountSnapshot, FenceRuntimeSessionThreadStart,
	FenceRuntimeSessionThreadStartOutcome, InitialQuickTaskTurnAdmissionOutcome,
	OrdinaryTaskConversationProjection, OrdinaryTaskPreSessionState, PersistedRoutingDecision,
	PlanInitialThreadContinuation, PostgresStore, PrepareProcessGenerationOutcome,
	PrepareQuickTaskProcessGeneration, PrepareQuickTaskProcessGenerationOutcome,
	ProcessGenerationMutationOutcome, PublishRoutingEvidence, QuickTaskContinuationBinding,
	QuickTaskInitialRoute, QuickTaskInitialRouteOutcome, QuickTaskRoutingSuccessorOutcome,
	RecordHistoryItem, ReplaceRoutingPolicy, RoleProfileRole, RouteAccount, RouteQuickTaskInitial,
	RoutingPolicyMemberInput, RuntimeSessionCommandOutcome, StoreError, StoredRuntimeSession,
	SuccessfulRuntimeSessionThreadStart,
};

const PROJECT_ID: &str = "a1000000-0000-4000-8000-000000000016";
const LEAD_ID: &str = "a2000000-0000-4000-8000-000000000016";
const ACCEPTED_POLICY_ID: &str = "a3000000-0000-4000-8000-000000000016";
const SELECTED_POLICY_ID: &str = "a4000000-0000-4000-8000-000000000016";
const WAITING_POLICY_ID: &str = "a4000000-0000-4000-8000-000000000017";
const NO_ROUTE_POLICY_ID: &str = "a4000000-0000-4000-8000-000000000018";
const CANCEL_POLICY_ID: &str = "a4000000-0000-4000-8000-000000000019";
const STALE_POLICY_ID: &str = "a4000000-0000-4000-8000-000000000020";
const SELECTED_ACCOUNT_ID: &str = "a5000000-0000-4000-8000-000000000016";
const WAITING_ACCOUNT_ID: &str = "a5000000-0000-4000-8000-000000000017";
const NO_ROUTE_ACCOUNT_ID: &str = "a5000000-0000-4000-8000-000000000018";
const BUILD_ID: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const NO_ROUTE_BUILD_ID: &str =
	"sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const SCHEMA_FINGERPRINT: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const PROCESS_EXECUTION_EPOCH_ID: &str = "d1000000-0000-4000-8000-000000001416";
const PROCESS_AUTHORIZATION_DIGEST: &str =
	"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const PROCESS_CREDENTIAL_FINGERPRINT: &str =
	"eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const PROCESS_CALLBACK_PROFILE: &str =
	"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const CURRENT_CODEX_EXECUTABLE_SHA256: &str =
	"eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";

#[derive(Clone)]
pub(super) struct RoutingFixture {
	pub continuation: QuickTaskContinuationBinding,
	pub waiting: PersistedRoutingDecision,
	pub cancel_waiting: PersistedRoutingDecision,
	pub stale_waiting: PersistedRoutingDecision,
	pub selected_account_id: AccountId,
	pub selected_runtime_session_id: RuntimeSessionId,
	pub continuation_runtime_session_id: RuntimeSessionId,
	pub stale_policy_id: String,
}

struct RunFixture {
	conversation_id: ConversationId,
	managed_run_id: ManagedRunId,
	execution_id: ManagedExecutionId,
	runtime_session_id: RuntimeSessionId,
	turn_id: TurnId,
}

struct RoutingContractSetup {
	selected_run: RunFixture,
	selected_request: RouteAccount,
	waiting_request: RouteAccount,
	no_route_request: RouteAccount,
	cancel_request: RouteAccount,
	stale_request: RouteAccount,
}

pub(super) async fn assert_routing_decision_contract(
	store: &PostgresStore,
	owner: &Client,
	runtime: &Config,
) -> Result<RoutingFixture, Box<dyn std::error::Error>> {
	let setup = prepare_routing_contract(store, owner).await?;
	assert_conversation_split_routing_rejected(store, owner, &setup).await?;
	assert_rolled_back_routing_decision(owner, &setup.selected_request).await?;
	let selected = assert_selected_routing_decision(store, owner, &setup.selected_request).await?;
	let (waiting, cancel_waiting, stale_waiting) = assert_alternate_routing_decisions(
		store,
		&setup.waiting_request,
		&setup.no_route_request,
		&setup.cancel_request,
		&setup.stale_request,
	)
	.await?;
	assert_concurrent_routing_replay(store, owner, &setup.selected_request).await?;
	let (continuation, _, continuation_runtime_session_id, _) =
		prepare_quick_task_continuation_route(store, owner).await?;
	assert_atomic_quick_task_route_race(store, owner).await?;
	assert_quick_task_routing_successor(store, owner).await?;

	let restarted =
		PostgresStore::connect_runtime_fixture(runtime.clone(), expected_peer_uid()).await?;
	assert_eq!(
		restarted.route_account("v16-selected", &setup.selected_request).await?,
		RoutingCommandOutcome::Success(selected.clone()),
	);

	Ok(RoutingFixture {
		continuation,
		waiting,
		cancel_waiting,
		stale_waiting,
		selected_account_id: AccountId::new(SELECTED_ACCOUNT_ID)?,
		selected_runtime_session_id: setup.selected_run.runtime_session_id,
		continuation_runtime_session_id,
		stale_policy_id: STALE_POLICY_ID.to_owned(),
	})
}

async fn assert_atomic_quick_task_route_race(
	store: &PostgresStore,
	owner: &Client,
) -> Result<(), Box<dyn std::error::Error>> {
	let conversation_id = ConversationId::new(uuid(0xca, 17))?;
	store
		.create_quick_task_conversation(
			&CommandIdentity::new("v16-quick-task-race-conversation", b"v16-race")?,
			&CreateQuickTaskConversation {
				conversation_id: conversation_id.clone(),
				title: "V16 initial route race".into(),
				message: "Serialize two initial route keys.".into(),
				working_directory: "/tmp".into(),
			},
		)
		.await?;
	let request = RouteQuickTaskInitial {
		conversation_id: conversation_id.clone(),
		expected_conversation_revision: 1,
	};
	let keys = ["v16-quick-task-race-a", "v16-quick-task-race-b"];
	let mut racers = JoinSet::new();
	for key in keys {
		let store = store.clone();
		let request = request.clone();
		racers.spawn(async move { (key, store.route_quick_task_initial(key, &request).await) });
	}
	let mut winner = None;
	let mut rejection_count = 0;
	while let Some(result) = racers.join_next().await {
		let (_key, outcome) = result?;
		match outcome? {
			QuickTaskInitialRouteOutcome::Fresh(route) => {
				assert!(winner.replace(route).is_none(), "initial route race had two winners");
			},
			QuickTaskInitialRouteOutcome::Rejected(rejection)
				if rejection.code == "initial_routing_already_bound" =>
			{
				rejection_count += 1;
			},
			other => return Err(format!("unexpected initial route race outcome: {other:?}").into()),
		}
	}
	let winner = winner
		.ok_or_else(|| StoreError::Incompatible("initial route race had no winner".into()))?;
	assert_eq!(rejection_count, 1);
	assert_eq!(store.read_quick_task_initial_route(&conversation_id).await?, Some(winner));
	for key in keys {
		assert_eq!(receipt_count(owner, key).await?, 1);
	}
	Ok(())
}

async fn assert_quick_task_routing_successor(
	store: &PostgresStore,
	owner: &Client,
) -> Result<(), Box<dyn std::error::Error>> {
	set_account_registry_quota_usage(owner, SELECTED_ACCOUNT_ID, 100).await?;
	let source_id = ConversationId::new(uuid(0xca, 18))?;
	store
		.create_quick_task_conversation(
			&CommandIdentity::new("v16-routing-successor-source", b"v16-successor")?,
			&CreateQuickTaskConversation {
				conversation_id: source_id.clone(),
				title: "V16 waiting routing source".into(),
				message: "Create a fresh routing Conversation.".into(),
				working_directory: "/tmp".into(),
			},
		)
		.await?;
	let route = match store
		.route_quick_task_initial(
			"v16-routing-successor-source-route",
			&RouteQuickTaskInitial {
				conversation_id: source_id.clone(),
				expected_conversation_revision: 1,
			},
		)
		.await?
	{
		QuickTaskInitialRouteOutcome::Fresh(route) => route,
		other => return Err(format!("waiting source route was not fresh: {other:?}").into()),
	};
	assert_eq!(route.decision.kind, AccountRegistryRoutingDecisionKind::Waiting);
	assert!(route.decision.selected_account_id.is_none());
	assert!(route.decision.causes.is_empty());
	assert!(!route.decision.exclusions.is_empty());

	let request = CreateQuickTaskRoutingSuccessor {
		source_conversation_id: source_id.clone(),
		expected_source_revision: 1,
	};
	let successor =
		match store.create_quick_task_routing_successor("v16-routing-successor", &request).await? {
			QuickTaskRoutingSuccessorOutcome::Fresh(successor) => successor,
			other => return Err(format!("routing successor was not fresh: {other:?}").into()),
		};
	assert_eq!(successor.source_routing_decision_id, route.decision_id);
	assert_eq!(successor.source_revision, 2);
	assert_eq!(successor.successor_revision, 1);
	let receipt = receipt_bytes(owner, "v16-routing-successor").await?;
	assert_eq!(
		store.create_quick_task_routing_successor("v16-routing-successor", &request).await?,
		QuickTaskRoutingSuccessorOutcome::Replayed(successor.clone()),
	);
	assert_eq!(receipt_bytes(owner, "v16-routing-successor").await?, receipt);
	assert!(matches!(
		store
			.create_quick_task_routing_successor("v16-routing-successor-other-key", &request)
			.await?,
		QuickTaskRoutingSuccessorOutcome::Rejected { .. }
	));

	let source_projection =
		store.read_ordinary_task_conversations(Some(&source_id), None, 2).await?;
	assert_eq!(
		source_projection,
		vec![OrdinaryTaskConversationProjection::RoutingSuccessorRedirect {
			source_conversation_id: source_id.clone(),
			source_revision: 2,
			successor_conversation_id: successor.successor_conversation_id.clone(),
			successor_conversation_revision: 1,
		}],
	);
	let successor_projection = store
		.read_ordinary_task_conversations(Some(&successor.successor_conversation_id), None, 2)
		.await?;
	assert!(matches!(
		successor_projection.as_slice(),
		[OrdinaryTaskConversationProjection::Current(readback)]
			if readback.conversation_revision == 1
				&& readback.runtime_session_id.is_none()
				&& readback.routing_decision_id.is_none()
				&& readback.pre_session_state == Some(OrdinaryTaskPreSessionState::RoutingPending)
	));
	let listed = store.read_ordinary_task_conversations(None, None, 65).await?;
	assert_eq!(
		listed
			.iter()
			.filter(|projection| matches!(
				projection,
				OrdinaryTaskConversationProjection::Current(readback)
					if readback.conversation_id == successor.successor_conversation_id
			))
			.count(),
		1,
	);
	assert!(!listed.iter().any(|projection| matches!(
		projection,
		OrdinaryTaskConversationProjection::Current(readback)
			if readback.conversation_id == source_id
	)));
	assert_eq!(
		store.read_quick_task_request(&successor.successor_conversation_id).await?,
		Some(decodex_postgres::QuickTaskRequest {
			message: "Create a fresh routing Conversation.".into(),
			working_directory: "/tmp".into(),
		}),
	);
	let relation_count: i64 = owner
		.query_one(
			"SELECT count(*) FROM decodex.conversation_routing_successors \
			 WHERE source_conversation_id=$1::text::uuid",
			&[&source_id.as_str()],
		)
		.await?
		.get(0);
	assert_eq!(relation_count, 1);
	set_account_registry_quota_usage(owner, SELECTED_ACCOUNT_ID, 25).await?;
	Ok(())
}

pub(super) async fn set_account_registry_quota_usage(
	owner: &Client,
	account_id: &str,
	used_percent: i32,
) -> Result<(), Box<dyn std::error::Error>> {
	for duration_minutes in [300_i32, 10_080_i32] {
		let updated = owner
			.execute(
				"WITH observed AS (SELECT (extract(epoch FROM \
				 pg_catalog.clock_timestamp())*1000000)::bigint AS micros) \
				 UPDATE decodex.account_quota_facts AS quota SET \
				 used_percent=$3,resets_at_micros=observed.micros+3600000000,\
				 error_code=NULL,observed_at_micros=observed.micros FROM observed \
				 WHERE quota.account_id=$1::text::uuid AND quota.duration_minutes=$2",
				&[&account_id, &duration_minutes, &used_percent],
			)
			.await?;
		assert_eq!(updated, 1);
	}
	Ok(())
}

async fn assert_conversation_split_routing_rejected(
	store: &PostgresStore,
	owner: &Client,
	setup: &RoutingContractSetup,
) -> Result<(), Box<dyn std::error::Error>> {
	let consumer = ExecutionConsumer::ConversationTurn {
		conversation_id: setup.selected_run.conversation_id.clone(),
		conversation_revision: 1,
		source_runtime_session_id: Some(setup.selected_run.runtime_session_id.clone()),
		source_runtime_session_revision: Some(1),
		turn_id: setup.selected_run.turn_id.clone(),
	};
	let request = RouteAccount { consumer: consumer.clone(), ..setup.selected_request.clone() };
	assert!(matches!(
		store.route_account("v16-conversation-split-decision", &request).await,
		Err(StoreError::InvalidInput(
			"split routing decisions are reserved for ManagedRun execution"
		))
	));
	assert!(matches!(
		store
			.resolve_routing_snapshot(
				"v16-conversation-split-snapshot",
				&setup.selected_request.routing_policy_id,
				setup.selected_request.expected_routing_policy_revision,
				&consumer,
			)
			.await,
		Err(StoreError::InvalidInput(
			"split routing snapshots are reserved for ManagedRun execution"
		))
	));
	assert_eq!(receipt_count(owner, "v16-conversation-split-decision").await?, 0);
	assert_eq!(receipt_count(owner, "v16-conversation-split-snapshot").await?, 0);
	Ok(())
}

async fn prepare_routing_contract(
	store: &PostgresStore,
	owner: &Client,
) -> Result<RoutingContractSetup, Box<dyn std::error::Error>> {
	create_project_and_policy(owner).await?;
	create_routing_accounts(owner).await?;
	prepare_routing_process_generations(store, owner).await?;
	insert_quota_pair(owner, SELECTED_ACCOUNT_ID, Some(73), Some(41), "selected").await?;
	insert_quota_pair(owner, WAITING_ACCOUNT_ID, Some(0), Some(0), "waiting").await?;
	insert_quota_pair(owner, NO_ROUTE_ACCOUNT_ID, Some(0), Some(0), "no-route").await?;
	align_tied_waiting_ready_time(owner).await?;

	let selected_run =
		create_run(store, owner, SELECTED_ACCOUNT_ID, "V16 account 16", 16, "usage").await?;
	let waiting_run =
		create_run(store, owner, WAITING_ACCOUNT_ID, "V16 account 17", 17, "usage").await?;
	let no_route_run =
		create_run(store, owner, NO_ROUTE_ACCOUNT_ID, "V16 account 18", 18, "usage").await?;
	let cancel_run =
		create_run(store, owner, NO_ROUTE_ACCOUNT_ID, "V16 account 18", 19, "usage").await?;
	let stale_run =
		create_run(store, owner, NO_ROUTE_ACCOUNT_ID, "V16 account 18", 20, "usage").await?;

	for (marker, account_id) in
		[(16_u8, SELECTED_ACCOUNT_ID), (17_u8, WAITING_ACCOUNT_ID), (18_u8, NO_ROUTE_ACCOUNT_ID)]
	{
		publish_evidence(store, marker, account_id).await?;
	}

	let selected_consumer = managed_consumer(&selected_run);
	let selected_request = create_policy_snapshot_and_request(
		store,
		owner,
		SELECTED_POLICY_ID,
		SELECTED_ACCOUNT_ID,
		selected_consumer,
		16,
		BUILD_ID,
	)
	.await?;
	let waiting_request = create_policy_snapshot_and_request(
		store,
		owner,
		WAITING_POLICY_ID,
		WAITING_ACCOUNT_ID,
		managed_consumer(&waiting_run),
		17,
		BUILD_ID,
	)
	.await?;
	let no_route_request = create_policy_snapshot_and_request(
		store,
		owner,
		NO_ROUTE_POLICY_ID,
		NO_ROUTE_ACCOUNT_ID,
		managed_consumer(&no_route_run),
		18,
		NO_ROUTE_BUILD_ID,
	)
	.await?;
	let cancel_request = create_policy_snapshot_and_request(
		store,
		owner,
		CANCEL_POLICY_ID,
		NO_ROUTE_ACCOUNT_ID,
		managed_consumer(&cancel_run),
		19,
		BUILD_ID,
	)
	.await?;
	let stale_request = create_policy_snapshot_and_request(
		store,
		owner,
		STALE_POLICY_ID,
		NO_ROUTE_ACCOUNT_ID,
		managed_consumer(&stale_run),
		20,
		BUILD_ID,
	)
	.await?;

	Ok(RoutingContractSetup {
		selected_run,
		selected_request,
		waiting_request,
		no_route_request,
		cancel_request,
		stale_request,
	})
}

#[expect(
	clippy::too_many_lines,
	reason = "the sequential authority fixture is one cohesive end-to-end contract"
)]
async fn prepare_quick_task_continuation_route(
	store: &PostgresStore,
	owner: &Client,
) -> Result<
	(QuickTaskContinuationBinding, ConversationId, RuntimeSessionId, i64),
	Box<dyn std::error::Error>,
> {
	for (account_id, used_percent) in [
		(SELECTED_ACCOUNT_ID, 25_i32),
		(WAITING_ACCOUNT_ID, 100_i32),
		(NO_ROUTE_ACCOUNT_ID, 100_i32),
	] {
		for duration_minutes in [300_i32, 10_080_i32] {
			owner
				.execute(
					"WITH observed AS (SELECT (extract(epoch FROM \
					 pg_catalog.clock_timestamp())*1000000)::bigint AS micros) \
					 INSERT INTO decodex.account_quota_facts(\
					 account_id,duration_minutes,used_percent,resets_at_micros,error_code,\
					 observed_at_micros) SELECT $1::text::uuid,$2,$3,\
					 observed.micros+3600000000,NULL,observed.micros FROM observed \
					 ON CONFLICT(account_id,duration_minutes) DO UPDATE SET \
					 used_percent=EXCLUDED.used_percent,\
					 resets_at_micros=EXCLUDED.resets_at_micros,error_code=NULL,\
					 observed_at_micros=EXCLUDED.observed_at_micros",
					&[&account_id, &duration_minutes, &used_percent],
				)
				.await?;
		}
	}

	let conversation_id = ConversationId::new(uuid(0xca, 16))?;
	store
		.create_quick_task_conversation(
			&CommandIdentity::new("v16-quick-task-conversation", b"v16")?,
			&CreateQuickTaskConversation {
				conversation_id: conversation_id.clone(),
				title: "V16 ordinary Quick Task".into(),
				message: "Exercise atomic Account Registry routing.".into(),
				working_directory: "/tmp".into(),
			},
		)
		.await?;
	let request = RouteQuickTaskInitial {
		conversation_id: conversation_id.clone(),
		expected_conversation_revision: 1,
	};
	let route = match store.route_quick_task_initial("v16-quick-task-route", &request).await? {
		QuickTaskInitialRouteOutcome::Fresh(route) => route,
		other => return Err(format!("initial Quick Task route was not fresh: {other:?}").into()),
	};
	assert_eq!(route.decision.kind, AccountRegistryRoutingDecisionKind::Selected);
	assert_eq!(
		route.decision.selected_account_id.as_ref(),
		Some(&AccountId::new(SELECTED_ACCOUNT_ID)?),
	);
	let route_bytes = receipt_bytes(owner, "v16-quick-task-route").await?;
	assert_eq!(
		store.route_quick_task_initial("v16-quick-task-route", &request).await?,
		QuickTaskInitialRouteOutcome::Replayed(route.clone()),
	);
	assert_eq!(receipt_bytes(owner, "v16-quick-task-route").await?, route_bytes);
	assert_eq!(store.read_quick_task_initial_route(&conversation_id).await?, Some(route.clone()));
	assert!(matches!(
		store.route_quick_task_initial("v16-quick-task-route-other-key", &request).await?,
		QuickTaskInitialRouteOutcome::Rejected(ref rejection)
			if rejection.code == "initial_routing_already_bound"
	));

	let initial_plan = match store
		.plan_initial_thread_continuation(
			"v16-quick-task-initial-plan",
			&PlanInitialThreadContinuation {
				operation_id: uuid(0xcb, 1),
				routing_decision_id: route.decision_id.clone(),
				expected_conversation_revision: 1,
				plan_id: uuid(0xcb, 2),
			},
		)
		.await?
	{
		ContinuationCommandOutcome::Success(plan) => plan,
		ContinuationCommandOutcome::Rejected(rejection) =>
			return Err(format!("initial Quick Task plan rejected: {rejection:?}").into()),
	};
	let (runtime_session_id, runtime_session_revision) =
		establish_quick_task_runtime_session(store, &route, &initial_plan).await?;

	let binding = match store
		.bind_quick_task_continuation(
			"v16-quick-task-continuation-1",
			&BindQuickTaskContinuation {
				operation_id: uuid(0xcf, 1),
				conversation_id: conversation_id.clone(),
				expected_conversation_revision: 1,
				source_runtime_session_id: runtime_session_id.clone(),
				expected_source_runtime_session_revision: runtime_session_revision,
				turn_id: TurnId::new(uuid(0xd0, 1))?,
			},
		)
		.await?
	{
		RoutingCommandOutcome::Success(binding) => binding,
		RoutingCommandOutcome::Rejected(rejection) =>
			return Err(format!("continuation binding rejected: {}", rejection.code).into()),
	};
	assert_eq!(binding.initial_decision_id, route.decision_id);
	let initial_session = initial_plan.runtime_session.as_ref().ok_or_else(|| {
		StoreError::Incompatible("initial Quick Task plan omitted its RuntimeSession".into())
	})?;
	assert_eq!(binding.account_snapshot_id, initial_session.account_snapshot.account_snapshot_id);
	assert_eq!(
		binding.account_snapshot_source_revision,
		initial_session.account_snapshot.source_revision,
	);
	assert_eq!(binding.profile_snapshot_id, initial_session.profile_snapshot.profile_snapshot_id);
	assert_eq!(
		binding.profile_snapshot_source_revision,
		initial_session.profile_snapshot.source_revision,
	);
	assert!(matches!(
		&binding.consumer,
		ExecutionConsumer::ConversationTurn {
			conversation_id: bound_conversation_id,
			conversation_revision: 1,
			source_runtime_session_id: Some(source_runtime_session_id),
			source_runtime_session_revision: Some(source_runtime_session_revision),
			turn_id,
		} if bound_conversation_id == &conversation_id
			&& source_runtime_session_id == &runtime_session_id
			&& *source_runtime_session_revision == runtime_session_revision
			&& turn_id.as_str() == uuid(0xd0, 1)
	));
	Ok((binding, conversation_id, runtime_session_id, runtime_session_revision))
}

async fn establish_quick_task_runtime_session(
	store: &PostgresStore,
	route: &QuickTaskInitialRoute,
	initial_plan: &ContinuationPlanEffect,
) -> Result<(RuntimeSessionId, i64), Box<dyn std::error::Error>> {
	let session = initial_plan.runtime_session.as_ref().ok_or_else(|| {
		StoreError::Incompatible("initial Quick Task plan omitted its RuntimeSession".into())
	})?;
	if initial_plan.plan.routing_decision_id != route.decision_id
		|| initial_plan.plan.consumer != route.consumer
		|| initial_plan.plan.source_runtime_session_id != session.runtime_session_id
		|| initial_plan.plan.source_runtime_session_revision != 1
		|| session.state != RuntimeSessionState::Starting
		|| session.revision != 1
		|| session.codex_thread_id.is_some()
	{
		return Err(StoreError::Incompatible(
			"initial Quick Task plan and RuntimeSession are cross-linked".into(),
		)
		.into());
	}

	let blob_store = isolated_blob_store()?;
	let admission = store
		.admit_initial_quick_task_turn(
			&blob_store,
			"v16-quick-task-initial-turn",
			&AdmitInitialQuickTaskTurn {
				expected_conversation_revision: 1,
				expected_runtime_session_revision: 1,
				continuation_plan_id: initial_plan.plan.plan_id.clone(),
				message: RecordHistoryItem {
					conversation_id: session.conversation_id.clone(),
					runtime_session_id: session.runtime_session_id.clone(),
					turn_id: route.turn_id.clone(),
					turn_sequence: 1,
					turn_role: TurnRole::User,
					possible_side_effects: PossibleSideEffects::Unknown,
					history_item_id: HistoryItemId::new(uuid(0xdd, 16))?,
					ordinal: 0,
					kind: HistoryItemKind::Message,
					status: ItemStatus::Completed,
					text: "Exercise atomic Account Registry routing.".into(),
					media_type: HistoryMediaType::new("text/markdown")?,
					metadata: HistoryMetadata::empty(),
					expected_revision: None,
					artifact: None,
				},
			},
		)
		.await?;
	let admitted = match admission {
		InitialQuickTaskTurnAdmissionOutcome::Fresh(admitted) => admitted,
		other => return Err(format!("initial Quick Task Turn was not fresh: {other:?}").into()),
	};
	assert_eq!(admitted.routing_decision_id, route.decision_id);
	assert_eq!(admitted.turn.turn_id, route.turn_id);
	assert_eq!(admitted.turn.revision, 1);

	let (generation_id, process_revision) =
		prepare_initial_quick_task_process_generation(store, route, initial_plan, session).await?;

	let thread_start_request_id = 1_276_016_i64;
	let thread_start_request_sha256 = "1".repeat(64);
	let fence_key = "v16-quick-task-thread-fence";
	let authority = match store
		.fence_runtime_session_thread_start(
			fence_key,
			&FenceRuntimeSessionThreadStart {
				conversation_id: session.conversation_id.clone(),
				expected_conversation_revision: 1,
				runtime_session_id: session.runtime_session_id.clone(),
				expected_revision: 1,
				turn_id: route.turn_id.clone(),
				expected_turn_revision: 1,
				continuation_plan_id: initial_plan.plan.plan_id.clone(),
				process_generation_id: generation_id,
				process_generation_revision: process_revision,
				process_execution_epoch_id: ProcessExecutionEpochId::new(
					PROCESS_EXECUTION_EPOCH_ID,
				)?,
				thread_start_request_id,
				thread_start_request_sha256: thread_start_request_sha256.clone(),
			},
		)
		.await?
	{
		FenceRuntimeSessionThreadStartOutcome::Fresh(authority) => authority,
		other => return Err(format!("RuntimeSession thread fence was not fresh: {other:?}").into()),
	};
	assert_eq!(authority.readback().prior_revision, 1);
	assert_eq!(authority.readback().revision, 2);
	let codex_thread_id = uuid(0xde, 16);
	let binding = authority.into_binding(SuccessfulRuntimeSessionThreadStart {
		response_id: thread_start_request_id,
		response_sha256: "2".repeat(64),
		codex_thread_id: codex_thread_id.clone(),
	});
	let bound =
		match store.bind_runtime_session_thread("v16-quick-task-thread-binding", &binding).await? {
			BindRuntimeSessionThreadOutcome::Applied(bound) => bound,
			other => return Err(format!("RuntimeSession thread binding failed: {other:?}").into()),
		};
	assert_eq!(bound.prior_revision, 2);
	assert_eq!(bound.revision, 3);
	assert_eq!(bound.codex_thread_id, codex_thread_id);
	Ok((session.runtime_session_id.clone(), bound.revision))
}

async fn prepare_initial_quick_task_process_generation(
	store: &PostgresStore,
	route: &QuickTaskInitialRoute,
	initial_plan: &ContinuationPlanEffect,
	session: &StoredRuntimeSession,
) -> Result<(ProcessGenerationId, i64), Box<dyn std::error::Error>> {
	retire_selected_routing_generation(store).await?;
	let generation_id = ProcessGenerationId::new(uuid(0xdc, 16))?;
	let process_admission = match store
		.prepare_quick_task_process_generation(
			"v16-quick-task-process-admission",
			&PrepareQuickTaskProcessGeneration {
				conversation_id: session.conversation_id.clone(),
				expected_conversation_revision: 1,
				runtime_session_id: session.runtime_session_id.clone(),
				expected_runtime_session_revision: 1,
				turn_id: route.turn_id.clone(),
				expected_turn_revision: 1,
				continuation_plan_id: initial_plan.plan.plan_id.clone(),
				routing_decision_id: route.decision_id.clone(),
				selected_account_id: session.account_snapshot.source_account_id.clone(),
				process_generation_id: generation_id.clone(),
			},
		)
		.await?
	{
		PrepareQuickTaskProcessGenerationOutcome::Fresh(admission) => admission,
		other =>
			return Err(format!("Quick Task process admission was not fresh: {other:?}").into()),
	};

	let boot_id = ProcessBootIdentity::new("xy-1276-quick-task-boot")?;
	let intent = ProcessGenerationIntent {
		generation_id: generation_id.clone(),
		account_id: session.account_snapshot.source_account_id.clone(),
		runner_identity: ProcessRunnerIdentity::new(format!(
			"sha256:{PROCESS_AUTHORIZATION_DIGEST}"
		))?,
		intended_boot_id: boot_id.clone(),
		control_kind: ProcessControlKind::StdioOnlyBestEffortEof,
		isolation_kind: ProcessIsolationKind::Session,
		execution_authorization: ProcessExecutionAuthorization::new(
			ProcessExecutionEpochId::new(PROCESS_EXECUTION_EPOCH_ID)?,
			PROCESS_AUTHORIZATION_DIGEST,
		)?,
	};
	let account_binding = ProcessGenerationAccountBinding::new(
		session.account_snapshot.source_revision,
		CredentialBinding {
			schema_version: CredentialStoreSchemaVersion::V1,
			version: CredentialVersion::new(1)?,
			fingerprint: CredentialFingerprint::new(PROCESS_CREDENTIAL_FINGERPRINT)?,
			provider: ProviderIdentity::new(AccountProvider::Chatgpt, provider_account_id(16))?,
			writer_operation_id: AccountOperationId::new(uuid(0xa6, 16))?,
		},
		PROCESS_CALLBACK_PROFILE,
	)?;
	let process_fence = match store
		.prepare_quick_task_bound_process_generation(&intent, &account_binding, process_admission)
		.await?
	{
		PrepareProcessGenerationOutcome::Fresh(fence) => fence,
		other =>
			return Err(format!("Quick Task ProcessGeneration was not fresh: {other:?}").into()),
	};
	assert_eq!(process_fence.revision(), 1);
	let process_id = 1_516_u32;
	let process_identity = ProcessIdentity::new(
		boot_id,
		process_id,
		ProcessStartIdentity::new("xy-1276-quick-task-process-start")?,
		process_id,
		process_id,
	)?;
	assert!(matches!(
		store.bind_process_generation_identity(&generation_id, 1, &process_identity).await?,
		ProcessGenerationMutationOutcome::Applied(ref mutation) if mutation.revision == 2
	));
	let process_revision = match store.mark_process_generation_ready(&generation_id, 2).await? {
		ProcessGenerationMutationOutcome::Applied(mutation) => mutation.revision,
		other => return Err(format!("Quick Task process readiness failed: {other:?}").into()),
	};
	assert_eq!(process_revision, 3);
	Ok((generation_id, process_revision))
}

async fn retire_selected_routing_generation(
	store: &PostgresStore,
) -> Result<(), Box<dyn std::error::Error>> {
	let generation_id = ProcessGenerationId::new(process_generation_id(16))?;
	let boot_id = ProcessBootIdentity::new("xy-1416-boot-16")?;
	let process_id = 1_416_u32;
	let process_identity = ProcessIdentity::new(
		boot_id.clone(),
		process_id,
		ProcessStartIdentity::new("xy-1416-process-start-16")?,
		process_id,
		process_id,
	)?;
	let evidence = ProcessDeathEvidence::new(
		ProcessDeathEvidenceId::new(uuid(0xdb, 16))?,
		generation_id,
		ProcessDeathEvidenceKind::OwnedChildExit,
		boot_id,
		Some(process_identity),
		"3".repeat(64),
	)?;
	assert!(matches!(
		store.record_process_generation_death(3, &evidence).await?,
		ProcessGenerationMutationOutcome::Applied(ref mutation)
			if mutation.state == decodex_core::ProcessGenerationState::Dead
				&& mutation.revision == 5
	));
	Ok(())
}

async fn create_routing_accounts(owner: &Client) -> Result<(), Box<dyn std::error::Error>> {
	owner.batch_execute("BEGIN; SELECT decodex.lock_account_routing_universe_exact()").await?;
	for (account_id, label) in [
		(SELECTED_ACCOUNT_ID, "V16 account 16"),
		(WAITING_ACCOUNT_ID, "V16 account 17"),
		(NO_ROUTE_ACCOUNT_ID, "V16 account 18"),
	] {
		let marker = account_marker(account_id)?;
		let provider_account_id = provider_account_id(marker);
		let writer_operation_id = uuid(0xa6, marker);
		owner
			.execute(
				"INSERT INTO decodex.accounts(\
				 account_id,display_label,state,enabled,provider_kind,provider_account_id,\
				 credential_store_schema_version,credential_version,credential_fingerprint,\
				 credential_writer_operation_id,credential_store_observation,\
				 credential_store_observed_at) \
				 VALUES($1::text::uuid,$2,'available',true,'chatgpt',$3,1,1,$4,\
				 $5::text::uuid,'exact',pg_catalog.clock_timestamp())",
				&[
					&account_id,
					&label,
					&provider_account_id,
					&PROCESS_CREDENTIAL_FINGERPRINT,
					&writer_operation_id,
				],
			)
			.await?;
		owner
			.execute(
				"INSERT INTO decodex.account_routing_order(account_id,position) \
				 SELECT $1::text::uuid,pg_catalog.count(*)::integer \
				 FROM decodex.account_routing_order",
				&[&account_id],
			)
			.await?;
	}
	owner
		.batch_execute(
			"UPDATE decodex.account_routing_control SET revision=revision+1,\
			 updated_at=pg_catalog.clock_timestamp() WHERE singleton; \
			 SELECT decodex.lock_account_routing_universe_exact(); COMMIT",
		)
		.await?;
	Ok(())
}

async fn assert_rolled_back_routing_decision(
	owner: &Client,
	selected_request: &RouteAccount,
) -> Result<(), Box<dyn std::error::Error>> {
	let ExecutionConsumer::ManagedRunExecution {
		managed_run_id,
		managed_run_revision,
		execution_id,
	} = &selected_request.consumer
	else {
		return Err("selected V16 fixture is not a ManagedRun execution".into());
	};
	owner.batch_execute("BEGIN").await?;
	let rolled_back: Vec<u8> = owner
		.query_one(
			"SELECT decodex.route_account_exact('decodex/exact-command/1',\
			 'v16-rollback',$1::text::uuid,'managed_run_project_policy',\
			 $2::text::uuid,1,NULL,'managed_run_execution',\
			 NULL,NULL,NULL,NULL,NULL,$3::text::uuid,$4,$5::text::uuid)",
			&[
				&uuid(0xb6, 1),
				&SELECTED_POLICY_ID,
				&managed_run_id.as_str(),
				managed_run_revision,
				&execution_id.as_str(),
			],
		)
		.await?
		.get(0);
	owner.batch_execute("ROLLBACK").await?;
	assert!(!rolled_back.is_empty());
	assert_eq!(receipt_count(owner, "v16-rollback").await?, 0);
	Ok(())
}

async fn assert_selected_routing_decision(
	store: &PostgresStore,
	owner: &Client,
	selected_request: &RouteAccount,
) -> Result<PersistedRoutingDecision, Box<dyn std::error::Error>> {
	let selected = success(store.route_account("v16-selected", selected_request).await?)?;
	assert_eq!(
		selected.decision.kind,
		RoutingDecisionKind::Selected,
		"unexpected selected decision: {selected:#?}",
	);
	assert_eq!(
		selected.decision.selected_account_id.as_ref(),
		Some(&AccountId::new(SELECTED_ACCOUNT_ID)?),
	);
	let closed_selected = owner
		.query_one(
			concat!(
				"SELECT pg_catalog.count(*)=(SELECT pg_catalog.count(*) ",
				"FROM decodex.routing_snapshot_members AS expected ",
				"WHERE expected.snapshot_id=member.snapshot_id),",
				"pg_catalog.count(*) FILTER (WHERE snapshot.disposition='included')=1 ",
				"AND pg_catalog.count(*) FILTER (WHERE snapshot.disposition='included' ",
				"AND member.account_id=$2::text::uuid)=1,",
				"(SELECT count(*) FROM decodex.routing_decision_quota_refs AS quota ",
				"WHERE quota.decision_id=member.decision_id)=pg_catalog.count(*)*2,",
				"(SELECT count(*) FROM decodex.routing_decision_capability_refs AS capability ",
				"WHERE capability.decision_id=member.decision_id)=pg_catalog.count(*)*8 ",
				"FROM decodex.routing_decision_member_refs AS member ",
				"JOIN decodex.routing_snapshot_members AS snapshot ",
				"ON snapshot.snapshot_id=member.snapshot_id ",
				"AND snapshot.account_id=member.account_id AND snapshot.position=member.position ",
				"WHERE member.decision_id=$1::text::uuid ",
				"GROUP BY member.decision_id,member.snapshot_id",
			),
			&[&selected.decision_id, &SELECTED_ACCOUNT_ID],
		)
		.await?;
	for index in 0..4 {
		assert!(closed_selected.get::<_, bool>(index), "closed V16 selected evidence {index}");
	}
	let selected_bytes = receipt_bytes(owner, "v16-selected").await?;
	assert_eq!(
		store.route_account("v16-selected", selected_request).await?,
		RoutingCommandOutcome::Success(selected.clone()),
	);
	assert_eq!(receipt_bytes(owner, "v16-selected").await?, selected_bytes);
	let mut changed = selected_request.clone();
	changed.routing_policy_id = WAITING_POLICY_ID.to_owned();
	assert!(matches!(
		store.route_account("v16-selected", &changed).await,
		Err(StoreError::IdempotencyConflict)
	));
	let alias_error = store
		.route_account("v16-cross-key-alias", selected_request)
		.await
		.expect_err("one routing operation cannot alias a second exact key");
	assert!(matches!(
		alias_error,
		StoreError::Database(ref error)
			if error.code() == Some(&tokio_postgres::error::SqlState::UNIQUE_VIOLATION)
				&& error.as_db_error().and_then(tokio_postgres::error::DbError::constraint)
					== Some("routing_decisions_operation_id_key")
	));
	assert_eq!(receipt_count(owner, "v16-selected").await?, 1);
	assert_eq!(receipt_count(owner, "v16-cross-key-alias").await?, 0);
	let decision_counts = owner
		.query_one(
			"SELECT count(*),count(*) FILTER (WHERE operation_id=$1::text::uuid) \
			 FROM decodex.routing_decisions",
			&[&selected_request.operation_id],
		)
		.await?;
	assert_eq!(decision_counts.get::<_, i64>(0), 1);
	assert_eq!(decision_counts.get::<_, i64>(1), 1);
	Ok(selected)
}

async fn assert_alternate_routing_decisions(
	store: &PostgresStore,
	waiting_request: &RouteAccount,
	no_route_request: &RouteAccount,
	cancel_request: &RouteAccount,
	stale_request: &RouteAccount,
) -> Result<
	(PersistedRoutingDecision, PersistedRoutingDecision, PersistedRoutingDecision),
	Box<dyn std::error::Error>,
> {
	let waiting = success(store.route_account("v16-waiting", waiting_request).await?)?;
	assert_eq!(waiting.decision.kind, RoutingDecisionKind::WaitingUsage);
	assert_eq!(waiting.decision.exclusions.len(), 2);
	assert_ne!(
		waiting.decision.exclusions[0].duration_minutes,
		waiting.decision.exclusions[1].duration_minutes,
	);
	assert_ne!(
		waiting.decision.exclusions[0].observed_at_provenance.source_id,
		waiting.decision.exclusions[1].observed_at_provenance.source_id,
	);
	assert!(waiting.decision.ready_at_micros.is_some());

	let no_route = success(store.route_account("v16-no-route", no_route_request).await?)?;
	assert_eq!(no_route.decision.kind, RoutingDecisionKind::NoRoute);
	assert!(no_route.decision.exclusions.is_empty());
	let stale_consumer = match &no_route_request.consumer {
		ExecutionConsumer::ManagedRunExecution { managed_run_id, execution_id, .. } =>
			ExecutionConsumer::ManagedRunExecution {
				managed_run_id: managed_run_id.clone(),
				managed_run_revision: 2,
				execution_id: execution_id.clone(),
			},
		ExecutionConsumer::ConversationTurn { .. } => {
			return Err("V16 ManagedRun fixture has an ordinary consumer".into());
		},
	};
	let stale = store
		.route_account(
			"v16-stale-lineage",
			&RouteAccount { consumer: stale_consumer, ..no_route_request.clone() },
		)
		.await?;
	assert!(matches!(stale, RoutingCommandOutcome::Rejected(ref rejection)
		if rejection.code == "stale_consumer"));
	let cancel_waiting = success(store.route_account("v16-cancel-waiting", cancel_request).await?)?;
	let stale_waiting = success(store.route_account("v16-stale-waiting", stale_request).await?)?;
	assert_eq!(cancel_waiting.decision.kind, RoutingDecisionKind::WaitingUsage);
	assert_eq!(stale_waiting.decision.kind, RoutingDecisionKind::WaitingUsage);
	Ok((waiting, cancel_waiting, stale_waiting))
}

async fn assert_concurrent_routing_replay(
	store: &PostgresStore,
	owner: &Client,
	selected_request: &RouteAccount,
) -> Result<(), Box<dyn std::error::Error>> {
	let race_request = RouteAccount { operation_id: uuid(0xb6, 2), ..selected_request.clone() };
	let mut racers = JoinSet::new();
	for _ in 0..2 {
		let store = store.clone();
		let request = race_request.clone();
		racers.spawn(async move { store.route_account("v16-concurrent-replay", &request).await });
	}
	let mut race_result = None;
	while let Some(result) = racers.join_next().await {
		let current = result??;
		if let Some(expected) = &race_result {
			assert_eq!(&current, expected);
		} else {
			race_result = Some(current);
		}
	}
	assert_eq!(receipt_count(owner, "v16-concurrent-replay").await?, 1);
	Ok(())
}

async fn create_project_and_policy(owner: &Client) -> Result<(), Box<dyn std::error::Error>> {
	owner
		.query_one(
			"SELECT project_id FROM decodex.create_project(\
			 $1::text::decodex.canonical_uuid_v4_text,$2,$3,$3,'{}'::jsonb,\
			 $4::text::decodex.canonical_uuid_v4_text)",
			&[&PROJECT_ID, &"vnext/postgres-acceptance", &"/srv/vnext-acceptance", &LEAD_ID],
		)
		.await?;
	owner
		.query_one(
			"SELECT policy_id FROM decodex.create_policy(\
			 $1::text::decodex.canonical_uuid_v4_text,\
			 $2::text::decodex.canonical_uuid_v4_text)",
			&[&ACCEPTED_POLICY_ID, &PROJECT_ID],
		)
		.await?;
	owner
		.query_one(
			"SELECT revision_accepted FROM decodex.accept_policy_revision(\
			 $1::text::decodex.canonical_uuid_v4_text,\
			 $2::text::decodex.canonical_uuid_v4_text,1,'vNext acceptance',\
			 '{\"routing\":\"disabled\"}'::jsonb,\
			 $3::text::decodex.canonical_uuid_v4_text,NULL)",
			&[&ACCEPTED_POLICY_ID, &PROJECT_ID, &LEAD_ID],
		)
		.await?;
	Ok(())
}

async fn insert_quota_pair(
	owner: &Client,
	account_id: &str,
	five_hour: Option<i16>,
	seven_day: Option<i16>,
	marker: &str,
) -> Result<(), Box<dyn std::error::Error>> {
	for (window, duration, remaining) in
		[("five_hour", 300_i16, five_hour), ("seven_day", 10_080_i16, seven_day)]
	{
		owner
			.execute(
				r#"WITH fact AS (
				 SELECT pg_catalog.clock_timestamp() AS observed_at,
				 pg_catalog.clock_timestamp()+CASE WHEN $2::smallint=300
				  THEN interval '5 hours' ELSE interval '7 days' END AS resets_at
				), encoded AS (
				 SELECT *, (extract(epoch FROM observed_at)*1000000)::bigint AS observed,
				 (extract(epoch FROM resets_at)*1000000)::bigint AS resets FROM fact
				)
				INSERT INTO decodex.quota_windows(account_id,window_class,duration_minutes,
				 remaining_percent,resets_at,observed_at,confidence,metadata,revision)
				SELECT $1::text::uuid,$3::text::decodex.quota_window_class,$2,$4,resets_at,observed_at,
				 CASE WHEN $4::smallint IS NULL THEN 'unknown' ELSE 'high' END::decodex.observation_confidence,
				 pg_catalog.jsonb_build_object('timestamp_precision','unix_microsecond',
				  'evidence_revision','1','source_id',$5||'/'||$3,
				  'raw_observed_at',observed::text,'raw_resets_at',resets::text),1 FROM encoded"#,
				&[&account_id, &duration, &window, &remaining, &marker],
			)
			.await?;
	}
	Ok(())
}

async fn align_tied_waiting_ready_time(owner: &Client) -> Result<(), Box<dyn std::error::Error>> {
	let updated = owner
		.execute(
			"UPDATE decodex.quota_windows AS target SET \
			 resets_at=source.resets_at+INTERVAL '1 microsecond',\
			 updated_at=pg_catalog.clock_timestamp(),metadata=pg_catalog.jsonb_set(\
			 target.metadata,'{raw_resets_at}',pg_catalog.to_jsonb(((extract(\
			 epoch FROM source.resets_at+INTERVAL '1 microsecond')*1000000)::bigint)::text)) \
			 FROM decodex.quota_windows AS source WHERE target.account_id=$1::text::uuid \
			 AND source.account_id=$2::text::uuid AND target.window_class=source.window_class \
			 AND target.duration_minutes=source.duration_minutes",
			&[&NO_ROUTE_ACCOUNT_ID, &WAITING_ACCOUNT_ID],
		)
		.await?;
	assert_eq!(updated, 2);
	Ok(())
}

async fn create_run(
	store: &PostgresStore,
	owner: &Client,
	account_id: &str,
	account_display_label: &str,
	marker: u8,
	wait_reason: &str,
) -> Result<RunFixture, Box<dyn std::error::Error>> {
	let conversation_id = ConversationId::new(uuid(0xc1, marker))?;
	store
		.create_conversation(
			&CommandIdentity::new(format!("v16-conversation-{marker}"), b"v16")?,
			&CreateConversation {
				conversation_id: conversation_id.clone(),
				title: format!("V16 acceptance {marker}"),
			},
		)
		.await?;
	let runtime_session_id = RuntimeSessionId::new(uuid(0xc2, marker))?;
	let outcome = store
		.create_runtime_session(
			&format!("v16-runtime-session-{marker}"),
			&CreateRuntimeSession {
				runtime_session_id: runtime_session_id.clone(),
				conversation_id: conversation_id.clone(),
				role: RoleProfileRole::Task,
				account_snapshot: CreateRuntimeSessionAccountSnapshot {
					account_snapshot_id: uuid(0xc4, marker),
					source_account_id: AccountId::new(account_id)?,
					display_label: account_display_label.to_owned(),
					observed_state: AccountState::Available,
					source_revision: 1,
				},
				codex_thread_id: None,
				initial_state: RuntimeSessionState::Starting,
			},
		)
		.await?;
	assert!(matches!(outcome, RuntimeSessionCommandOutcome::Success(_)));
	let work_item_id = uuid(0xc5, marker);
	let managed_run_id = ManagedRunId::new(uuid(0xc6, marker))?;
	let execution_id = ManagedExecutionId::new(uuid(0xc8, marker))?;
	let initial_work_item = owner
		.query_one(
			r#"WITH operation AS (SELECT pg_catalog.clock_timestamp() AS operation_time)
			INSERT INTO decodex.work_items(work_item_id,project_id,lead_agent_id,title,
			description,priority,acceptance_criteria,validation_criteria,state,revision,
			last_changed_by,last_correlation_id,last_provenance,created_at,updated_at)
			SELECT $1::text::uuid,$2::text::uuid,$3::text::uuid,$4,'acceptance fixture','high',
			ARRAY['routing authority is exact'],ARRAY['unified PostgreSQL gate'],'inbox',1,
			$3::text::uuid,$5::text::uuid,'vNext PostgreSQL acceptance',operation_time,
			operation_time FROM operation RETURNING state::text,revision,created_at=updated_at"#,
			&[
				&work_item_id,
				&PROJECT_ID,
				&LEAD_ID,
				&format!("V16 WorkItem {marker}"),
				&uuid(0xc7, marker),
			],
		)
		.await?;
	assert_eq!(
		(
			initial_work_item.get::<_, String>(0),
			initial_work_item.get::<_, i64>(1),
			initial_work_item.get::<_, bool>(2),
		),
		("inbox".to_owned(), 1, true),
	);
	let initial_managed_run = owner
		.query_one(
			"WITH operation AS (SELECT pg_catalog.clock_timestamp() AS operation_time) \
			 INSERT INTO decodex.managed_runs(managed_run_id,project_id,work_item_id,\
			 runtime_session_id,runtime_session_revision,phase,lifecycle,wait_reason,blocked,\
			 revision,created_at,updated_at) SELECT $1::text::uuid,$2::text::uuid,$3::text::uuid,\
			 $4::text::uuid,1,'execute','waiting',$5::text::decodex.managed_run_wait_reason,true,\
			 1,operation_time,operation_time \
			 FROM operation RETURNING lifecycle::text,blocked,revision,created_at=updated_at",
			&[
				&managed_run_id.as_str(),
				&PROJECT_ID,
				&work_item_id,
				&runtime_session_id.as_str(),
				&wait_reason,
			],
		)
		.await?;
	assert_eq!(
		(
			initial_managed_run.get::<_, String>(0),
			initial_managed_run.get::<_, bool>(1),
			initial_managed_run.get::<_, i64>(2),
			initial_managed_run.get::<_, bool>(3),
		),
		("waiting".to_owned(), true, 1, true),
	);
	owner
		.execute(
			"INSERT INTO decodex.managed_run_assignments(\
			 managed_run_id,project_id,runtime_session_id,role)\
			 VALUES($1::text::uuid,$2::text::uuid,$3::text::uuid,'task')",
			&[&managed_run_id.as_str(), &PROJECT_ID, &runtime_session_id.as_str()],
		)
		.await?;
	let turn_id = TurnId::new(uuid(0xc9, marker))?;
	Ok(RunFixture { conversation_id, managed_run_id, execution_id, runtime_session_id, turn_id })
}

fn managed_consumer(run: &RunFixture) -> ExecutionConsumer {
	ExecutionConsumer::ManagedRunExecution {
		managed_run_id: run.managed_run_id.clone(),
		managed_run_revision: 1,
		execution_id: run.execution_id.clone(),
	}
}

async fn prepare_routing_process_generations(
	store: &PostgresStore,
	owner: &Client,
) -> Result<(), Box<dyn std::error::Error>> {
	assert!(
		store
			.attest_codex_account_capability(&decodex_postgres::CodexAccountCapabilityAttestation {
				build_identity: "sha256:fixture-current-codex".to_owned(),
				executable_sha256: CURRENT_CODEX_EXECUTABLE_SHA256.to_owned(),
				schema_sha256: SCHEMA_FINGERPRINT.to_owned(),
				callback_profile_sha256: PROCESS_CALLBACK_PROFILE.to_owned(),
				login_chatgpt_auth_tokens: true,
				refresh_callback: true,
			},)
			.await?
	);
	owner
		.execute(
			"INSERT INTO decodex.process_generation_execution_epochs(\
			 execution_epoch_id,authorization_digest,authorized_at)\
			 VALUES($1::text::uuid,$2,pg_catalog.clock_timestamp())",
			&[&PROCESS_EXECUTION_EPOCH_ID, &PROCESS_AUTHORIZATION_DIGEST],
		)
		.await?;

	for (account_id, marker) in
		[(SELECTED_ACCOUNT_ID, 16_u8), (WAITING_ACCOUNT_ID, 17_u8), (NO_ROUTE_ACCOUNT_ID, 18_u8)]
	{
		let generation_id = ProcessGenerationId::new(process_generation_id(marker))?;
		let boot_id = ProcessBootIdentity::new(format!("xy-1416-boot-{marker}"))?;
		let authorization = ProcessExecutionAuthorization::new(
			ProcessExecutionEpochId::new(PROCESS_EXECUTION_EPOCH_ID)?,
			PROCESS_AUTHORIZATION_DIGEST,
		)?;
		let intent = ProcessGenerationIntent {
			generation_id: generation_id.clone(),
			account_id: AccountId::new(account_id)?,
			runner_identity: ProcessRunnerIdentity::new(format!(
				"sha256:{PROCESS_AUTHORIZATION_DIGEST}"
			))?,
			intended_boot_id: boot_id.clone(),
			control_kind: ProcessControlKind::StdioOnlyBestEffortEof,
			isolation_kind: ProcessIsolationKind::Session,
			execution_authorization: authorization,
		};
		let binding = ProcessGenerationAccountBinding::new(
			1,
			CredentialBinding {
				schema_version: CredentialStoreSchemaVersion::V1,
				version: CredentialVersion::new(1)?,
				fingerprint: CredentialFingerprint::new(PROCESS_CREDENTIAL_FINGERPRINT)?,
				provider: ProviderIdentity::new(
					AccountProvider::Chatgpt,
					provider_account_id(marker),
				)?,
				writer_operation_id: AccountOperationId::new(uuid(0xa6, marker))?,
			},
			PROCESS_CALLBACK_PROFILE,
		)?;
		let fence = match store.prepare_bound_process_generation(&intent, &binding).await? {
			PrepareProcessGenerationOutcome::Fresh(fence) => fence,
			other => {
				return Err(
					format!("routing ProcessGeneration preparation failed: {other:?}").into()
				);
			},
		};
		assert_eq!(fence.revision(), 1);
		let process_id = 1_400_u32 + u32::from(marker);
		let identity = ProcessIdentity::new(
			boot_id,
			process_id,
			ProcessStartIdentity::new(format!("xy-1416-process-start-{marker}"))?,
			process_id,
			process_id,
		)?;
		let bound_revision = match store
			.bind_process_generation_identity(&generation_id, 1, &identity)
			.await?
		{
			ProcessGenerationMutationOutcome::Applied(mutation) => mutation.revision,
			other => {
				return Err(format!("routing process identity was not applied: {other:?}").into());
			},
		};
		assert_eq!(bound_revision, 2);
		let ready_revision = match store.mark_process_generation_ready(&generation_id, 2).await? {
			ProcessGenerationMutationOutcome::Applied(mutation) => mutation.revision,
			other => {
				return Err(format!("routing process readiness was not applied: {other:?}").into());
			},
		};
		assert_eq!(ready_revision, 3);
	}
	Ok(())
}

async fn publish_evidence(
	store: &PostgresStore,
	marker: u8,
	account_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
	let outcome = store
		.publish_routing_evidence(
			&format!("v16-evidence-{marker}"),
			&PublishRoutingEvidence {
				evidence_id: uuid(0xd1, marker),
				account_id: AccountId::new(account_id)?,
				expected_account_revision: 1,
				expected_evidence_revision: None,
				role: RoleProfileRole::Task,
				role_profile_revision: 1,
				build_id: BUILD_ID.to_owned(),
				process_id: uuid(0xd2, marker),
				process_account_id: AccountId::new(account_id)?,
				schema_fingerprint: SCHEMA_FINGERPRINT.to_owned(),
				capabilities: CodexCapability::ALL
					.into_iter()
					.map(|capability| (capability, RoutingCapabilityState::Supported))
					.collect(),
			},
		)
		.await?;
	assert!(matches!(outcome, RoutingCommandOutcome::Success(_)));
	Ok(())
}

async fn create_policy_snapshot_and_request(
	store: &PostgresStore,
	owner: &Client,
	routing_policy_id: &str,
	included_account_id: &str,
	consumer: ExecutionConsumer,
	marker: u8,
	required_build_id: &str,
) -> Result<RouteAccount, Box<dyn std::error::Error>> {
	let rows = owner
		.query("SELECT account_id::text,revision FROM decodex.accounts ORDER BY account_id", &[])
		.await?;
	let mut members = Vec::with_capacity(rows.len());
	for row in rows {
		let account_id = AccountId::new(row.get::<_, String>(0))?;
		let disposition = if account_id.as_str() == included_account_id {
			RoutingMemberDisposition::Included
		} else {
			RoutingMemberDisposition::Excluded
		};
		members.push(RoutingPolicyMemberInput {
			account_id,
			account_revision: row.get(1),
			disposition,
		});
	}
	let policy = store
		.replace_routing_policy(
			&format!("v16-policy-{marker}"),
			&ReplaceRoutingPolicy {
				routing_policy_id: routing_policy_id.to_owned(),
				project_id: PROJECT_ID.to_owned(),
				expected_revision: None,
				accepted_policy_id: ACCEPTED_POLICY_ID.to_owned(),
				accepted_policy_revision: 1,
				required_role: RoleProfileRole::Task,
				required_role_profile_revision: 1,
				required_build_id: required_build_id.to_owned(),
				members,
				required_capabilities: vec![
					CodexCapability::Initialize,
					CodexCapability::AccountRead,
					CodexCapability::ThreadRead,
					CodexCapability::PaginatedHistory,
				],
			},
		)
		.await?;
	assert!(matches!(policy, RoutingCommandOutcome::Success(_)));
	let snapshot = store
		.resolve_routing_snapshot(
			&format!("v16-snapshot-{marker}"),
			routing_policy_id,
			1,
			&consumer,
		)
		.await?;
	assert!(
		matches!(&snapshot, RoutingCommandOutcome::Success(_)),
		"unexpected routing snapshot outcome: {snapshot:?}",
	);
	Ok(RouteAccount {
		operation_id: uuid(0xe1, marker),
		routing_policy_id: routing_policy_id.to_owned(),
		expected_routing_policy_revision: 1,
		consumer,
	})
}

pub(super) async fn advance_stale_policy(
	store: &PostgresStore,
	owner: &Client,
	routing_policy_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
	let rows = owner
		.query("SELECT account_id::text,revision FROM decodex.accounts ORDER BY account_id", &[])
		.await?;
	let mut members = Vec::with_capacity(rows.len());
	for row in rows {
		let account_id = AccountId::new(row.get::<_, String>(0))?;
		let disposition = if account_id.as_str() == WAITING_ACCOUNT_ID {
			RoutingMemberDisposition::Included
		} else {
			RoutingMemberDisposition::Excluded
		};
		members.push(RoutingPolicyMemberInput {
			account_id,
			account_revision: row.get(1),
			disposition,
		});
	}
	let outcome = store
		.replace_routing_policy(
			"v18-stale-policy-advance",
			&ReplaceRoutingPolicy {
				routing_policy_id: routing_policy_id.to_owned(),
				project_id: PROJECT_ID.to_owned(),
				expected_revision: Some(1),
				accepted_policy_id: ACCEPTED_POLICY_ID.to_owned(),
				accepted_policy_revision: 1,
				required_role: RoleProfileRole::Task,
				required_role_profile_revision: 1,
				required_build_id: BUILD_ID.to_owned(),
				members,
				required_capabilities: vec![
					CodexCapability::Initialize,
					CodexCapability::AccountRead,
					CodexCapability::ThreadRead,
					CodexCapability::PaginatedHistory,
				],
			},
		)
		.await?;
	assert!(matches!(outcome, RoutingCommandOutcome::Success(_)));
	Ok(())
}

fn success<T>(outcome: RoutingCommandOutcome<T>) -> Result<T, Box<dyn std::error::Error>> {
	match outcome {
		RoutingCommandOutcome::Success(value) => Ok(value),
		RoutingCommandOutcome::Rejected(rejection) =>
			Err(format!("routing command rejected: {}", rejection.code).into()),
	}
}

async fn receipt_count(client: &Client, key: &str) -> Result<i64, tokio_postgres::Error> {
	Ok(client
		.query_one(
			"SELECT count(*) FROM decodex.exact_command_receipts WHERE idempotency_key=$1",
			&[&key],
		)
		.await?
		.get(0))
}

async fn receipt_bytes(client: &Client, key: &str) -> Result<Vec<u8>, tokio_postgres::Error> {
	Ok(client
		.query_one(
			"SELECT response_bytes FROM decodex.exact_command_receipts WHERE idempotency_key=$1",
			&[&key],
		)
		.await?
		.get(0))
}

fn uuid(prefix: u8, marker: u8) -> String {
	format!("{prefix:02x}000000-0000-4000-8000-{marker:012}")
}

fn account_marker(account_id: &str) -> Result<u8, Box<dyn std::error::Error>> {
	match account_id {
		SELECTED_ACCOUNT_ID => Ok(16),
		WAITING_ACCOUNT_ID => Ok(17),
		NO_ROUTE_ACCOUNT_ID => Ok(18),
		_ => Err("routing fixture account is unknown".into()),
	}
}

fn provider_account_id(marker: u8) -> String {
	format!("v16-provider-{marker}")
}

fn process_generation_id(marker: u8) -> String {
	format!("d2000000-0000-4000-8000-{:012}", 1_400_u16 + u16::from(marker))
}

pub(super) async fn assert_restored_routing_contract(
	client: &Client,
) -> Result<(), Box<dyn std::error::Error>> {
	let row = client
		.query_one(
			concat!(
				"SELECT (SELECT count(*) FROM decodex.routing_decisions WHERE kind='selected')=6,",
				"(SELECT count(*) FROM decodex.routing_decisions WHERE kind='waiting_usage')=3,",
				"(SELECT count(*) FROM decodex.routing_decisions WHERE kind='no_route')=1,",
				"(SELECT count(DISTINCT duration_minutes) FROM decodex.routing_decision_exclusions ",
				"WHERE decision_id=(SELECT decision_id FROM decodex.routing_decisions ",
				"WHERE operation_id=$1::text::uuid))=2",
			),
			&[&uuid(0xe1, 17)],
		)
		.await?;
	for index in 0..4 {
		assert!(row.get::<_, bool>(index), "restored V16 assertion {index}");
	}
	Ok(())
}
