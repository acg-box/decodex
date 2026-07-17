use std::{env, path::PathBuf};

use tokio::{task::JoinSet, time};
use tokio_postgres::{Client, NoTls};

use super::{expected_peer_uid, separated_configs};
use decodex_core::{ConversationId, RuntimeSessionId, RuntimeSessionState};
use decodex_postgres::{
	AccountId, AccountState, BootstrapRoleProfiles, CommandIdentity, CreateConversation,
	CreateRuntimeSession, CreateRuntimeSessionAccountSnapshot, PostgresStore,
	RoleProfileCommandOutcome, RoleProfileConfiguration, RoleProfileRole,
	RuntimeSessionCommandEffect, RuntimeSessionCommandOutcome, RuntimeSessionRejection, StoreError,
};

fn profile(marker: &str) -> RoleProfileConfiguration {
	RoleProfileConfiguration {
		model: format!("gpt-5.6-{marker}"),
		reasoning_effort: "medium".into(),
		service_tier: "priority".into(),
		instructions: format!("Immutable XY-1337 {marker} instructions."),
		provenance: Some(format!("XY-1337 {marker}")),
	}
}

fn profiles(marker: &str) -> BootstrapRoleProfiles {
	BootstrapRoleProfiles {
		advisor: profile(&format!("advisor-{marker}")),
		lead: profile(&format!("lead-{marker}")),
		task: profile(&format!("task-{marker}")),
		reviewer: profile(&format!("reviewer-{marker}")),
	}
}

fn account(
	snapshot_id: &str,
	marker: &str,
) -> Result<CreateRuntimeSessionAccountSnapshot, StoreError> {
	Ok(CreateRuntimeSessionAccountSnapshot {
		account_snapshot_id: snapshot_id.into(),
		source_account_id: AccountId::new("13000000-0000-4000-8000-000000000001")
			.map_err(|_| StoreError::InvalidInput("invalid fixture account identity"))?,
		display_label: format!("Account {marker}"),
		observed_state: AccountState::Unknown,
		source_revision: 7,
	})
}

fn creation(
	session_id: &str,
	conversation_id: &ConversationId,
	snapshot_id: &str,
	marker: &str,
) -> Result<CreateRuntimeSession, StoreError> {
	Ok(CreateRuntimeSession {
		runtime_session_id: RuntimeSessionId::new(session_id)
			.map_err(|_| StoreError::InvalidInput("invalid fixture RuntimeSession identity"))?,
		conversation_id: conversation_id.clone(),
		role: RoleProfileRole::Task,
		account_snapshot: account(snapshot_id, marker)?,
		codex_thread_id: Some(session_id.replacen("41000000", "44000000", 1)),
		initial_state: RuntimeSessionState::Starting,
	})
}

async fn stored_response(client: &Client, key: &str) -> Result<Vec<u8>, tokio_postgres::Error> {
	Ok(client
		.query_one(
			"SELECT response_bytes FROM decodex.exact_command_receipts \
			 WHERE protocol_version='decodex/exact-command/1' AND idempotency_key=$1",
			&[&key],
		)
		.await?
		.get(0))
}

fn success(
	value: RuntimeSessionCommandOutcome<RuntimeSessionCommandEffect>,
) -> RuntimeSessionCommandEffect {
	let RuntimeSessionCommandOutcome::Success(effect) = value else {
		panic!("exact RuntimeSession command must succeed")
	};
	effect
}

struct CreatedSessionFixture {
	request: CreateRuntimeSession,
	effect: RuntimeSessionCommandEffect,
}

