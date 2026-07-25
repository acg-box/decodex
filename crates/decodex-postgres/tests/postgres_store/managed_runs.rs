use std::time::Duration;

use tokio_postgres::{Client, Config, NoTls, error::SqlState};

use super::{continuation, expected_peer_uid, routing_decision, separated_configs};
use decodex_core::{
	ConversationId, ExecutionAssignmentRole, ManagedExecutionId, ManagedRunId, ManagedRunPhase,
	ManagedRunState, ManagedRunWaitReason, ProcessBootIdentity, ProcessControlKind,
	ProcessExecutionAuthorization, ProcessExecutionEpochId, ProcessGenerationId,
	ProcessGenerationIntent, ProcessIdentity, ProcessIsolationKind, ProcessRunnerIdentity,
	ProcessStartIdentity, ProjectId, ProviderAttemptConsumer, ProviderAttemptId,
	ProviderAttemptPreparation, ProviderAttemptState, ProviderDuplicateRisk, ProviderEvidenceId,
	ProviderEvidenceSource, ProviderPositiveEvidence, ProviderRequestId, ProviderRequestKey,
	ProviderRequestKeys, ProviderTerminalOutcome, RuntimeSessionId, RuntimeSessionState,
};
use decodex_postgres::{
	AccountId, AccountState, AuthorizeProviderDispatchOutcome, BootstrapRoleProfiles,
	CommandIdentity, CreateConversation, CreateRuntimeSession, CreateRuntimeSessionAccountSnapshot,
	OutboxReconciliation, PostgresStore, PrepareProcessGenerationOutcome,
	PrepareProviderAttemptOutcome, ProcessGenerationMutationOutcome, ProviderAttemptMutationOutcome,
	ReconciliationOutcome, RoleProfileCommandOutcome, RoleProfileConfiguration, RoleProfileRole,
	RuntimeSessionCommandOutcome, StoreError,
};

const PROJECT_ID: &str = "a1000000-0000-4000-8000-000000000016";
const SELECTED_MANAGED_RUN_ID: &str = "c6000000-0000-4000-8000-000000000016";
const SELECTED_RUNTIME_SESSION_ID: &str = "c2000000-0000-4000-8000-000000000016";
const SELECTED_EXECUTION_ID: &str = "c8000000-0000-4000-8000-000000000016";
const EXECUTION_EPOCH_ID: &str = "d1000000-0000-4000-8000-000000001416";
const PROCESS_GENERATION_ID: &str = "d2000000-0000-4000-8000-000000001416";
const PROVIDER_ATTEMPT_ID: &str = "d3000000-0000-4000-8000-000000001416";
const PROVIDER_REQUEST_ID: &str = "d4000000-0000-4000-8000-000000001416";
const PROVIDER_EVIDENCE_ID: &str = "d5000000-0000-4000-8000-000000001416";
const OUTBOX_WORKER_ID: &str = "d6000000-0000-4000-8000-000000001416";
const AUTHORIZATION_DIGEST: &str =
	"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const REQUEST_DIGEST: &str =
	"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const WITNESS_DIGEST: &str =
	"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

fn profile(role: &str) -> RoleProfileConfiguration {
	RoleProfileConfiguration {
		model: format!("gpt-5.6-{role}"),
		reasoning_effort: "medium".into(),
		service_tier: "priority".into(),
		instructions: format!("Inert XY-1416 {role} fixture."),
		provenance: Some("XY-1416 V26 synthetic fixture".into()),
	}
}

fn profiles() -> BootstrapRoleProfiles {
	BootstrapRoleProfiles {
		advisor: profile("advisor"),
		lead: profile("lead"),
		task: profile("task"),
		reviewer: profile("reviewer"),
	}
}

