use std::time::Duration;

use tokio::task::JoinSet;
use tokio_postgres::{Client, NoTls, error::SqlState};

use super::{expected_peer_uid, separated_configs, wait_for_any_blocked_by};
use decodex_core::{
	AgentId, ConversationId, ManagedRunId, ManagedRunSafetyInput, ProjectId, RuntimeSessionId,
	RuntimeSessionState, SafetyObservationId, SubmittedTurnReceiptId, TurnId,
	WorkItemCorrelationId, WorkItemId, WorkItemPriority, WorkItemProvenance,
};
use decodex_postgres::{
	AccountId, AccountState, BootstrapRoleProfiles, CommandIdentity, CreateConversation,
	CreateRuntimeSession, CreateRuntimeSessionAccountSnapshot, CreateWorkItem,
	ManagedRunEffectBarrierState, ManagedRunSafetyOutcome, ManagedRunSafetyRejection,
	OutboxReconciliation, PostgresStore, ReconciliationOutcome, RoleProfileCommandOutcome,
	RoleProfileConfiguration, RoleProfileRole, RuntimeSessionCommandOutcome,
	WorkItemCommandOutcome, WorkItemRelations,
};

const PROJECT_ID: &str = "22000000-0000-4000-8000-000000001338";
const LEAD_ID: &str = "32000000-0000-4000-8000-000000001338";
const OUTBOX_WORKER_ID: &str = "82000000-0000-4000-8000-000000001338";

fn uuid(prefix: u8, marker: u8) -> String {
	format!("{prefix:02}000000-0000-4000-8000-{marker:012}")
}

fn non_v4_turn_uuid(prefix: u8, marker: u8) -> String {
	format!("{prefix:02}000000-0000-1000-8000-{marker:012}")
}