async fn assert_creation_command(
	store: &PostgresStore,
	client: &Client,
	conversation_id: &ConversationId,
) -> Result<CreatedSessionFixture, Box<dyn std::error::Error>> {
	let create = creation(
		"41000000-0000-4000-8000-000000000001",
		conversation_id,
		"43000000-0000-4000-8000-000000000001",
		"one",
	)?;
	let created = success(store.create_runtime_session("session-create", &create).await?);
	assert_eq!(created.prior_state, None);
	assert_eq!(created.prior_revision, None);
	assert_eq!(created.new_state, RuntimeSessionState::Starting);
	assert_eq!(created.new_revision, 1);
	assert_eq!(created.runtime_session.last_known_turn_id, None);
	let create_bytes = stored_response(client, "session-create").await?;
	assert_eq!(
		store.create_runtime_session("session-create", &create).await?,
		RuntimeSessionCommandOutcome::Success(created.clone()),
	);
	assert_eq!(stored_response(client, "session-create").await?, create_bytes);

	let mut substitutions = Vec::new();
	let mut changed = create.clone();
	changed.runtime_session_id = RuntimeSessionId::new("41000000-0000-4000-8000-000000000002")?;
	substitutions.push(changed);
	let mut changed = create.clone();
	changed.conversation_id = ConversationId::new("40000000-0000-4000-8000-000000000002")?;
	substitutions.push(changed);
	let mut changed = create.clone();
	changed.role = RoleProfileRole::Reviewer;
	substitutions.push(changed);
	let mut changed = create.clone();
	changed.account_snapshot.account_snapshot_id = "43000000-0000-4000-8000-000000000002".into();
	substitutions.push(changed);
	let mut changed = create.clone();
	changed.account_snapshot.source_account_id =
		AccountId::new("13000000-0000-4000-8000-000000000002")?;
	substitutions.push(changed);
	let mut changed = create.clone();
	changed.account_snapshot.display_label.push_str(" changed");
	substitutions.push(changed);
	let mut changed = create.clone();
	changed.account_snapshot.observed_state = AccountState::Unavailable;
	substitutions.push(changed);
	let mut changed = create.clone();
	changed.account_snapshot.source_revision += 1;
	substitutions.push(changed);
	let mut changed = create.clone();
	changed.codex_thread_id = None;
	substitutions.push(changed);
	let mut changed = create.clone();
	changed.initial_state = RuntimeSessionState::Active;
	substitutions.push(changed);
	for substitution in substitutions {
		assert!(matches!(
			store.create_runtime_session("session-create", &substitution).await,
			Err(StoreError::IdempotencyConflict)
		));
	}

	assert_eq!(
		store.create_runtime_session("session-duplicate", &create).await?,
		RuntimeSessionCommandOutcome::Rejected(RuntimeSessionRejection::DuplicateTarget),
	);
	let duplicate_bytes = stored_response(client, "session-duplicate").await?;
	assert_eq!(
		store.create_runtime_session("session-duplicate", &create).await?,
		RuntimeSessionCommandOutcome::Rejected(RuntimeSessionRejection::DuplicateTarget),
	);
	assert_eq!(stored_response(client, "session-duplicate").await?, duplicate_bytes);
	let mut conflicting_account = creation(
		"41000000-0000-4000-8000-000000000006",
		conversation_id,
		"43000000-0000-4000-8000-000000000001",
		"conflict",
	)?;
	conflicting_account.codex_thread_id = None;
	assert_eq!(
		store.create_runtime_session("session-account-conflict", &conflicting_account).await?,
		RuntimeSessionCommandOutcome::Rejected(RuntimeSessionRejection::AccountSnapshotConflict,),
	);

	let missing = creation(
		"41000000-0000-4000-8000-000000000003",
		&ConversationId::new("40000000-0000-4000-8000-000000000099")?,
		"43000000-0000-4000-8000-000000000003",
		"missing",
	)?;
	assert_eq!(
		store.create_runtime_session("session-missing", &missing).await?,
		RuntimeSessionCommandOutcome::Rejected(RuntimeSessionRejection::MissingTarget),
	);
	Ok(CreatedSessionFixture { request: create, effect: created })
}

async fn assert_transition_command(
	store: &PostgresStore,
	client: &Client,
	create: &CreateRuntimeSession,
) -> Result<(), Box<dyn std::error::Error>> {
	let transitioned = success(
		store
			.transition_runtime_session(
				"session-active",
				&create.runtime_session_id,
				1,
				RuntimeSessionState::Active,
			)
			.await?,
	);
	assert_eq!(transitioned.prior_state, Some(RuntimeSessionState::Starting));
	assert_eq!(transitioned.prior_revision, Some(1));
	assert_eq!(transitioned.new_revision, 2);
	let transition_bytes = stored_response(client, "session-active").await?;
	assert_eq!(
		store
			.transition_runtime_session(
				"session-active",
				&create.runtime_session_id,
				1,
				RuntimeSessionState::Active,
			)
			.await?,
		RuntimeSessionCommandOutcome::Success(transitioned),
	);
	assert_eq!(stored_response(client, "session-active").await?, transition_bytes);
	for (session, revision, state) in [
		(
			RuntimeSessionId::new("41000000-0000-4000-8000-000000000099")?,
			1,
			RuntimeSessionState::Active,
		),
		(create.runtime_session_id.clone(), 2, RuntimeSessionState::Active),
		(create.runtime_session_id.clone(), 1, RuntimeSessionState::Ended),
	] {
		assert!(matches!(
			store.transition_runtime_session("session-active", &session, revision, state).await,
			Err(StoreError::IdempotencyConflict)
		));
	}
	assert_eq!(
		store
			.transition_runtime_session(
				"session-stale",
				&create.runtime_session_id,
				1,
				RuntimeSessionState::Ended,
			)
			.await?,
		RuntimeSessionCommandOutcome::Rejected(RuntimeSessionRejection::StaleRevision),
	);
	assert_eq!(
		store
			.transition_runtime_session(
				"session-illegal",
				&create.runtime_session_id,
				2,
				RuntimeSessionState::Starting,
			)
			.await?,
		RuntimeSessionCommandOutcome::Rejected(RuntimeSessionRejection::IllegalTransition),
	);
	assert_eq!(
		store
			.transition_runtime_session(
				"session-transition-missing",
				&RuntimeSessionId::new("41000000-0000-4000-8000-000000000098")?,
				1,
				RuntimeSessionState::Active,
			)
			.await?,
		RuntimeSessionCommandOutcome::Rejected(RuntimeSessionRejection::MissingTarget),
	);
	Ok(())
}