async fn create_lead_session(
	store: &PostgresStore,
	account_id: &AccountId,
) -> Result<RuntimeSessionId, Box<dyn std::error::Error>> {
	let conversation_id =
		ConversationId::new("d7000000-0000-4000-8000-000000001416")?;
	store
		.create_conversation(
			&CommandIdentity::new("xy-1416-lead-conversation", b"xy-1416")?,
			&CreateConversation {
				conversation_id: conversation_id.clone(),
				title: "XY-1416 Lead assignment scope".into(),
			},
		)
		.await?;
	let runtime_session_id =
		RuntimeSessionId::new("d8000000-0000-4000-8000-000000001416")?;
	let outcome = store
		.create_runtime_session(
			"xy-1416-lead-session",
			&CreateRuntimeSession {
				runtime_session_id: runtime_session_id.clone(),
				conversation_id,
				role: RoleProfileRole::Lead,
				account_snapshot: CreateRuntimeSessionAccountSnapshot {
					account_snapshot_id: "d9000000-0000-4000-8000-000000001416".into(),
					source_account_id: account_id.clone(),
					display_label: "XY-1416 Lead account".into(),
					observed_state: AccountState::Available,
					source_revision: 1,
				},
				codex_thread_id: Some("da000000-0000-4000-8000-000000001416".into()),
				initial_state: RuntimeSessionState::Active,
			},
		)
		.await?;
	assert!(matches!(outcome, RuntimeSessionCommandOutcome::Success(_)));
	Ok(runtime_session_id)
}

async fn assert_assignment_scope(
	owner: &Client,
	managed_run_id: &ManagedRunId,
	lead_session_id: &RuntimeSessionId,
) -> Result<(), Box<dyn std::error::Error>> {
	let error = owner
		.execute(
			"INSERT INTO decodex.managed_run_assignments(\
			 managed_run_id,project_id,runtime_session_id,role)\
			 VALUES($1::text::uuid,$2::text::uuid,$3::text::uuid,'reviewer')",
			&[&managed_run_id.as_str(), &PROJECT_ID, &lead_session_id.as_str()],
		)
		.await
		.expect_err("a Lead RuntimeSession cannot become a Reviewer assignment");
	assert_eq!(error.code(), Some(&SqlState::CHECK_VIOLATION));
	assert_eq!(
		error.as_db_error().and_then(tokio_postgres::error::DbError::constraint),
		Some("managed_run_assignment_scope"),
	);
	Ok(())
}

async fn assert_namespace_rejected(
	runtime: &Client,
	statement: &str,
) -> Result<(), Box<dyn std::error::Error>> {
	let error = runtime
		.batch_execute(statement)
		.await
		.expect_err("ManagedRun-linked activity or outbox authority must fail closed");
	let database = error.as_db_error().ok_or("namespace rejection was not a database error")?;
	assert_eq!(database.code(), &SqlState::INSUFFICIENT_PRIVILEGE);
	assert_eq!(database.constraint(), Some("managed_run_event_namespace"));
	Ok(())
}

struct EventNamespaceFixture {
	protected_activity: i64,
	protected_outbox: i64,
	generic_outbox: i64,
}

async fn create_event_namespace_fixture(
	owner: &Client,
	managed_run_id: &ManagedRunId,
) -> Result<EventNamespaceFixture, Box<dyn std::error::Error>> {
	let protected_activity = owner
		.query_one(
			"INSERT INTO decodex.activity(aggregate_kind,aggregate_id,revision,event_kind,\
			 correlation_key,payload) VALUES('managed_run',$1::text,1,\
			 'managed_run_execution_recorded','xy-1416-namespace-owner',\
			 pg_catalog.jsonb_build_object('managed_run_id',$1::text)) RETURNING sequence",
			&[&managed_run_id.as_str()],
		)
		.await?
		.get(0);
	let protected_outbox = owner
		.query_one(
			"INSERT INTO decodex.outbox(effect_key,aggregate_kind,aggregate_id,\
			 aggregate_revision,payload,available_at) VALUES('xy-1416-protected-delivery',\
			 'generic','linked-managed-run',1,pg_catalog.jsonb_build_object('nested',\
			 pg_catalog.jsonb_build_object('activity_sequence',$1::bigint)),\
			 pg_catalog.clock_timestamp()+interval '1 hour') RETURNING id",
			&[&protected_activity],
		)
		.await?
		.get(0);
	let generic_outbox = owner
		.query_one(
			"INSERT INTO decodex.outbox(effect_key,aggregate_kind,aggregate_id,\
			 aggregate_revision,payload,available_at) VALUES('xy-1416-generic-delivery',\
			 'generic','generic',1,'{\"fixture\":\"generic\"}',\
			 pg_catalog.clock_timestamp()+interval '1 hour') RETURNING id",
			&[],
		)
		.await?
		.get(0);
	Ok(EventNamespaceFixture { protected_activity, protected_outbox, generic_outbox })
}