fn profile(role: &str) -> RoleProfileConfiguration {
	RoleProfileConfiguration {
		model: format!("gpt-5.6-{role}"),
		reasoning_effort: "medium".into(),
		service_tier: "priority".into(),
		instructions: format!("Inert XY-1338 {role} fixture."),
		provenance: Some("XY-1338 synthetic fixture".into()),
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

async fn create_project(client: &Client) -> Result<(), Box<dyn std::error::Error>> {
	client
		.query_one(
			"SELECT project_id FROM decodex.create_project(\
		 $1::text::decodex.canonical_uuid_v4_text,$2,$3,$3,'{}'::jsonb,\
		 $4::text::decodex.canonical_uuid_v4_text)",
			&[&PROJECT_ID, &"xy-1338/managed-runs", &"/srv/xy-1338-managed-runs", &LEAD_ID],
		)
		.await?;
	Ok(())
}

async fn create_session(
	store: &PostgresStore,
	marker: u8,
	role: RoleProfileRole,
) -> Result<RuntimeSessionId, Box<dyn std::error::Error>> {
	let conversation_id = ConversationId::new(uuid(40, marker))?;
	store
		.create_conversation(
			&CommandIdentity::new(format!("run-conversation-{marker}"), b"xy-1338")?,
			&CreateConversation {
				conversation_id: conversation_id.clone(),
				title: format!("ManagedRun fixture {marker}"),
			},
		)
		.await?;
	let runtime_session_id = RuntimeSessionId::new(uuid(41, marker))?;
	let create = CreateRuntimeSession {
		runtime_session_id: runtime_session_id.clone(),
		conversation_id,
		role,
		account_snapshot: CreateRuntimeSessionAccountSnapshot {
			account_snapshot_id: uuid(43, marker),
			source_account_id: AccountId::new(uuid(13, marker))?,
			display_label: format!("ManagedRun account {marker}"),
			observed_state: AccountState::Unknown,
			source_revision: 1,
		},
		codex_thread_id: Some(uuid(44, marker)),
		initial_state: RuntimeSessionState::Active,
	};
	assert!(matches!(
		store.create_runtime_session(&format!("run-session-{marker}"), &create,).await?,
		RuntimeSessionCommandOutcome::Success(_)
	));
	Ok(runtime_session_id)
}

fn work_item(marker: u8) -> Result<CreateWorkItem, Box<dyn std::error::Error>> {
	Ok(CreateWorkItem {
		work_item_id: WorkItemId::new(uuid(51, marker))?,
		project_id: ProjectId::new(PROJECT_ID)?,
		lead_agent_id: AgentId::new(LEAD_ID)?,
		program_id: None,
		relations: WorkItemRelations::default(),
		title: format!("ManagedRun WorkItem {marker}"),
		description: "Inert safety fixture.".into(),
		priority: WorkItemPriority::High,
		acceptance_criteria: vec!["Safety remains fail closed.".into()],
		validation_criteria: vec!["No progress path exists.".into()],
		provenance: WorkItemProvenance::new(
			AgentId::new(LEAD_ID)?,
			WorkItemCorrelationId::new(uuid(61, marker))?,
			"XY-1338 fixture",
		)?,
	})
}

struct RunFixture {
	managed_run_id: ManagedRunId,
	work_item_id: WorkItemId,
	runtime_session_id: RuntimeSessionId,
}

async fn create_run_fixture(
	store: &PostgresStore,
	owner: &Client,
	marker: u8,
) -> Result<RunFixture, Box<dyn std::error::Error>> {
	let runtime_session_id = create_session(store, marker, RoleProfileRole::Task).await?;
	let create_work_item = work_item(marker)?;
	assert!(matches!(
		store.create_work_item(&format!("run-work-item-{marker}"), &create_work_item,).await?,
		WorkItemCommandOutcome::Success(_)
	));
	let managed_run_id = ManagedRunId::new(uuid(71, marker))?;
	owner
		.execute(
			r#"INSERT INTO decodex.managed_runs(
		 managed_run_id,project_id,work_item_id,runtime_session_id,runtime_session_revision,
		 phase,wait_reason,created_at,updated_at) SELECT $1::text::uuid,$2::text::uuid,
		 $3::text::uuid,$4::text::uuid,1,'execute','external',observed_at,observed_at
		 FROM (SELECT pg_catalog.clock_timestamp() AS observed_at) AS observation"#,
			&[
				&managed_run_id.as_str(),
				&PROJECT_ID,
				&create_work_item.work_item_id.as_str(),
				&runtime_session_id.as_str(),
			],
		)
		.await?;
	owner
		.execute(
			"INSERT INTO decodex.managed_run_assignments(\
		 managed_run_id,project_id,runtime_session_id,role)\
		 VALUES($1::text::uuid,$2::text::uuid,$3::text::uuid,'task')",
			&[&managed_run_id.as_str(), &PROJECT_ID, &runtime_session_id.as_str()],
		)
		.await?;
	owner
		.execute(
			"INSERT INTO decodex.managed_run_effect_barriers(\
		 managed_run_id,project_id,work_item_id) VALUES($1::text::uuid,$2::text::uuid,$3::text::uuid)",
			&[&managed_run_id.as_str(), &PROJECT_ID, &create_work_item.work_item_id.as_str()],
		)
		.await?;
	owner
		.execute(
			"INSERT INTO decodex.managed_run_effects(\
		 effect_id,managed_run_id,project_id,work_item_id,ordinal,kind,effect_key)\
		 VALUES($1::text::uuid,$2::text::uuid,$3::text::uuid,$4::text::uuid,1,'git',$5)",
			&[
				&uuid(81, marker),
				&managed_run_id.as_str(),
				&PROJECT_ID,
				&create_work_item.work_item_id.as_str(),
				&format!("git/{marker}"),
			],
		)
		.await?;
	Ok(RunFixture {
		managed_run_id,
		work_item_id: create_work_item.work_item_id,
		runtime_session_id,
	})
}

async fn assert_unknown_turn_truth_table(
	store: &PostgresStore,
	owner: &Client,
	runtime: &tokio_postgres::Config,
	fixture: &RunFixture,
) -> Result<(), Box<dyn std::error::Error>> {
	let active_turn = uuid(91, 1);
	let (runtime_client, runtime_connection) = runtime.connect(NoTls).await?;
	let runtime_task = tokio::spawn(runtime_connection);
	runtime_client
		.execute(
			"INSERT INTO decodex.turns(turn_id,conversation_id,runtime_session_id,sequence,role)\
		 SELECT $1::text::uuid,conversation_id,runtime_session_id,1,'assistant'\
		 FROM decodex.runtime_sessions WHERE runtime_session_id=$2::text::uuid",
			&[&active_turn, &fixture.runtime_session_id.as_str()],
		)
		.await?;
	let input = ManagedRunSafetyInput::PositivelyObservedUnknownTurn {
		observation_id: SafetyObservationId::new(uuid(92, 1))?,
		runtime_session_id: fixture.runtime_session_id.clone(),
		turn_id: TurnId::new(non_v4_turn_uuid(93, 1))?,
	};
	owner.batch_execute("BEGIN").await?;
	owner
		.query_one(
			"SELECT decodex.apply_managed_run_safety_input_exact(\
		 'decodex/exact-command/1','rolled-back-input',$1::text::uuid,$2::text::uuid,1,\
		 'positively_observed_unknown_turn',$3::text::uuid,$4::text::uuid,$5::text::uuid)",
			&[
				&fixture.managed_run_id.as_str(),
				&PROJECT_ID,
				&uuid(92, 1),
				&fixture.runtime_session_id.as_str(),
				&non_v4_turn_uuid(93, 1),
			],
		)
		.await?;
	owner.batch_execute("ROLLBACK").await?;
	let first = store
		.apply_managed_run_safety_input(
			"unknown-turn-a",
			&ProjectId::new(PROJECT_ID)?,
			&fixture.managed_run_id,
			1,
			&input,
		)
		.await?;
	let replay = store
		.apply_managed_run_safety_input(
			"unknown-turn-b",
			&ProjectId::new(PROJECT_ID)?,
			&fixture.managed_run_id,
			1,
			&input,
		)
		.await?;
	assert_eq!(first, replay);
	for (protocol, key, expected_revision) in [
		("decodex/exact-command/1", "unknown-turn-changed-revision", 2_i64),
		("decodex/exact-command/2", "unknown-turn-changed-protocol", 1_i64),
	] {
		let response: Vec<u8> = owner
			.query_one(
				"SELECT decodex.apply_managed_run_safety_input_exact(\
			 $1,$2,$3::text::uuid,$4::text::uuid,$5,'positively_observed_unknown_turn',\
			 $6::text::uuid,$7::text::uuid,$8::text::uuid)",
				&[
					&protocol,
					&key,
					&fixture.managed_run_id.as_str(),
					&PROJECT_ID,
					&expected_revision,
					&uuid(92, 1),
					&fixture.runtime_session_id.as_str(),
					&non_v4_turn_uuid(93, 1),
				],
			)
			.await?
			.get(0);
		let rejection: serde_json::Value = serde_json::from_slice(&response)?;
		assert_eq!(rejection["classification"], "stable_domain_rejection");
		assert_eq!(rejection["effect"]["reason"], "input_identity_conflict");
	}
	let ManagedRunSafetyOutcome::Success(effect) = first else { panic!("unknown turn rejected") };
	assert!(effect.runtime_session_diverged);
	assert!(effect.effect_barrier_closed_now);
	let row = owner
		.query_one(
			r#"SELECT run.revision,run.diverged,session.state::text,session.revision,barrier.state::text,
		 (SELECT count(*) FROM decodex.turns WHERE turn_id=$2::text::uuid AND status='active'),
			 (SELECT count(*) FROM decodex.managed_run_safety_inputs WHERE managed_run_id=run.managed_run_id),
			 (SELECT count(*) FROM decodex.exact_command_receipts WHERE idempotency_key IN
			 ('unknown-turn-a','unknown-turn-b','unknown-turn-changed-revision',
			 'unknown-turn-changed-protocol')),
			 (SELECT count(*) FROM decodex.exact_command_receipts WHERE idempotency_key IN
			 ('unknown-turn-a','unknown-turn-b') AND receipt_state='completed_success'
			 AND outcome_class='success'),
			 (SELECT count(*) FROM decodex.exact_command_receipts WHERE idempotency_key IN
			 ('unknown-turn-changed-revision','unknown-turn-changed-protocol')
			 AND receipt_state='completed_rejected' AND outcome_class='stable_domain_rejection'),
			 (SELECT count(*) FROM decodex.managed_run_effects WHERE managed_run_id=run.managed_run_id),
			 barrier.revision
		 FROM decodex.managed_runs run JOIN decodex.runtime_sessions session
		 ON session.runtime_session_id=run.runtime_session_id
		 JOIN decodex.managed_run_effect_barriers barrier USING(managed_run_id)
		 WHERE run.managed_run_id=$1::text::uuid"#,
			&[&fixture.managed_run_id.as_str(), &active_turn],
		)
		.await?;
	assert_eq!(row.get::<_, i64>(0), 2);
	assert!(row.get::<_, bool>(1));
	assert_eq!(row.get::<_, String>(2), "diverged");
	assert_eq!(row.get::<_, i64>(3), 2);
	assert_eq!(row.get::<_, String>(4), "closed");
	assert_eq!(row.get::<_, i64>(5), 1);
	assert_eq!(row.get::<_, i64>(6), 1);
	assert_eq!(row.get::<_, i64>(7), 4);
	assert_eq!(row.get::<_, i64>(8), 2);
	assert_eq!(row.get::<_, i64>(9), 2);
	assert_eq!(row.get::<_, i64>(10), 1);
	assert_eq!(row.get::<_, i64>(11), 2);
	drop(runtime_client);
	runtime_task.await??;
	Ok(())
}

async fn assert_unknown_turn_insert_serialization(
	store: &PostgresStore,
	owner: &Client,
	runtime: &tokio_postgres::Config,
	fixture: &RunFixture,
) -> Result<(), Box<dyn std::error::Error>> {
	let turn_id = uuid(94, 1);
	let history_item_id = uuid(95, 1);
	let observation_id = uuid(96, 1);
	let (runtime_client, runtime_connection) = runtime.connect(NoTls).await?;
	let runtime_task = tokio::spawn(runtime_connection);
	let holder_pid: i32 = runtime_client.query_one("SELECT pg_backend_pid()", &[]).await?.get(0);

	runtime_client.batch_execute("BEGIN").await?;
	runtime_client
		.execute(
			r#"INSERT INTO decodex.turns(
		 turn_id,conversation_id,runtime_session_id,sequence,role)
		 SELECT $1::text::uuid,conversation_id,runtime_session_id,2,'assistant'
		 FROM decodex.runtime_sessions WHERE runtime_session_id=$2::text::uuid"#,
			&[&turn_id, &fixture.runtime_session_id.as_str()],
		)
		.await?;
	runtime_client
		.execute(
			r#"INSERT INTO decodex.history_items(
		 history_item_id,conversation_id,history_position,turn_id,ordinal,kind,status,
		 inline_text,media_type)
		 SELECT $1::text::uuid,conversation_id,1,$2::text::uuid,0,'message','completed',
		 'runtime hierarchy fixture','text/plain'
		 FROM decodex.runtime_sessions WHERE runtime_session_id=$3::text::uuid"#,
			&[&history_item_id, &turn_id, &fixture.runtime_session_id.as_str()],
		)
		.await?;

	let task_store = store.clone();
	let managed_run_id = fixture.managed_run_id.clone();
	let runtime_session_id = fixture.runtime_session_id.clone();
	let safety_task = tokio::spawn(async move {
		task_store
			.apply_managed_run_safety_input(
				"unknown-turn-concurrent-insert",
				&ProjectId::new(PROJECT_ID).expect("fixture Project ID is canonical"),
				&managed_run_id,
				1,
				&ManagedRunSafetyInput::PositivelyObservedUnknownTurn {
					observation_id: SafetyObservationId::new(observation_id)
						.expect("fixture observation ID is canonical"),
					runtime_session_id,
					turn_id: TurnId::new(turn_id).expect("fixture Turn ID is canonical"),
				},
			)
			.await
	});

	assert!(
		wait_for_any_blocked_by(owner, holder_pid).await?,
		"ManagedRun safety waited on the hierarchy coordinator held by the Turn writer"
	);
	let run_scope_unheld: bool = owner
		.query_one(
			"SELECT pg_catalog.pg_try_advisory_xact_lock(1338,\
			 pg_catalog.hashtext($1))",
			&[&fixture.managed_run_id.as_str()],
		)
		.await?
		.get(0);
	assert!(run_scope_unheld, "safety acquired run scope before hierarchy authority");
	runtime_client.batch_execute("COMMIT").await?;

	let outcome = tokio::time::timeout(Duration::from_secs(2), safety_task).await???;
	assert!(matches!(
		outcome,
		ManagedRunSafetyOutcome::Rejected(ManagedRunSafetyRejection::TurnAlreadyOwnedOrKnown)
	));
	let state = owner
		.query_one(
			r#"SELECT run.revision,barrier.state::text,
		 (SELECT count(*) FROM decodex.managed_run_safety_inputs
		  WHERE input_id=$2::text::uuid),receipt.receipt_state::text,receipt.outcome_class
		 FROM decodex.managed_runs run
		 JOIN decodex.managed_run_effect_barriers barrier USING(managed_run_id)
		 JOIN decodex.exact_command_receipts receipt
		  ON receipt.protocol_version='decodex/exact-command/1'
		  AND receipt.idempotency_key='unknown-turn-concurrent-insert'
		 WHERE run.managed_run_id=$1::text::uuid"#,
			&[&fixture.managed_run_id.as_str(), &uuid(96, 1)],
		)
		.await?;
	assert_eq!(state.get::<_, i64>(0), 1);
	assert_eq!(state.get::<_, String>(1), "guarded");
	assert_eq!(state.get::<_, i64>(2), 0);
	assert_eq!(state.get::<_, String>(3), "completed_rejected");
	assert_eq!(state.get::<_, String>(4), "stable_domain_rejection");

	runtime_client.batch_execute("BEGIN ISOLATION LEVEL REPEATABLE READ").await?;
	let turn_error = runtime_client
		.execute(
			r#"INSERT INTO decodex.turns(
		 turn_id,conversation_id,runtime_session_id,sequence,role)
		 SELECT $1::text::uuid,conversation_id,runtime_session_id,3,'assistant'
		 FROM decodex.runtime_sessions WHERE runtime_session_id=$2::text::uuid"#,
			&[&uuid(97, 1), &fixture.runtime_session_id.as_str()],
		)
		.await
		.expect_err("non-READ-COMMITTED Turn write must fail closed");
	assert_eq!(turn_error.code(), Some(&SqlState::T_R_SERIALIZATION_FAILURE));
	runtime_client.batch_execute("ROLLBACK").await?;

	runtime_client.batch_execute("BEGIN ISOLATION LEVEL SERIALIZABLE").await?;
	let history_error = runtime_client
		.execute(
			r#"INSERT INTO decodex.history_items(
		 history_item_id,conversation_id,history_position,turn_id,ordinal,kind,status,
		 inline_text,media_type)
		 SELECT $1::text::uuid,conversation_id,2,$2::text::uuid,1,'message','completed',
		 'unsupported isolation fixture','text/plain'
		 FROM decodex.runtime_sessions WHERE runtime_session_id=$3::text::uuid"#,
			&[&uuid(98, 1), &uuid(94, 1), &fixture.runtime_session_id.as_str()],
		)
		.await
		.expect_err("non-READ-COMMITTED HistoryItem write must fail closed");
	assert_eq!(history_error.code(), Some(&SqlState::T_R_SERIALIZATION_FAILURE));
	runtime_client.batch_execute("ROLLBACK").await?;

	drop(runtime_client);
	runtime_task.await??;
	Ok(())
}