async fn assert_profile_snapshot_race(
	store: &PostgresStore,
	client: &Client,
	conversation_id: &ConversationId,
	created: &RuntimeSessionCommandEffect,
) -> Result<(), Box<dyn std::error::Error>> {
	let before_update = created.runtime_session.profile_snapshot.clone();
	assert!(matches!(
		store.update_role_profile("task-v2", RoleProfileRole::Task, 1, &profile("task-v2")).await?,
		RoleProfileCommandOutcome::Success(_)
	));
	let after_update_request = creation(
		"41000000-0000-4000-8000-000000000004",
		conversation_id,
		"43000000-0000-4000-8000-000000000004",
		"four",
	)?;
	let after_update = success(
		store.create_runtime_session("session-after-profile-update", &after_update_request).await?,
	);
	assert_eq!(before_update.source_revision, 1);
	assert_eq!(after_update.runtime_session.profile_snapshot.source_revision, 2);
	let preserved: bool = client
		.query_one(
			"SELECT model=$2 AND instructions=$3 AND source_revision=1 \
			 FROM decodex.profile_snapshots WHERE profile_snapshot_id=$1::text::uuid",
			&[
				&before_update.profile_snapshot_id,
				&before_update.model,
				&before_update.instructions,
			],
		)
		.await?
		.get(0);
	assert!(preserved);

	let mut profile_race = JoinSet::new();
	for index in 0..24 {
		let store = store.clone();
		let conversation_id = conversation_id.clone();
		profile_race.spawn(async move {
			let identity = index + 100;
			let create = creation(
				&format!("41000000-0000-4000-8000-{identity:012x}"),
				&conversation_id,
				&format!("43000000-0000-4000-8000-{identity:012x}"),
				&format!("profile-race-{index}"),
			)?;
			store.create_runtime_session(&format!("session-profile-race-{index}"), &create).await
		});
	}
	let updating_store = store.clone();
	let profile_update = tokio::spawn(async move {
		updating_store
			.update_role_profile("task-v3", RoleProfileRole::Task, 2, &profile("task-v3"))
			.await
	});
	while let Some(result) = profile_race.join_next().await {
		let effect = success(result??);
		let snapshot = effect.runtime_session.profile_snapshot;
		match snapshot.source_revision {
			2 => {
				assert_eq!(snapshot.model, "gpt-5.6-task-v2");
				assert_eq!(snapshot.instructions, "Immutable XY-1337 task-v2 instructions.");
			},
			3 => {
				assert_eq!(snapshot.model, "gpt-5.6-task-v3");
				assert_eq!(snapshot.instructions, "Immutable XY-1337 task-v3 instructions.");
			},
			other => panic!("profile race selected unexpected revision {other}"),
		}
	}
	assert!(matches!(profile_update.await??, RoleProfileCommandOutcome::Success(_)));
	Ok(())
}

async fn assert_runtime_authority_denials(
	runtime: &tokio_postgres::Config,
) -> Result<(), Box<dyn std::error::Error>> {
	let (runtime_client, runtime_connection) = runtime.connect(NoTls).await?;
	let runtime_task = tokio::spawn(runtime_connection);
	for statement in [
		"INSERT INTO decodex.profile_snapshots(profile_snapshot_id,source_profile_id,role,model,reasoning_effort,service_tier,instructions_digest,instructions,source_revision) VALUES ('42000000-0000-4000-8000-000000000099','task','task','x','medium','priority',repeat('0',64),'x',1)",
		"UPDATE decodex.account_snapshots SET display_label='forged'",
		"DELETE FROM decodex.runtime_sessions",
		"TRUNCATE decodex.runtime_sessions",
		"SELECT decodex.complete_exact_runtime_session_rejection('x','x','x')",
		"INSERT INTO decodex.activity(aggregate_kind,aggregate_id,revision,event_kind,correlation_key,payload) VALUES ('runtime_session','forged',1,'runtime_session_created','forged','{}')",
		"INSERT INTO decodex.activity(aggregate_kind,aggregate_id,revision,event_kind,correlation_key,payload) VALUES ('conversation','forged',1,'conversation_recorded','forged','{\"event\":{\"aggregate_kind\":\"runtime_session\"}}')",
		"INSERT INTO decodex.activity(aggregate_kind,aggregate_id,revision,event_kind,correlation_key,payload) VALUES ('conversation','forged',1,'runtime_session_recorded','forged','{}')",
		"INSERT INTO decodex.outbox(effect_key,aggregate_kind,aggregate_id,aggregate_revision,payload) VALUES ('forged','runtime_session','forged',1,'{}')",
		"INSERT INTO decodex.outbox(effect_key,aggregate_kind,aggregate_id,aggregate_revision,payload) VALUES ('forged','conversation','forged',1,'{\"effect\":{\"payload\":{\"event_kind\":\"runtime_session_transitioned\"}}}')",
	] {
		let error = runtime_client.batch_execute(statement).await.expect_err(statement);
		assert_eq!(error.code().map(|code| code.code()), Some("42501"), "{statement}");
	}
	drop(runtime_client);
	runtime_task.await??;
	Ok(())
}