async fn assert_event_namespace_insert_rejections(
	runtime: &Client,
	managed_run_id: &ManagedRunId,
	namespace: &EventNamespaceFixture,
) -> Result<(), Box<dyn std::error::Error>> {
	for statement in [
		format!(
			"INSERT INTO decodex.activity(aggregate_kind,aggregate_id,revision,event_kind,\
			 correlation_key,payload) VALUES('managed_run','{}',1,\
			 'managed_run_injected','xy-1416-namespace-new','{{}}')",
			managed_run_id.as_str(),
		),
		format!(
			"INSERT INTO decodex.outbox(effect_key,aggregate_kind,aggregate_id,\
			 aggregate_revision,payload) VALUES('xy-1416-linked-injected','generic','generic',\
			 1,'{{\"nested\":{{\"activity_sequence\":{}}}}}')",
			namespace.protected_activity,
		),
		"INSERT INTO decodex.outbox(effect_key,aggregate_kind,aggregate_id,\
		 aggregate_revision,payload) VALUES('xy-1416-malformed-link','generic','generic',\
		 1,'{\"activity_sequence\":\"malformed\"}')"
			.into(),
		"INSERT INTO decodex.outbox(effect_key,aggregate_kind,aggregate_id,\
		 aggregate_revision,payload) VALUES('xy-1416-overflow-link','generic','generic',\
		 1,'{\"activity_sequence\":999999999999999999999999999999}')"
			.into(),
	] {
		assert_namespace_rejected(runtime, &statement).await?;
	}
	Ok(())
}

async fn assert_event_namespace_update_rejections(
	owner: &Client,
	runtime: &Client,
	runtime_config: &Config,
	managed_run_id: &ManagedRunId,
	namespace: &EventNamespaceFixture,
) -> Result<(), Box<dyn std::error::Error>> {
	for statement in [
		format!(
			"UPDATE decodex.outbox SET aggregate_id='rewritten' WHERE id={}",
			namespace.protected_outbox,
		),
		format!(
			"UPDATE decodex.outbox SET aggregate_kind='generic',payload='{{}}' WHERE id={}",
			namespace.protected_outbox,
		),
		format!(
			"UPDATE decodex.outbox SET aggregate_kind='managed_run',aggregate_id='{}' \
			 WHERE id={}",
			managed_run_id.as_str(),
			namespace.generic_outbox,
		),
	] {
		assert_namespace_rejected(runtime, &statement).await?;
	}

	let runtime_role = runtime_config.get_user().ok_or("runtime role is absent")?;
	owner
		.batch_execute(&format!(
			"ALTER TABLE decodex.activity DISABLE TRIGGER activity_append_only; \
			 GRANT UPDATE ON decodex.activity TO {runtime_role}",
		))
		.await?;
	let rewrite_result = assert_namespace_rejected(
		runtime,
		&format!(
			"UPDATE decodex.activity SET aggregate_kind='generic',event_kind='generic_recorded',\
			 payload='{{}}' WHERE sequence={}",
			namespace.protected_activity,
		),
	)
	.await;
	owner
		.batch_execute(&format!(
			"REVOKE UPDATE ON decodex.activity FROM {runtime_role}; \
			 ALTER TABLE decodex.activity ENABLE TRIGGER activity_append_only",
		))
		.await?;
	rewrite_result
}