async fn assert_receipt_case(
	store: &PostgresStore,
	owner: &Client,
	fixture: &RunFixture,
	marker: u8,
	receipt_revision: i64,
	expected_stale: bool,
) -> Result<(), Box<dyn std::error::Error>> {
	let receipt_id = SubmittedTurnReceiptId::new(uuid(82, marker))?;
	let turn_id = TurnId::new(non_v4_turn_uuid(83, marker))?;
	owner.execute(
		"INSERT INTO decodex.managed_run_submitted_turn_receipts(\
		 receipt_id,managed_run_id,project_id,runtime_session_id,runtime_session_revision,turn_id,submitted_at)\
		 VALUES($1::text::uuid,$2::text::uuid,$3::text::uuid,$4::text::uuid,$5,$6::text::uuid,\
		 pg_catalog.clock_timestamp())",
		&[&receipt_id.as_str(), &fixture.managed_run_id.as_str(), &PROJECT_ID,
			&fixture.runtime_session_id.as_str(), &receipt_revision, &turn_id.as_str()],
	).await?;
	let outcome = store
		.apply_managed_run_safety_input(
			&format!("submitted-receipt-{marker}"),
			&ProjectId::new(PROJECT_ID)?,
			&fixture.managed_run_id,
			1,
			&ManagedRunSafetyInput::SubmittedTurnReceipt {
				receipt_id,
				runtime_session_id: fixture.runtime_session_id.clone(),
				turn_id,
			},
		)
		.await?;
	let ManagedRunSafetyOutcome::Success(effect) = outcome else { panic!("receipt rejected") };
	assert_eq!(effect.stale_receipt, expected_stale);
	assert!(!effect.runtime_session_diverged);
	assert!(effect.effect_barrier_closed_now);
	Ok(())
}