async fn assert_duplicate_creation_race(
	store: &PostgresStore,
	conversation_id: &ConversationId,
) -> Result<(), Box<dyn std::error::Error>> {
	let mut duplicate_race = JoinSet::new();
	let contested = creation(
		"41000000-0000-4000-8000-000000000005",
		conversation_id,
		"43000000-0000-4000-8000-000000000005",
		"five",
	)?;
	for index in 0..16 {
		let store = store.clone();
		let contested = contested.clone();
		duplicate_race.spawn(async move {
			store.create_runtime_session(&format!("session-race-{index}"), &contested).await
		});
	}
	let mut winners = 0;
	let mut duplicates = 0;
	while let Some(result) = duplicate_race.join_next().await {
		match result?? {
			RuntimeSessionCommandOutcome::Success(_) => winners += 1,
			RuntimeSessionCommandOutcome::Rejected(RuntimeSessionRejection::DuplicateTarget) =>
				duplicates += 1,
			other => panic!("unexpected duplicate-race result: {other:?}"),
		}
	}
	assert_eq!((winners, duplicates), (1, 15));
	Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "requires an isolated PostgreSQL 18 V10 RuntimeSession database"]
async fn postgres_exact_runtime_session_commands() -> Result<(), Box<dyn std::error::Error>> {
	let (migration, runtime) = separated_configs("DECODEX_TEST")?;
	let store =
		PostgresStore::connect(migration.clone(), runtime.clone(), expected_peer_uid()).await?;
	let (migration_client, migration_connection) = migration.connect(NoTls).await?;
	let migration_task = tokio::spawn(migration_connection);

	assert!(matches!(
		store.bootstrap_role_profiles("session-profiles", &profiles("v1")).await?,
		RoleProfileCommandOutcome::Success(_)
	));
	let conversation_id = ConversationId::new("40000000-0000-4000-8000-000000000001")?;
	store
		.create_conversation(
			&CommandIdentity::new("session-conversation", b"session-conversation-v1")?,
			&CreateConversation {
				conversation_id: conversation_id.clone(),
				title: "Exact RuntimeSession fixture".into(),
			},
		)
		.await?;

	let created = assert_creation_command(&store, &migration_client, &conversation_id).await?;
	assert_transition_command(&store, &migration_client, &created.request).await?;
	assert_profile_snapshot_race(&store, &migration_client, &conversation_id, &created.effect)
		.await?;
	assert_runtime_authority_denials(&runtime).await?;
	assert_duplicate_creation_race(&store, &conversation_id).await?;

	drop(migration_client);
	migration_task.await??;
	Ok(())
}