async fn deliver_protected_outbox(
	store: &PostgresStore,
	owner: &Client,
	managed_run_id: &ManagedRunId,
	protected_outbox: i64,
) -> Result<(), Box<dyn std::error::Error>> {
	owner
		.execute(
			"UPDATE decodex.outbox SET available_at=CASE WHEN id=$1 \
			 THEN pg_catalog.clock_timestamp() ELSE pg_catalog.clock_timestamp()+interval '1 hour' \
			 END WHERE state='pending'",
			&[&protected_outbox],
		)
		.await?;
	let claim = store
		.claim_outbox(OUTBOX_WORKER_ID, 1, Duration::from_secs(5))
		.await?
		.pop()
		.ok_or("protected ManagedRun outbox row was not claimable")?;
	assert_eq!(claim.id, protected_outbox);
	store.begin_outbox_effect(claim.id, OUTBOX_WORKER_ID, &claim.claim_token).await?;
	store
		.record_outbox_receipt(
			claim.id,
			OUTBOX_WORKER_ID,
			&claim.claim_token,
			&serde_json::json!({"provider_receipt": "xy-1416"}),
		)
		.await?;
	store
		.reconcile_outbox(
			claim.id,
			OUTBOX_WORKER_ID,
			&claim.claim_token,
			&OutboxReconciliation {
				readback: serde_json::json!({"managed_run_id": managed_run_id.as_str()}),
				outcome: ReconciliationOutcome::EffectPresent,
			},
			Duration::from_millis(1),
			Duration::from_secs(1),
		)
		.await?;
	assert_eq!(
		owner
			.query_one("SELECT state::text FROM decodex.outbox WHERE id=$1", &[&protected_outbox])
			.await?
			.get::<_, String>(0),
		"delivered",
	);
	Ok(())
}

async fn assert_event_namespace_contract(
	store: &PostgresStore,
	owner: &Client,
	runtime_config: &Config,
	managed_run_id: &ManagedRunId,
) -> Result<(), Box<dyn std::error::Error>> {
	let namespace = create_event_namespace_fixture(owner, managed_run_id).await?;
	let (runtime, connection) = runtime_config.connect(NoTls).await?;
	let connection_task = tokio::spawn(connection);
	assert_event_namespace_insert_rejections(&runtime, managed_run_id, &namespace).await?;
	assert_event_namespace_update_rejections(
		owner,
		&runtime,
		runtime_config,
		managed_run_id,
		&namespace,
	)
	.await?;
	deliver_protected_outbox(store, owner, managed_run_id, namespace.protected_outbox).await?;
	drop(runtime);
	connection_task.await??;
	Ok(())
}