async fn assert_inconclusive_case(
	store: &PostgresStore,
	fixture: &RunFixture,
	marker: u8,
) -> Result<(), Box<dyn std::error::Error>> {
	let outcome = store
		.apply_managed_run_safety_input(
			&format!("inconclusive-{marker}"),
			&ProjectId::new(PROJECT_ID)?,
			&fixture.managed_run_id,
			1,
			&ManagedRunSafetyInput::InconclusiveObservation {
				observation_id: SafetyObservationId::new(uuid(84, marker))?,
				runtime_session_id: fixture.runtime_session_id.clone(),
			},
		)
		.await?;
	let ManagedRunSafetyOutcome::Success(effect) = outcome else { panic!("inconclusive rejected") };
	assert!(!effect.runtime_session_diverged);
	assert!(!effect.stale_receipt);
	assert!(effect.managed_run_blocked);
	Ok(())
}

async fn assert_fk_and_role_scope(
	owner: &Client,
	fixture: &RunFixture,
	lead_session_id: &RuntimeSessionId,
) -> Result<(), Box<dyn std::error::Error>> {
	assert!(
		owner
			.execute(
				"INSERT INTO decodex.managed_run_effects(\
		 effect_id,managed_run_id,project_id,work_item_id,ordinal,kind,effect_key)\
		 VALUES($1::text::uuid,$2::text::uuid,$3::text::uuid,$4::text::uuid,2,'tool','cross-scope')",
				&[
					&uuid(85, 1),
					&fixture.managed_run_id.as_str(),
					&uuid(22, 99),
					&fixture.work_item_id.as_str()
				],
			)
			.await
			.is_err()
	);
	assert!(
		owner
			.execute(
				"INSERT INTO decodex.managed_run_assignments(\
		 managed_run_id,project_id,runtime_session_id,role)\
		 VALUES($1::text::uuid,$2::text::uuid,$3::text::uuid,'reviewer')",
				&[&fixture.managed_run_id.as_str(), &PROJECT_ID, &lead_session_id.as_str()],
			)
			.await
			.is_err()
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
		.expect_err("ManagedRun-linked activity/outbox authority must reject the runtime role");
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
	fixture: &RunFixture,
) -> Result<EventNamespaceFixture, Box<dyn std::error::Error>> {
	let protected_activity: i64 = owner
		.query_one(
			"INSERT INTO decodex.activity(aggregate_kind,aggregate_id,revision,event_kind,\
			 correlation_key,payload) VALUES('managed_run',$1::text,1,'managed_run_safety_recorded',\
			 'xy-1338-namespace-owner',jsonb_build_object('managed_run_id',$1::text)) RETURNING sequence",
			&[&fixture.managed_run_id.as_str()],
		)
		.await?
		.get(0);
	let protected_outbox: i64 = owner
		.query_one(
			"INSERT INTO decodex.outbox(effect_key,aggregate_kind,aggregate_id,\
		 aggregate_revision,payload,available_at) VALUES('xy-1338-protected-delivery','generic',\
			 'linked-managed-run',1,jsonb_build_object('nested',jsonb_build_object(\
			 'activity_sequence',$1::bigint)),clock_timestamp()+interval '1 hour') RETURNING id",
			&[&protected_activity],
		)
		.await?
		.get(0);
	let generic_outbox: i64 = owner
		.query_one(
			"INSERT INTO decodex.outbox(effect_key,aggregate_kind,aggregate_id,\
		 aggregate_revision,payload,available_at) VALUES('xy-1338-generic-delivery','generic',\
		 'generic',1,'{\"fixture\":\"generic\"}',clock_timestamp()+interval '1 hour') RETURNING id",
			&[],
		)
		.await?
		.get(0);
	Ok(EventNamespaceFixture { protected_activity, protected_outbox, generic_outbox })
}

async fn assert_event_namespace_insert_rejections(
	runtime: &Client,
	fixture: &RunFixture,
	namespace: &EventNamespaceFixture,
) -> Result<(), Box<dyn std::error::Error>> {
	assert_namespace_rejected(
		runtime,
		&format!(
			"INSERT INTO decodex.activity(aggregate_kind,aggregate_id,revision,event_kind,\
			 correlation_key,payload) VALUES('managed_run','{}',1,'managed_run_injected',\
			 'xy-1338-namespace-new','{{}}')",
			fixture.managed_run_id.as_str(),
		),
	)
	.await?;
	assert_namespace_rejected(
		runtime,
		&format!(
			"INSERT INTO decodex.outbox(effect_key,aggregate_kind,aggregate_id,\
			 aggregate_revision,payload) VALUES('xy-1338-linked-injected','generic','generic',1,\
			 '{{\"nested\":{{\"activity_sequence\":{}}}}}')",
			namespace.protected_activity,
		),
	)
	.await?;
	assert_namespace_rejected(
		runtime,
		"INSERT INTO decodex.outbox(effect_key,aggregate_kind,aggregate_id,aggregate_revision,payload) \
		 VALUES('xy-1338-malformed-link','generic','generic',1,\
		 '{\"activity_sequence\":\"malformed\"}')",
	)
	.await?;
	assert_namespace_rejected(
		runtime,
		"INSERT INTO decodex.outbox(effect_key,aggregate_kind,aggregate_id,aggregate_revision,payload) \
		 VALUES('xy-1338-overflow-link','generic','generic',1,\
		 '{\"activity_sequence\":999999999999999999999999999999}')",
	)
	.await?;
	Ok(())
}

async fn assert_event_namespace_update_rejections(
	owner: &Client,
	runtime: &Client,
	runtime_config: &tokio_postgres::Config,
	fixture: &RunFixture,
	namespace: &EventNamespaceFixture,
) -> Result<(), Box<dyn std::error::Error>> {
	assert_namespace_rejected(
		runtime,
		&format!(
			"UPDATE decodex.outbox SET aggregate_id='rewritten' WHERE id={}",
			namespace.protected_outbox,
		),
	)
	.await?;
	assert_namespace_rejected(
		runtime,
		&format!(
			"UPDATE decodex.outbox SET aggregate_kind='generic',payload='{{}}' WHERE id={}",
			namespace.protected_outbox,
		),
	)
	.await?;
	assert_namespace_rejected(
		runtime,
		&format!(
			"UPDATE decodex.outbox SET aggregate_kind='managed_run',aggregate_id='{}' \
			 WHERE id={}",
			fixture.managed_run_id.as_str(),
			namespace.generic_outbox,
		),
	)
	.await?;

	let runtime_role = runtime_config.get_user().ok_or("runtime role is absent")?;
	owner
		.batch_execute(&format!(
			"ALTER TABLE decodex.activity DISABLE TRIGGER activity_append_only; \
			 GRANT UPDATE ON decodex.activity TO {runtime_role}"
		))
		.await?;
	let old_activity_result = assert_namespace_rejected(
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
			 ALTER TABLE decodex.activity ENABLE TRIGGER activity_append_only"
		))
		.await?;
	old_activity_result?;
	Ok(())
}