async fn runtime_state(client: &Client) -> Result<[i64; 6], tokio_postgres::Error> {
	let row = client
		.query_one(
			"SELECT \
			 (SELECT count(*) FROM decodex.exact_command_receipts), \
			 (SELECT count(*) FROM decodex.profile_snapshots), \
			 (SELECT count(*) FROM decodex.account_snapshots), \
			 (SELECT count(*) FROM decodex.runtime_sessions), \
			 (SELECT count(*) FROM decodex.activity WHERE aggregate_kind='runtime_session'), \
			 (SELECT count(*) FROM decodex.outbox WHERE aggregate_kind='runtime_session')",
			&[],
		)
		.await?;
	Ok(std::array::from_fn(|index| row.get(index)))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires an isolated PostgreSQL 18 V10 RuntimeSession rollback database"]
async fn postgres_exact_runtime_session_atomic_rollback() -> Result<(), Box<dyn std::error::Error>>
{
	let (migration, runtime) = separated_configs("DECODEX_TEST")?;
	let store = PostgresStore::connect(migration.clone(), runtime, expected_peer_uid()).await?;
	assert!(matches!(
		store.bootstrap_role_profiles("rollback-profiles", &profiles("rollback")).await?,
		RoleProfileCommandOutcome::Success(_)
	));
	let conversation_id = ConversationId::new("40000000-0000-4000-8000-000000000020")?;
	store
		.create_conversation(
			&CommandIdentity::new("rollback-conversation", b"rollback-conversation-v1")?,
			&CreateConversation {
				conversation_id: conversation_id.clone(),
				title: "RuntimeSession rollback fixture".into(),
			},
		)
		.await?;
	let (admin, admin_connection) = migration.connect(NoTls).await?;
	let admin_task = tokio::spawn(admin_connection);
	admin
		.batch_execute(
			"CREATE SEQUENCE public.xy1337_rollback_faults; \
			 CREATE TABLE public.xy1337_rollback_schedule(boundary text PRIMARY KEY); \
			 REVOKE ALL ON TABLE public.xy1337_rollback_schedule FROM PUBLIC; \
			 REVOKE ALL ON SEQUENCE public.xy1337_rollback_faults FROM PUBLIC; \
			 CREATE FUNCTION public.xy1337_raise_scheduled_runtime_fault() \
			 RETURNS trigger LANGUAGE plpgsql AS $$ \
			 BEGIN \
			   IF session_user <> current_user AND EXISTS ( \
			     SELECT 1 FROM public.xy1337_rollback_schedule WHERE boundary=TG_ARGV[0] \
			   ) THEN \
			     PERFORM pg_catalog.nextval('public.xy1337_rollback_faults'); \
			     RAISE EXCEPTION 'scheduled XY-1337 rollback fault' USING ERRCODE='XX000'; \
			   END IF; \
			   RETURN NEW; \
			 END $$; \
			 CREATE TRIGGER xy1337_fault_receipt AFTER INSERT ON decodex.exact_command_receipts \
			 FOR EACH ROW EXECUTE FUNCTION public.xy1337_raise_scheduled_runtime_fault('receipt'); \
			 CREATE TRIGGER xy1337_fault_domain AFTER INSERT ON decodex.runtime_sessions \
			 FOR EACH ROW EXECUTE FUNCTION public.xy1337_raise_scheduled_runtime_fault('domain'); \
			 CREATE TRIGGER xy1337_fault_activity AFTER INSERT ON decodex.activity \
			 FOR EACH ROW EXECUTE FUNCTION public.xy1337_raise_scheduled_runtime_fault('activity'); \
			 CREATE TRIGGER xy1337_fault_outbox AFTER INSERT ON decodex.outbox \
			 FOR EACH ROW EXECUTE FUNCTION public.xy1337_raise_scheduled_runtime_fault('outbox'); \
			 CREATE TRIGGER xy1337_fault_response AFTER UPDATE ON decodex.exact_command_receipts \
			 FOR EACH ROW WHEN (NEW.response_bytes IS NOT NULL) \
			 EXECUTE FUNCTION public.xy1337_raise_scheduled_runtime_fault('response'); \
			 REVOKE ALL ON FUNCTION public.xy1337_raise_scheduled_runtime_fault() FROM PUBLIC",
		)
		.await?;

	for (index, boundary) in
		["receipt", "domain", "activity", "outbox", "response"].into_iter().enumerate()
	{
		let identity = index + 30;
		let key = format!("runtime-rollback-{boundary}");
		let create = creation(
			&format!("41000000-0000-4000-8000-{identity:012x}"),
			&conversation_id,
			&format!("43000000-0000-4000-8000-{identity:012x}"),
			boundary,
		)?;
		admin
			.execute(
				"INSERT INTO public.xy1337_rollback_schedule(boundary) VALUES ($1)",
				&[&boundary],
			)
			.await?;
		let before = runtime_state(&admin).await?;
		assert!(matches!(
			store.create_runtime_session(&key, &create).await,
			Err(StoreError::Database(_))
		));
		assert_eq!(runtime_state(&admin).await?, before, "rollback at {boundary}");
		admin
			.execute("DELETE FROM public.xy1337_rollback_schedule WHERE boundary=$1", &[&boundary])
			.await?;
		assert!(matches!(
			store.create_runtime_session(&key, &create).await?,
			RuntimeSessionCommandOutcome::Success(_)
		));
	}
	assert_eq!(
		admin
			.query_one("SELECT last_value FROM public.xy1337_rollback_faults", &[])
			.await?
			.get::<_, i64>(0),
		5,
	);
	drop(admin);
	admin_task.await??;
	Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires an isolated PostgreSQL 18 V10 RuntimeSession retry database"]
async fn postgres_exact_runtime_session_retry_convergence() -> Result<(), Box<dyn std::error::Error>>
{
	let (migration, runtime) = separated_configs("DECODEX_TEST")?;
	let store = PostgresStore::connect(migration.clone(), runtime, expected_peer_uid()).await?;
	assert!(matches!(
		store.bootstrap_role_profiles("retry-profiles", &profiles("retry")).await?,
		RoleProfileCommandOutcome::Success(_)
	));
	let conversation_id = ConversationId::new("40000000-0000-4000-8000-000000000060")?;
	store
		.create_conversation(
			&CommandIdentity::new("retry-conversation", b"retry-conversation-v1")?,
			&CreateConversation {
				conversation_id: conversation_id.clone(),
				title: "RuntimeSession retry fixture".into(),
			},
		)
		.await?;
	let (admin, admin_connection) = migration.connect(NoTls).await?;
	let admin_task = tokio::spawn(admin_connection);
	admin
		.batch_execute(
			"CREATE SEQUENCE public.xy1337_retry_attempts; \
			 REVOKE ALL ON SEQUENCE public.xy1337_retry_attempts FROM PUBLIC; \
			 CREATE FUNCTION public.xy1337_schedule_runtime_retry() \
			 RETURNS trigger LANGUAGE plpgsql AS $$ \
			 DECLARE attempt bigint; \
			 BEGIN \
			   IF session_user <> current_user \
			      AND NEW.idempotency_key='runtime-serialization-retry' THEN \
			     attempt := pg_catalog.nextval('public.xy1337_retry_attempts'); \
			     IF attempt=1 THEN \
			       RAISE EXCEPTION 'scheduled serialization retry' USING ERRCODE='40001'; \
			     END IF; \
			   END IF; \
			   RETURN NEW; \
			 END $$; \
			 CREATE TRIGGER xy1337_retry_receipt BEFORE INSERT ON decodex.exact_command_receipts \
			 FOR EACH ROW EXECUTE FUNCTION public.xy1337_schedule_runtime_retry(); \
			 REVOKE ALL ON FUNCTION public.xy1337_schedule_runtime_retry() FROM PUBLIC",
		)
		.await?;
	let create = creation(
		"41000000-0000-4000-8000-000000000060",
		&conversation_id,
		"43000000-0000-4000-8000-000000000060",
		"retry",
	)?;
	assert!(matches!(
		store.create_runtime_session("runtime-serialization-retry", &create).await?,
		RuntimeSessionCommandOutcome::Success(_)
	));
	assert_eq!(
		admin
			.query_one("SELECT last_value FROM public.xy1337_retry_attempts", &[])
			.await?
			.get::<_, i64>(0),
		2,
	);
	assert_eq!(runtime_state(&admin).await?[1..], [1, 1, 1, 1, 1]);
	drop(admin);
	admin_task.await??;
	Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires an isolated PostgreSQL 18 V9 to V10 upgrade database"]
async fn postgres_v9_to_v10_runtime_session_upgrade() -> Result<(), Box<dyn std::error::Error>> {
	let (migration, _) = separated_configs("DECODEX_TEST")?;
	PostgresStore::migrate_fixture_through_v9(migration.clone(), expected_peer_uid()).await?;
	let (client, connection) = migration.clone().connect(NoTls).await?;
	let connection_task = tokio::spawn(connection);
	assert_eq!(
		client
			.query_one("SELECT max(version) FROM public.refinery_schema_history", &[])
			.await?
			.get::<_, i32>(0),
		9,
	);
	let identities_before: (u32, u32, u32) = {
		let row = client
			.query_one(
				"SELECT 'decodex.profile_snapshots'::regclass::oid, \
				 'decodex.account_snapshots'::regclass::oid, \
				 'decodex.runtime_sessions'::regclass::oid",
				&[],
			)
			.await?;
		(row.get(0), row.get(1), row.get(2))
	};
	drop(client);
	connection_task.await??;
	PostgresStore::migrate(migration.clone(), expected_peer_uid()).await?;
	let (client, connection) = migration.connect(NoTls).await?;
	let connection_task = tokio::spawn(connection);
	let row = client
		.query_one(
			"SELECT max(version), 'decodex.profile_snapshots'::regclass::oid, \
			 'decodex.account_snapshots'::regclass::oid, \
			 'decodex.runtime_sessions'::regclass::oid",
			&[],
		)
		.await?;
	assert_eq!(row.get::<_, i32>(0), 10);
	assert_eq!(
		(row.get::<_, u32>(1), row.get::<_, u32>(2), row.get::<_, u32>(3)),
		identities_before,
	);
	drop(client);
	connection_task.await??;
	Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires an isolated PostgreSQL 18 V10 zero-state rejection database"]
async fn postgres_v10_rejects_classified_runtime_state() -> Result<(), Box<dyn std::error::Error>> {
	let variant = env::var("DECODEX_V10_PRIOR_STATE")?;
	let (migration, _) = separated_configs("DECODEX_TEST")?;
	PostgresStore::migrate_fixture_through_v9(migration.clone(), expected_peer_uid()).await?;
	let (client, connection) = migration.clone().connect(NoTls).await?;
	let connection_task = tokio::spawn(connection);
	let statement = match variant.as_str() {
		"profile_snapshot" =>
			"INSERT INTO decodex.profile_snapshots(profile_snapshot_id,source_profile_id,role,model,reasoning_effort,service_tier,instructions_digest,source_revision) VALUES ('42000000-0000-4000-8000-000000000090','task','task','model','medium','priority',repeat('a',64),1)",
		"account_snapshot" =>
			"INSERT INTO decodex.account_snapshots(account_snapshot_id,source_account_id,display_label,observed_state,source_revision) VALUES ('43000000-0000-4000-8000-000000000090','13000000-0000-4000-8000-000000000090','Account','unknown',1)",
		"runtime_session" =>
			"INSERT INTO decodex.conversations(conversation_id,title) VALUES ('40000000-0000-4000-8000-000000000090','V9 state'); INSERT INTO decodex.profile_snapshots(profile_snapshot_id,source_profile_id,role,model,reasoning_effort,service_tier,instructions_digest,source_revision) VALUES ('42000000-0000-4000-8000-000000000090','task','task','model','medium','priority',repeat('a',64),1); INSERT INTO decodex.account_snapshots(account_snapshot_id,source_account_id,display_label,observed_state,source_revision) VALUES ('43000000-0000-4000-8000-000000000090','13000000-0000-4000-8000-000000000090','Account','unknown',1); INSERT INTO decodex.runtime_sessions(runtime_session_id,conversation_id,profile_snapshot_id,account_snapshot_id) VALUES ('41000000-0000-4000-8000-000000000090','40000000-0000-4000-8000-000000000090','42000000-0000-4000-8000-000000000090','43000000-0000-4000-8000-000000000090')",
		"legacy_receipt" =>
			"INSERT INTO decodex.command_receipts(idempotency_key,request_hash,operation,claim_token,claim_expires_at) VALUES ('v9-runtime',repeat('a',64),'create_runtime_session',gen_random_uuid(),clock_timestamp()+interval '1 minute')",
		"exact_receipt" =>
			"WITH request(value) AS (VALUES ('{\"operation\":\"create_runtime_session\"}'::jsonb)), effect(value) AS (VALUES ('{\"changed\":false,\"code\":\"missing_target\"}'::jsonb)) INSERT INTO decodex.exact_command_receipts(protocol_version,idempotency_key,request_envelope,request_digest,receipt_state,outcome_class,effect_envelope,response_bytes,completed_at) SELECT 'decodex/exact-command/1','v9-runtime',request.value,public.digest(convert_to(request.value::text,'UTF8'),'sha256'),'completed_rejected','stable_domain_rejection',effect.value,convert_to(jsonb_build_object('classification','stable_domain_rejection','code','missing_target','effect',effect.value)::text,'UTF8'),clock_timestamp() FROM request,effect",
		"activity" =>
			"INSERT INTO decodex.activity(aggregate_kind,aggregate_id,revision,event_kind,correlation_key,payload) VALUES ('runtime_session','v9',1,'runtime_session_recorded','v9-runtime','{}')",
		"activity_nested_aggregate" =>
			"INSERT INTO decodex.activity(aggregate_kind,aggregate_id,revision,event_kind,correlation_key,payload) VALUES ('conversation','v9',1,'conversation_recorded','v9-runtime','{\"event\":{\"aggregate_kind\":\"runtime_session\"}}')",
		"activity_legacy_other_aggregate" =>
			"INSERT INTO decodex.activity(aggregate_kind,aggregate_id,revision,event_kind,correlation_key,payload) VALUES ('conversation','v9',1,'runtime_session_recorded','v9-runtime','{}')",
		"outbox" =>
			"INSERT INTO decodex.outbox(effect_key,aggregate_kind,aggregate_id,aggregate_revision,payload) VALUES ('v9-runtime','runtime_session','v9',1,'{}')",
		"outbox_nested_effect" =>
			"INSERT INTO decodex.outbox(effect_key,aggregate_kind,aggregate_id,aggregate_revision,payload) VALUES ('v9-runtime','conversation','v9',1,'{\"effect\":{\"payload\":{\"event_kind\":\"runtime_session_transitioned\"}}}')",
		"outbox_activity_link" =>
			"WITH activity AS (INSERT INTO decodex.activity(aggregate_kind,aggregate_id,revision,event_kind,correlation_key,payload) VALUES ('conversation','v9',1,'runtime_session_recorded','v9-runtime','{}') RETURNING sequence) INSERT INTO decodex.outbox(effect_key,aggregate_kind,aggregate_id,aggregate_revision,payload) SELECT 'v9-runtime','conversation','v9',1,jsonb_build_object('link',jsonb_build_object('activity_sequence',sequence)) FROM activity",
		_ => return Err(format!("unknown V10 prior-state fixture {variant}").into()),
	};
	client.batch_execute(statement).await?;
	let error = PostgresStore::migrate(migration, expected_peer_uid())
		.await
		.expect_err("classified V9 RuntimeSession state must reject V10 atomically");
	assert!(format!("{error:?}").contains("zero incompatible state"));
	assert_eq!(
		client
			.query_one("SELECT max(version) FROM public.refinery_schema_history", &[])
			.await?
			.get::<_, i32>(0),
		9,
	);
	drop(client);
	connection_task.await??;
	Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires an isolated PostgreSQL 18 V10 old-writer fence database"]
async fn postgres_v10_fences_blocked_old_runtime_writer() -> Result<(), Box<dyn std::error::Error>>
{
	let (migration, runtime) = separated_configs("DECODEX_TEST")?;
	PostgresStore::migrate_fixture_through_v9(migration.clone(), expected_peer_uid()).await?;
	let runtime_role = runtime.get_user().ok_or("runtime role is absent")?;
	let (admin, admin_connection) = migration.clone().connect(NoTls).await?;
	let admin_task = tokio::spawn(admin_connection);
	admin
		.batch_execute(&format!(
			"GRANT USAGE ON SCHEMA decodex TO {runtime_role}; \
			 GRANT INSERT ON decodex.profile_snapshots TO {runtime_role}; \
			 GRANT USAGE ON TYPE decodex.role_profile_role TO {runtime_role}"
		))
		.await?;
	let (old_writer, old_connection) = runtime.connect(NoTls).await?;
	let old_connection_task = tokio::spawn(old_connection);
	old_writer
		.batch_execute(
			"PREPARE xy1337_old_writer AS \
			 INSERT INTO decodex.profile_snapshots( \
			  profile_snapshot_id,source_profile_id,role,model,reasoning_effort, \
			  service_tier,instructions_digest,source_revision \
			 ) VALUES ( \
			  '42000000-0000-4000-8000-000000000091','task','task','old', \
			  'medium','priority',repeat('a',64),1 \
			 )",
		)
		.await?;
	let old_pid: i32 = old_writer.query_one("SELECT pg_backend_pid()", &[]).await?.get(0);
	admin.query_one("SELECT pg_advisory_lock(991337)", &[]).await?;
	let writer = tokio::spawn(async move {
		old_writer
			.batch_execute(
				"BEGIN; SELECT pg_advisory_xact_lock(991337); \
				 EXECUTE xy1337_old_writer; COMMIT",
			)
			.await
	});
	for _ in 0..1_000 {
		let blocked: bool = admin
			.query_one(
				"SELECT wait_event_type='Lock' AND wait_event='advisory' \
				 FROM pg_stat_activity WHERE pid=$1",
				&[&old_pid],
			)
			.await?
			.get(0);
		if blocked {
			break;
		}
		time::sleep(std::time::Duration::from_millis(10)).await;
	}
	assert!(!writer.is_finished(), "old writer must be blocked before V10");
	PostgresStore::migrate(migration, expected_peer_uid()).await?;
	admin.query_one("SELECT pg_advisory_unlock(991337)", &[]).await?;
	let error = writer.await?.expect_err("pre-V10 writer must fail behind the committed fence");
	assert_eq!(error.code().map(|code| code.code()), Some("42501"));
	let fenced: bool = admin
		.query_one(
			"SELECT (SELECT max(version)=10 FROM public.refinery_schema_history) \
			 AND NOT EXISTS (SELECT 1 FROM decodex.profile_snapshots)",
			&[],
		)
		.await?
		.get(0);
	assert!(fenced);
	drop(admin);
	admin_task.await??;
	old_connection_task.await??;
	Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires an isolated PostgreSQL 18 V10 RuntimeSession crash database"]
async fn postgres_exact_runtime_session_crash_recovery() -> Result<(), Box<dyn std::error::Error>> {
	let sync = PathBuf::from(env::var("DECODEX_RUNTIME_SESSION_RESTART_SYNC")?);
	let (migration, runtime) = separated_configs("DECODEX_TEST")?;
	let store =
		PostgresStore::connect(migration.clone(), runtime.clone(), expected_peer_uid()).await?;
	assert!(matches!(
		store.bootstrap_role_profiles("crash-profiles", &profiles("crash")).await?,
		RoleProfileCommandOutcome::Success(_)
	));
	let conversation_id = ConversationId::new("40000000-0000-4000-8000-000000000070")?;
	store
		.create_conversation(
			&CommandIdentity::new("crash-conversation", b"crash-conversation-v1")?,
			&CreateConversation {
				conversation_id: conversation_id.clone(),
				title: "RuntimeSession crash fixture".into(),
			},
		)
		.await?;
	let create = creation(
		"41000000-0000-4000-8000-000000000070",
		&conversation_id,
		"43000000-0000-4000-8000-000000000070",
		"crash",
	)?;
	let (blocker, blocker_connection) = migration.clone().connect(NoTls).await?;
	let blocker_task = tokio::spawn(blocker_connection);
	blocker
		.batch_execute("BEGIN; LOCK TABLE decodex.runtime_sessions IN ACCESS EXCLUSIVE MODE")
		.await?;
	let blocker_pid: i32 = blocker.query_one("SELECT pg_backend_pid()", &[]).await?.get(0);
	let task_store = store.clone();
	let task_create = create.clone();
	let create_task = tokio::spawn(async move {
		task_store.create_runtime_session("crash-session", &task_create).await
	});
	let (observer, observer_connection) = migration.clone().connect(NoTls).await?;
	let observer_task = tokio::spawn(observer_connection);
	assert!(super::wait_for_any_blocked_by(&observer, blocker_pid).await?);
	std::fs::write(sync.join("ready"), b"ready")?;
	for _ in 0..3_000 {
		if sync.join("restarted").exists() {
			break;
		}
		time::sleep(std::time::Duration::from_millis(10)).await;
	}
	if !sync.join("restarted").exists() {
		return Err("PostgreSQL restart fixture did not signal recovery".into());
	}
	assert!(create_task.await?.is_err(), "precommit loss must not report success");
	drop(observer);
	drop(blocker);
	let _ = observer_task.await;
	let _ = blocker_task.await;
	drop(store);

	let recovered = PostgresStore::connect(migration.clone(), runtime, expected_peer_uid()).await?;
	assert!(matches!(
		recovered.create_runtime_session("crash-session", &create).await?,
		RuntimeSessionCommandOutcome::Success(_)
	));
	let (client, connection) = migration.connect(NoTls).await?;
	let connection_task = tokio::spawn(connection);
	let state = runtime_state(&client).await?;
	assert_eq!(state[1..], [1, 1, 1, 1, 1]);
	assert_eq!(
		client
			.query_one(
				"SELECT count(*) FROM decodex.exact_command_receipts \
				 WHERE idempotency_key='crash-session' AND receipt_state='completed_success'",
				&[],
			)
			.await?
			.get::<_, i64>(0),
		1,
	);
	drop(client);
	connection_task.await??;
	Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires the populated PostgreSQL 18 V10 RuntimeSession restore database"]
async fn postgres_exact_runtime_session_restore() -> Result<(), Box<dyn std::error::Error>> {
	let (migration, runtime) = separated_configs("DECODEX_TEST")?;
	let _store = PostgresStore::connect(migration.clone(), runtime, expected_peer_uid()).await?;
	let (client, connection) = migration.connect(NoTls).await?;
	let connection_task = tokio::spawn(connection);
	let valid: bool = client
		.query_one(
			"SELECT \
			 (SELECT max(version)=10 FROM public.refinery_schema_history) AND \
			 NOT EXISTS (SELECT 1 FROM decodex.exact_command_receipts \
			  WHERE receipt_state='executing' OR response_bytes IS NULL \
			  OR convert_from(response_bytes,'UTF8')::jsonb->'effect' IS DISTINCT FROM effect_envelope) AND \
			 NOT EXISTS (SELECT 1 FROM decodex.runtime_sessions AS session \
			  LEFT JOIN decodex.profile_snapshots AS profile USING (profile_snapshot_id) \
			  LEFT JOIN decodex.account_snapshots AS account USING (account_snapshot_id) \
			  WHERE profile.profile_snapshot_id IS NULL OR account.account_snapshot_id IS NULL)",
			&[],
		)
		.await?
		.get(0);
	assert!(valid);
	drop(client);
	connection_task.await??;
	Ok(())
}