async fn create_provider_projection(
	store: &PostgresStore,
	owner: &Client,
	managed_run_id: &ManagedRunId,
	account_id: &AccountId,
) -> Result<ManagedExecutionId, Box<dyn std::error::Error>> {
	let plan = owner
		.query_one(
			"SELECT plan_id::text,managed_execution_id::text \
			 FROM decodex.continuation_plans WHERE consumer_kind='managed_run_execution' \
			 AND managed_run_id=$1::text::uuid AND kind='same_thread' ORDER BY plan_id LIMIT 1",
			&[&managed_run_id.as_str()],
		)
		.await?;
	let plan_id: String = plan.get(0);
	let execution_id = ManagedExecutionId::new(plan.get::<_, String>(1))?;

	owner
		.execute(
			"INSERT INTO decodex.process_generation_execution_epochs(\
			 execution_epoch_id,authorization_digest,authorized_at)\
			 VALUES($1::text::uuid,$2,pg_catalog.clock_timestamp())",
			&[&EXECUTION_EPOCH_ID, &AUTHORIZATION_DIGEST],
		)
		.await?;
	let generation_id = ProcessGenerationId::new(PROCESS_GENERATION_ID)?;
	let boot_id = ProcessBootIdentity::new("xy-1416-boot")?;
	let generation = ProcessGenerationIntent {
		generation_id: generation_id.clone(),
		account_id: account_id.clone(),
		runner_identity: ProcessRunnerIdentity::new(format!(
			"sha256:{AUTHORIZATION_DIGEST}"
		))?,
		intended_boot_id: boot_id.clone(),
		control_kind: ProcessControlKind::StdioOnlyBestEffortEof,
		isolation_kind: ProcessIsolationKind::Session,
		execution_authorization: ProcessExecutionAuthorization::new(
			ProcessExecutionEpochId::new(EXECUTION_EPOCH_ID)?,
			AUTHORIZATION_DIGEST,
		)?,
	};
	let generation_fence = match store.prepare_process_generation(&generation).await? {
		PrepareProcessGenerationOutcome::Fresh(fence) => fence,
		other => return Err(format!("generation preparation was not fresh: {other:?}").into()),
	};
	assert_eq!(generation_fence.revision(), 1);
	let identity = ProcessIdentity::new(
		boot_id,
		1416,
		ProcessStartIdentity::new("xy-1416-process-start")?,
		1416,
		1416,
	)?;
	let bound_revision = match store
		.bind_process_generation_identity(&generation_id, 1, &identity)
		.await?
	{
		ProcessGenerationMutationOutcome::Applied(mutation) => mutation.revision,
		other => return Err(format!("generation identity was not applied: {other:?}").into()),
	};
	assert_eq!(bound_revision, 2);
	let ready_revision = match store.mark_process_generation_ready(&generation_id, 2).await? {
		ProcessGenerationMutationOutcome::Applied(mutation) => mutation.revision,
		other => return Err(format!("generation readiness was not applied: {other:?}").into()),
	};
	assert_eq!(ready_revision, 3);

	let attempt_id = ProviderAttemptId::new(PROVIDER_ATTEMPT_ID)?;
	let request_id = ProviderRequestId::new(PROVIDER_REQUEST_ID)?;
	let provider_key = ProviderRequestKey::new("xy-1416-provider-idempotency")?;
	let preparation = ProviderAttemptPreparation::new(
		attempt_id.clone(),
		ProviderAttemptConsumer::ManagedRunExecution {
			managed_run_id: managed_run_id.clone(),
			managed_run_revision: 1,
			execution_id: execution_id.clone(),
		},
		plan_id,
		request_id.clone(),
		REQUEST_DIGEST,
		ProviderRequestKeys::new(Some(provider_key.clone()), None)?,
		ProviderDuplicateRisk::OriginalIntent,
	)?;
	let prepared = match store
		.prepare_provider_attempt(&preparation, &generation_id, ready_revision)
		.await?
	{
		PrepareProviderAttemptOutcome::Fresh(prepared) => prepared,
		other => return Err(format!("ProviderAttempt preparation was not fresh: {other:?}").into()),
	};
	assert!(matches!(
		store
			.prepare_provider_attempt(&preparation, &generation_id, ready_revision)
			.await?,
		PrepareProviderAttemptOutcome::Replayed(ref mutation)
			if mutation.revision == 1 && mutation.state == ProviderAttemptState::Prepared
	));
	let dispatch = match store
		.authorize_provider_attempt_dispatch(prepared, &generation_id, ready_revision)
		.await?
	{
		AuthorizeProviderDispatchOutcome::Fresh(dispatch) => dispatch,
		other => return Err(format!("ProviderAttempt dispatch was not fresh: {other:?}").into()),
	};
	assert_eq!(dispatch.attempt_revision(), 2);
	let evidence = ProviderPositiveEvidence::new(
		ProviderEvidenceId::new(PROVIDER_EVIDENCE_ID)?,
		attempt_id,
		request_id,
		ProviderEvidenceSource::ProviderReceipt,
		ProviderTerminalOutcome::Succeeded,
		provider_key,
		Some("xy-1416-positive-provider-receipt".into()),
		None,
		None,
		WITNESS_DIGEST,
	)?;
	assert!(matches!(
		store.record_provider_attempt_positive_evidence(2, &evidence).await?,
		ProviderAttemptMutationOutcome::Applied(ref mutation)
			if mutation.revision == 3 && mutation.state == ProviderAttemptState::Succeeded
	));
	assert!(matches!(
		store.record_provider_attempt_positive_evidence(2, &evidence).await?,
		ProviderAttemptMutationOutcome::Replayed(ref mutation)
			if mutation.revision == 3 && mutation.state == ProviderAttemptState::Succeeded
	));
	Ok(execution_id)
}

