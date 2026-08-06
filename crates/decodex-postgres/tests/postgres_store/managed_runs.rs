use std::time::Duration;

use tokio_postgres::{Client, Config, NoTls, error::SqlState};

use super::{continuation, expected_peer_uid, owner_runtime_configs, routing_decision};
use decodex_core::{
	ConversationId, ExecutionAssignmentRole, ManagedRunId, ManagedRunPhase, ManagedRunState,
	ManagedRunWaitReason, ProjectId, RuntimeSessionId, RuntimeSessionState,
};
use decodex_postgres::{
	AccountId, AccountState, BootstrapRoleProfiles, CommandIdentity, CreateConversation,
	CreateRuntimeSession, CreateRuntimeSessionAccountSnapshot, OutboxReconciliation, PostgresStore,
	ReconciliationOutcome, RoleProfileCommandOutcome, RoleProfileConfiguration, RoleProfileRole,
	RuntimeSessionCommandOutcome, StoreError,
};

const PROJECT_ID: &str = "a1000000-0000-4000-8000-000000000016";
const SELECTED_MANAGED_RUN_ID: &str = "c6000000-0000-4000-8000-000000000016";
const SELECTED_RUNTIME_SESSION_ID: &str = "c2000000-0000-4000-8000-000000000016";
const OUTBOX_WORKER_ID: &str = "d6000000-0000-4000-8000-000000001416";

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
	let conversation_id = ConversationId::new("d7000000-0000-4000-8000-000000001416")?;
	store
		.create_conversation(
			&CommandIdentity::new("xy-1416-lead-conversation", b"xy-1416")?,
			&CreateConversation {
				conversation_id: conversation_id.clone(),
				title: "XY-1416 Lead assignment scope".into(),
			},
		)
		.await?;
	let runtime_session_id = RuntimeSessionId::new("d8000000-0000-4000-8000-000000001416")?;
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
				codex_thread_id: None,
				initial_state: RuntimeSessionState::Starting,
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

async fn assert_readback(
	store: &PostgresStore,
	managed_run_id: &ManagedRunId,
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
	assert_eq!(readback.runtime_session_state, RuntimeSessionState::Starting);
	assert_eq!(readback.assignments.len(), 1);
	assert_eq!(readback.assignments[0].role, ExecutionAssignmentRole::Task);
	assert_eq!(&readback.assignments[0].runtime_session_id, expected_runtime_session_id,);
	assert!(readback.provider_attempts.is_empty());
	assert!(!readback.created_at.is_empty());
	assert!(!readback.updated_at.is_empty());
	Ok(readback)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires an isolated PostgreSQL 18 V26 ManagedRun database"]
async fn postgres_managed_run_v26_contract() -> Result<(), Box<dyn std::error::Error>> {
	let (schema_owner, runtime) = owner_runtime_configs("DECODEX_TEST")?;
	let store =
		PostgresStore::connect_runtime_fixture(runtime.clone(), expected_peer_uid()).await?;
	let (owner, connection) = schema_owner.connect(NoTls).await?;
	let connection_task = tokio::spawn(connection);
	assert!(matches!(
		store.bootstrap_role_profiles("xy-1416-role-profiles", &profiles()).await?,
		RoleProfileCommandOutcome::Success(_)
	));
	let routing =
		routing_decision::assert_routing_decision_contract(&store, &owner, &runtime).await?;
	continuation::assert_continuation_contract(&store, &owner, &runtime, &routing).await?;
	let selected_managed_run_id = ManagedRunId::new(SELECTED_MANAGED_RUN_ID)?;
	let lead_session = create_lead_session(&store, &routing.selected_account_id).await?;
	assert_assignment_scope(&owner, &selected_managed_run_id, &lead_session).await?;
	assert_event_namespace_contract(&store, &owner, &runtime, &selected_managed_run_id).await?;
	let readback =
		assert_readback(&store, &selected_managed_run_id, &routing.selected_runtime_session_id)
			.await?;
	assert!(matches!(
		store
			.read_managed_run_exact(&ProjectId::new(PROJECT_ID)?, &selected_managed_run_id, 2,)
			.await,
		Err(StoreError::InvalidInput("exact ManagedRun revision readback did not match"))
	));
	assert!(matches!(
		store
			.read_managed_run_exact(&ProjectId::new(PROJECT_ID)?, &selected_managed_run_id, 0,)
			.await,
		Err(StoreError::InvalidInput("ManagedRun revision must be positive"))
	));
	let restarted =
		PostgresStore::connect_runtime_fixture(runtime.clone(), expected_peer_uid()).await?;
	assert_eq!(
		assert_readback(
			&restarted,
			&selected_managed_run_id,
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
	let (_, runtime) = owner_runtime_configs("DECODEX_TEST")?;
	let store =
		PostgresStore::connect_runtime_fixture(runtime.clone(), expected_peer_uid()).await?;
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
	assert_eq!(readback.assignments[0].runtime_session_id.as_str(), SELECTED_RUNTIME_SESSION_ID,);
	assert!(readback.provider_attempts.is_empty());
	Ok(())
}