async fn deliver_protected_outbox(
	store: &PostgresStore,
	owner: &Client,
	fixture: &RunFixture,
	protected_outbox: i64,
) -> Result<(), Box<dyn std::error::Error>> {
	owner
		.execute(
			"UPDATE decodex.outbox SET available_at=CASE WHEN id=$1 THEN clock_timestamp()\
		 ELSE clock_timestamp()+interval '1 hour' END WHERE state='pending'",
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
			&serde_json::json!({"provider_receipt": "xy-1338"}),
		)
		.await?;
	store
		.reconcile_outbox(
			claim.id,
			OUTBOX_WORKER_ID,
			&claim.claim_token,
			&OutboxReconciliation {
				readback: serde_json::json!({"managed_run_id": fixture.managed_run_id.as_str()}),
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
	runtime_config: &tokio_postgres::Config,
	fixture: &RunFixture,
) -> Result<(), Box<dyn std::error::Error>> {
	let namespace = create_event_namespace_fixture(owner, fixture).await?;
	let (runtime, runtime_connection) = runtime_config.connect(NoTls).await?;
	let runtime_task = tokio::spawn(runtime_connection);
	assert_event_namespace_insert_rejections(&runtime, fixture, &namespace).await?;
	assert_event_namespace_update_rejections(owner, &runtime, runtime_config, fixture, &namespace)
		.await?;
	deliver_protected_outbox(store, owner, fixture, namespace.protected_outbox).await?;
	drop(runtime);
	runtime_task.await??;
	Ok(())
}

fn inconclusive_input(
	fixture: &RunFixture,
	marker: u8,
) -> Result<ManagedRunSafetyInput, Box<dyn std::error::Error>> {
	Ok(ManagedRunSafetyInput::InconclusiveObservation {
		observation_id: SafetyObservationId::new(uuid(86, marker))?,
		runtime_session_id: fixture.runtime_session_id.clone(),
	})
}

async fn apply_inconclusive_exact(
	client: &Client,
	key: &str,
	fixture: &RunFixture,
	marker: u8,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
	Ok(client
		.query_one(
			"SELECT decodex.apply_managed_run_safety_input_exact(\
		 'decodex/exact-command/1',$1,$2::text::uuid,$3::text::uuid,1,\
		 'inconclusive_observation',$4::text::uuid,$5::text::uuid,NULL::uuid)",
			&[
				&key,
				&fixture.managed_run_id.as_str(),
				&PROJECT_ID,
				&uuid(86, marker),
				&fixture.runtime_session_id.as_str(),
			],
		)
		.await?
		.get(0))
}

async fn assert_single_safety_application(
	owner: &Client,
	fixture: &RunFixture,
	key: &str,
	marker: u8,
) -> Result<(), Box<dyn std::error::Error>> {
	let state = owner
		.query_one(
			r#"SELECT run.revision,barrier.revision,barrier.closure_input_id::text,
		 (SELECT count(*) FROM decodex.managed_run_safety_inputs input
		  WHERE input.managed_run_id=run.managed_run_id),
		 (SELECT count(*) FROM decodex.exact_command_receipts WHERE idempotency_key=$2),
		 (SELECT count(*) FROM decodex.managed_run_effects effect
		  WHERE effect.managed_run_id=run.managed_run_id)
		 FROM decodex.managed_runs run JOIN decodex.managed_run_effect_barriers barrier
		 USING(managed_run_id) WHERE run.managed_run_id=$1::text::uuid"#,
			&[&fixture.managed_run_id.as_str(), &key],
		)
		.await?;
	assert_eq!(state.get::<_, i64>(0), 2);
	assert_eq!(state.get::<_, i64>(1), 2);
	assert_eq!(state.get::<_, String>(2), uuid(86, marker));
	assert_eq!(state.get::<_, i64>(3), 1);
	assert_eq!(state.get::<_, i64>(4), 1);
	assert_eq!(state.get::<_, i64>(5), 1);
	Ok(())
}

async fn assert_precommit_connection_loss_retry(
	store: &PostgresStore,
	owner: &Client,
	migration: &tokio_postgres::Config,
	fixture: &RunFixture,
) -> Result<(), Box<dyn std::error::Error>> {
	let (precommit_client, precommit_connection) = migration.connect(NoTls).await?;
	let precommit_task = tokio::spawn(precommit_connection);
	precommit_client.batch_execute("BEGIN").await?;
	let _lost_before_commit =
		apply_inconclusive_exact(&precommit_client, "managed-run-precommit-retry", fixture, 5)
			.await?;
	drop(precommit_client);
	precommit_task.await??;
	assert!(matches!(
		store
			.apply_managed_run_safety_input(
				"managed-run-precommit-retry",
				&ProjectId::new(PROJECT_ID)?,
				&fixture.managed_run_id,
				1,
				&inconclusive_input(fixture, 5)?,
			)
			.await?,
		ManagedRunSafetyOutcome::Success(_)
	));
	assert_single_safety_application(owner, fixture, "managed-run-precommit-retry", 5).await?;
	Ok(())
}

async fn assert_postcommit_lost_result_replay(
	store: &PostgresStore,
	owner: &Client,
	migration: &tokio_postgres::Config,
	fixture: &RunFixture,
) -> Result<(), Box<dyn std::error::Error>> {
	let (postcommit_client, postcommit_connection) = migration.connect(NoTls).await?;
	let postcommit_task = tokio::spawn(postcommit_connection);
	postcommit_client.batch_execute("BEGIN").await?;
	let lost_after_commit =
		apply_inconclusive_exact(&postcommit_client, "managed-run-postcommit-replay", fixture, 6)
			.await?;
	postcommit_client.batch_execute("COMMIT").await?;
	drop(postcommit_client);
	postcommit_task.await??;
	assert!(matches!(
		store
			.apply_managed_run_safety_input(
				"managed-run-postcommit-replay",
				&ProjectId::new(PROJECT_ID)?,
				&fixture.managed_run_id,
				1,
				&inconclusive_input(fixture, 6)?,
			)
			.await?,
		ManagedRunSafetyOutcome::Success(_)
	));
	assert_eq!(
		owner
			.query_one(
				r#"SELECT response_bytes FROM decodex.exact_command_receipts
			 WHERE protocol_version='decodex/exact-command/1'
			 AND idempotency_key='managed-run-postcommit-replay'"#,
				&[],
			)
			.await?
			.get::<_, Vec<u8>>(0),
		lost_after_commit,
	);
	assert_single_safety_application(owner, fixture, "managed-run-postcommit-replay", 6).await?;
	Ok(())
}

async fn assert_concurrent_identical_retries(
	store: &PostgresStore,
	owner: &Client,
	fixture: &RunFixture,
) -> Result<(), Box<dyn std::error::Error>> {
	let input = inconclusive_input(fixture, 7)?;
	let mut retries = JoinSet::new();
	for _ in 0..2 {
		let store = store.clone();
		let input = input.clone();
		let managed_run_id = fixture.managed_run_id.clone();
		retries.spawn(async move {
			store
				.apply_managed_run_safety_input(
					"managed-run-concurrent-retry",
					&ProjectId::new(PROJECT_ID).expect("fixture Project ID is canonical"),
					&managed_run_id,
					1,
					&input,
				)
				.await
		});
	}
	let mut outcome = None;
	while let Some(result) = retries.join_next().await {
		let current = result??;
		assert!(matches!(current, ManagedRunSafetyOutcome::Success(_)));
		if let Some(expected) = &outcome {
			assert_eq!(&current, expected);
		} else {
			outcome = Some(current);
		}
	}
	assert_single_safety_application(owner, fixture, "managed-run-concurrent-retry", 7).await?;
	Ok(())
}

async fn assert_crash_retry_contract(
	store: &PostgresStore,
	owner: &Client,
	migration: &tokio_postgres::Config,
	precommit: &RunFixture,
	postcommit: &RunFixture,
	concurrent: &RunFixture,
) -> Result<(), Box<dyn std::error::Error>> {
	assert_precommit_connection_loss_retry(store, owner, migration, precommit).await?;
	assert_postcommit_lost_result_replay(store, owner, migration, postcommit).await?;
	assert_concurrent_identical_retries(store, owner, concurrent).await?;
	Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires an isolated PostgreSQL 18 V12 ManagedRun database"]
async fn postgres_managed_run_safety_contract() -> Result<(), Box<dyn std::error::Error>> {
	let (migration, runtime) = separated_configs("DECODEX_TEST")?;
	let store =
		PostgresStore::connect(migration.clone(), runtime.clone(), expected_peer_uid()).await?;
	let (owner, connection) = migration.connect(NoTls).await?;
	let owner_task = tokio::spawn(connection);
	create_project(&owner).await?;
	assert!(matches!(
		store.bootstrap_role_profiles("managed-run-profiles", &profiles()).await?,
		RoleProfileCommandOutcome::Success(_)
	));
	let unknown = create_run_fixture(&store, &owner, 1).await?;
	let current = create_run_fixture(&store, &owner, 2).await?;
	let stale = create_run_fixture(&store, &owner, 3).await?;
	let inconclusive = create_run_fixture(&store, &owner, 4).await?;
	let precommit = create_run_fixture(&store, &owner, 5).await?;
	let postcommit = create_run_fixture(&store, &owner, 6).await?;
	let concurrent = create_run_fixture(&store, &owner, 7).await?;
	let lead_session_id = create_session(&store, 9, RoleProfileRole::Lead).await?;
	assert_unknown_turn_insert_serialization(&store, &owner, &runtime, &unknown).await?;
	assert_unknown_turn_truth_table(&store, &owner, &runtime, &unknown).await?;
	assert_receipt_case(&store, &owner, &current, 2, 1, false).await?;
	assert_receipt_case(&store, &owner, &stale, 3, 99, true).await?;
	assert_inconclusive_case(&store, &inconclusive, 4).await?;
	assert_fk_and_role_scope(&owner, &unknown, &lead_session_id).await?;
	assert_event_namespace_contract(&store, &owner, &runtime, &unknown).await?;
	assert_crash_retry_contract(&store, &owner, &migration, &precommit, &postcommit, &concurrent)
		.await?;
	let readback = store
		.read_managed_run_exact(&ProjectId::new(PROJECT_ID)?, &unknown.managed_run_id, 2)
		.await?;
	assert!(readback.diverged && readback.blocked);
	assert_eq!(readback.assignments.len(), 1);
	assert_eq!(readback.effects.len(), 1);
	assert_eq!(readback.barrier.state, ManagedRunEffectBarrierState::Closed);
	drop(owner);
	owner_task.await??;
	Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires a restored PostgreSQL 18 V12 ManagedRun database"]
async fn postgres_managed_run_safety_restore() -> Result<(), Box<dyn std::error::Error>> {
	let (migration, runtime) = separated_configs("DECODEX_TEST")?;
	let store = PostgresStore::connect(migration, runtime, expected_peer_uid()).await?;
	let readback = store
		.read_managed_run_exact(&ProjectId::new(PROJECT_ID)?, &ManagedRunId::new(uuid(71, 1))?, 2)
		.await?;
	assert!(readback.diverged && readback.blocked);
	assert_eq!(readback.runtime_session_state, RuntimeSessionState::Diverged);
	assert_eq!(readback.barrier.state, ManagedRunEffectBarrierState::Closed);
	Ok(())
}