async fn assert_readback(
	store: &PostgresStore,
	managed_run_id: &ManagedRunId,
	execution_id: &ManagedExecutionId,
	expected_runtime_session_id: &RuntimeSessionId,
) -> Result<decodex_postgres::StoredManagedRun, Box<dyn std::error::Error>> {
	let readback =
		store.read_managed_run_exact(&ProjectId::new(PROJECT_ID)?, managed_run_id, 1).await?;
	assert_eq!(&readback.managed_run_id, managed_run_id);
	assert_eq!(readback.project_id.as_str(), PROJECT_ID);
	assert_eq!(&readback.runtime_session_id, expected_runtime_session_id);
	assert_eq!(
		readback.state,
		ManagedRunState::Waiting(ManagedRunPhase::Execute, ManagedRunWaitReason::Usage),
	);
	assert_eq!(readback.revision, 1);
	assert!(!readback.diverged);
	assert!(readback.blocked);
	assert_eq!(readback.runtime_session_revision, 1);
	assert_eq!(readback.runtime_session_state, RuntimeSessionState::Active);
	assert_eq!(readback.assignments.len(), 1);
	assert_eq!(readback.assignments[0].role, ExecutionAssignmentRole::Task);
	assert_eq!(
		&readback.assignments[0].runtime_session_id,
		expected_runtime_session_id,
	);
	assert_eq!(readback.provider_attempts.len(), 1);
	let attempt = &readback.provider_attempts[0];
	assert_eq!(&attempt.execution_id, execution_id);
	assert_eq!(attempt.attempt_id.as_str(), PROVIDER_ATTEMPT_ID);
	assert_eq!(attempt.process_generation_id.as_str(), PROCESS_GENERATION_ID);
	assert_eq!(attempt.state, ProviderAttemptState::Succeeded);
	assert_eq!(attempt.revision, 3);
	assert_eq!(
		attempt.terminal_evidence_id.as_ref().map(ProviderEvidenceId::as_str),
		Some(PROVIDER_EVIDENCE_ID),
	);
	assert_eq!(attempt.unknown_reason, None);
	assert!(!readback.created_at.is_empty());
	assert!(!readback.updated_at.is_empty());
	Ok(readback)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires an isolated PostgreSQL 18 V26 ManagedRun database"]
async fn postgres_managed_run_v26_contract() -> Result<(), Box<dyn std::error::Error>> {
	let (migration, runtime) = separated_configs("DECODEX_TEST")?;
	let store =
		PostgresStore::connect(migration.clone(), runtime.clone(), expected_peer_uid()).await?;
	let (owner, connection) = migration.connect(NoTls).await?;
	let connection_task = tokio::spawn(connection);
	assert!(matches!(
		store.bootstrap_role_profiles("xy-1416-role-profiles", &profiles()).await?,
		RoleProfileCommandOutcome::Success(_)
	));
	let routing =
		routing_decision::assert_routing_decision_contract(&store, &owner, &migration, &runtime)
			.await?;
	continuation::assert_continuation_contract(&store, &owner, &migration, &runtime, &routing)
		.await?;
	assert_eq!(routing.selected_managed_run_id.as_str(), SELECTED_MANAGED_RUN_ID);
	let lead_session = create_lead_session(&store, &routing.selected_account_id).await?;
	assert_assignment_scope(&owner, &routing.selected_managed_run_id, &lead_session).await?;
	assert_event_namespace_contract(&store, &owner, &runtime, &routing.selected_managed_run_id)
		.await?;
	let execution_id = create_provider_projection(
		&store,
		&owner,
		&routing.selected_managed_run_id,
		&routing.selected_account_id,
	)
	.await?;
	let readback = assert_readback(
		&store,
		&routing.selected_managed_run_id,
		&execution_id,
		&routing.selected_runtime_session_id,
	)
	.await?;
	assert!(matches!(
		store
			.read_managed_run_exact(
				&ProjectId::new(PROJECT_ID)?,
				&routing.selected_managed_run_id,
				2,
			)
			.await,
		Err(StoreError::InvalidInput("exact ManagedRun revision readback did not match"))
	));
	assert!(matches!(
		store
			.read_managed_run_exact(
				&ProjectId::new(PROJECT_ID)?,
				&routing.selected_managed_run_id,
				0,
			)
			.await,
		Err(StoreError::InvalidInput("ManagedRun revision must be positive"))
	));
	let restarted =
		PostgresStore::connect(migration.clone(), runtime.clone(), expected_peer_uid()).await?;
	assert_eq!(
		assert_readback(
			&restarted,
			&routing.selected_managed_run_id,
			&execution_id,
			&routing.selected_runtime_session_id,
		)
		.await?,
		readback,
	);
	drop(owner);
	connection_task.await??;
	Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires a restored PostgreSQL 18 V26 ManagedRun database"]
async fn postgres_managed_run_v26_restore() -> Result<(), Box<dyn std::error::Error>> {
	let (migration, runtime) = separated_configs("DECODEX_TEST")?;
	let store = PostgresStore::connect(migration, runtime, expected_peer_uid()).await?;
	let readback = store
		.read_managed_run_exact(
			&ProjectId::new(PROJECT_ID)?,
			&ManagedRunId::new(SELECTED_MANAGED_RUN_ID)?,
			1,
		)
		.await?;
	assert_eq!(readback.managed_run_id.as_str(), SELECTED_MANAGED_RUN_ID);
	assert_eq!(readback.project_id.as_str(), PROJECT_ID);
	assert_eq!(readback.runtime_session_id.as_str(), SELECTED_RUNTIME_SESSION_ID);
	assert_eq!(
		readback.state,
		ManagedRunState::Waiting(ManagedRunPhase::Execute, ManagedRunWaitReason::Usage),
	);
	assert_eq!(readback.revision, 1);
	assert!(!readback.diverged);
	assert!(readback.blocked);
	assert_eq!(readback.runtime_session_revision, 1);
	assert_eq!(readback.runtime_session_state, RuntimeSessionState::Active);
	assert_eq!(readback.assignments.len(), 1);
	assert_eq!(readback.assignments[0].role, ExecutionAssignmentRole::Task);
	assert_eq!(
		readback.assignments[0].runtime_session_id.as_str(),
		SELECTED_RUNTIME_SESSION_ID,
	);
	assert_eq!(readback.provider_attempts.len(), 1);
	assert_eq!(
		readback.provider_attempts[0].execution_id.as_str(),
		SELECTED_EXECUTION_ID,
	);
	assert_eq!(readback.provider_attempts[0].attempt_id.as_str(), PROVIDER_ATTEMPT_ID);
	assert_eq!(
		readback.provider_attempts[0].process_generation_id.as_str(),
		PROCESS_GENERATION_ID,
	);
	assert_eq!(readback.provider_attempts[0].state, ProviderAttemptState::Succeeded);
	assert_eq!(readback.provider_attempts[0].revision, 3);
	assert_eq!(
		readback.provider_attempts[0]
			.terminal_evidence_id
			.as_ref()
			.map(ProviderEvidenceId::as_str),
		Some(PROVIDER_EVIDENCE_ID),
	);
	Ok(())
}
