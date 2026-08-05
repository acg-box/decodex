use std::{env, path::PathBuf, time::Duration};

use tokio::{task::JoinSet, time};
use tokio_postgres::{Client, Config, NoTls};

use super::{expected_peer_uid, owner_runtime_configs};
use decodex_postgres::{
	BootstrapRoleProfiles, PostgresStore, RoleProfileCommandOutcome, RoleProfileConfiguration,
	RoleProfileRejection, RoleProfileRevision, RoleProfileRole, StoreError,
};

fn configuration(marker: &str) -> RoleProfileConfiguration {
	RoleProfileConfiguration {
		model: format!("gpt-5.6-{marker}"),
		reasoning_effort: "medium".into(),
		service_tier: "priority".into(),
		instructions: format!("Own the exact {marker} role."),
		provenance: Some(format!("XY-1346 {marker}")),
	}
}

fn bootstrap() -> BootstrapRoleProfiles {
	BootstrapRoleProfiles {
		advisor: configuration("advisor"),
		lead: configuration("lead"),
		task: configuration("task"),
		reviewer: configuration("reviewer"),
	}
}

async fn role_profile_state(client: &Client) -> Result<[i64; 6], tokio_postgres::Error> {
	let row = client
		.query_one(
			"SELECT \
			 (SELECT count(*) FROM decodex.exact_command_receipts), \
			 (SELECT count(*) FROM decodex.role_profiles), \
			 (SELECT count(*) FROM decodex.role_profile_revisions), \
			 (SELECT count(*) FROM decodex.activity WHERE aggregate_kind='role_profile'), \
			 (SELECT count(*) FROM decodex.outbox WHERE aggregate_kind='role_profile'), \
			 (SELECT coalesce(sum(current_revision),0)::bigint FROM decodex.role_profiles)",
			&[],
		)
		.await?;

	Ok(std::array::from_fn(|index| row.get(index)))
}

async fn assert_execution_path_contract(
	runtime: &Config,
) -> Result<(), Box<dyn std::error::Error>> {
	let (probe, probe_connection) = runtime.clone().connect(NoTls).await?;
	let probe_task = tokio::spawn(probe_connection);
	let (execution_sql, allowed) = PostgresStore::execution_path_contract_fixture();
	let execution = probe.query_one(execution_sql, &[&allowed]).await?;
	let execution_contract = [
		execution.get::<_, bool>(0),
		execution.get::<_, bool>(1),
		execution.get::<_, bool>(2),
		execution.get::<_, bool>(3),
	];
	assert_eq!(execution_contract, [true; 4], "execution path closure");
	drop(probe);
	probe_task.await??;

	Ok(())
}

async fn exercise_role_profile_commands(
	store: &PostgresStore,
	profiles: &BootstrapRoleProfiles,
) -> Result<Vec<RoleProfileRevision>, Box<dyn std::error::Error>> {
	let RoleProfileCommandOutcome::Success(created) =
		store.bootstrap_role_profiles("role-bootstrap", profiles).await?
	else {
		panic!("first exact bootstrap must succeed");
	};
	assert_eq!(created.len(), 4);
	assert!(created.iter().all(|profile| profile.revision == 1));
	assert_eq!(
		store.bootstrap_role_profiles("role-bootstrap", profiles).await?,
		RoleProfileCommandOutcome::Success(created.clone()),
		"same request replays parsed stored bytes",
	);

	let mut conflicting = profiles.clone();
	conflicting.reviewer.instructions.push_str(" changed");
	assert!(matches!(
		store.bootstrap_role_profiles("role-bootstrap", &conflicting).await,
		Err(StoreError::IdempotencyConflict)
	));
	assert_eq!(
		store.bootstrap_role_profiles("second-bootstrap", profiles).await?,
		RoleProfileCommandOutcome::Rejected(RoleProfileRejection::AlreadyBootstrapped),
	);

	let updated_configuration = RoleProfileConfiguration {
		model: "gpt-5.6-sol".into(),
		reasoning_effort: "high".into(),
		service_tier: "priority".into(),
		instructions: "Review the exact frozen candidate read-only.".into(),
		provenance: None,
	};
	let RoleProfileCommandOutcome::Success(updated) = store
		.update_role_profile(
			"reviewer-update",
			RoleProfileRole::Reviewer,
			1,
			&updated_configuration,
		)
		.await?
	else {
		panic!("exact update must succeed");
	};
	assert_eq!(updated.role, RoleProfileRole::Reviewer);
	assert_eq!(updated.revision, 2);
	assert_eq!(updated.configuration, updated_configuration);
	assert_eq!(
		store
			.update_role_profile(
				"reviewer-update",
				RoleProfileRole::Reviewer,
				1,
				&updated_configuration,
			)
			.await?,
		RoleProfileCommandOutcome::Success(updated),
	);
	assert_eq!(
		store
			.update_role_profile(
				"reviewer-stale",
				RoleProfileRole::Reviewer,
				1,
				&updated_configuration,
			)
			.await?,
		RoleProfileCommandOutcome::Rejected(RoleProfileRejection::StaleRevision),
	);
	let mut credential_shaped = updated_configuration.clone();
	credential_shaped.service_tier = "sk_live_0123456789abcdef".into();
	assert!(matches!(
		store
			.update_role_profile(
				"credential-profile",
				RoleProfileRole::Reviewer,
				2,
				&credential_shaped,
			)
			.await,
		Err(StoreError::CredentialRejected)
	));

	Ok(created)
}

async fn assert_runtime_role_profile_restrictions(
	runtime: &Config,
) -> Result<(), Box<dyn std::error::Error>> {
	let (runtime_client, runtime_connection) = runtime.connect(NoTls).await?;
	let runtime_task = tokio::spawn(runtime_connection);
	for statement in [
		"SELECT * FROM decodex.exact_command_receipts",
		"SELECT * FROM decodex.role_profiles",
		"INSERT INTO decodex.activity(aggregate_kind,aggregate_id,revision,event_kind,correlation_key,payload) VALUES ('role_profile','advisor',9,'role_profile_updated','forged','{\"kind\":\"role_profile\"}')",
		"INSERT INTO decodex.activity(aggregate_kind,aggregate_id,revision,event_kind,correlation_key,payload) VALUES ('other','advisor',9,'role_profile_updated','forged-event','{}')",
		"INSERT INTO decodex.activity(aggregate_kind,aggregate_id,revision,event_kind,correlation_key,payload) VALUES ('other','advisor',9,'other','forged-structured','{\"aggregate_kind\":\"role_profile\",\"event_kind\":\"role_profile_updated\"}')",
		"INSERT INTO decodex.activity(aggregate_kind,aggregate_id,revision,event_kind,correlation_key,payload) VALUES ('other','advisor',9,'other','forged-payload','{\"role\":\"advisor\",\"model\":\"x\",\"reasoning_effort\":\"x\",\"service_tier\":\"x\",\"instructions\":\"x\",\"revision\":9}')",
		"INSERT INTO decodex.outbox(effect_key,aggregate_kind,aggregate_id,aggregate_revision,payload) VALUES ('forged-outbox','role_profile','advisor',9,'{}')",
		"INSERT INTO decodex.outbox(effect_key,aggregate_kind,aggregate_id,aggregate_revision,payload) VALUES ('forged-envelope','other','advisor',9,'{\"aggregate_kind\":\"role_profile\"}')",
		"SELECT decodex.complete_exact_role_profile_rejection('x','x','x')",
	] {
		let error = runtime_client.batch_execute(statement).await.expect_err(statement);
		assert_eq!(error.code().map(|code| code.code()), Some("42501"), "{statement}");
	}
	drop(runtime_client);
	runtime_task.await??;

	Ok(())
}

async fn assert_schema_owner_role_profile_invariants(
	schema_owner: &Config,
	store: &PostgresStore,
	profiles: &BootstrapRoleProfiles,
	created: Vec<RoleProfileRevision>,
) -> Result<(), Box<dyn std::error::Error>> {
	let (schema_owner_client, schema_owner_connection) = schema_owner.connect(NoTls).await?;
	let schema_owner_task = tokio::spawn(schema_owner_connection);
	let response_before: Vec<u8> = schema_owner_client
		.query_one(
			"SELECT response_bytes FROM decodex.exact_command_receipts \
			 WHERE protocol_version='decodex/exact-command/1' AND idempotency_key='role-bootstrap'",
			&[],
		)
		.await?
		.get(0);
	assert_eq!(
		store.bootstrap_role_profiles("role-bootstrap", profiles).await?,
		RoleProfileCommandOutcome::Success(created),
	);
	let response_after: Vec<u8> = schema_owner_client
		.query_one(
			"SELECT response_bytes FROM decodex.exact_command_receipts \
			 WHERE protocol_version='decodex/exact-command/1' AND idempotency_key='role-bootstrap'",
			&[],
		)
		.await?
		.get(0);
	assert_eq!(response_after, response_before, "replay retains byte-identical authority");

	for (statement, code) in [
		(
			"INSERT INTO decodex.exact_command_receipts( \
			 protocol_version,idempotency_key,request_envelope,request_digest,receipt_state, \
			 outcome_class,effect_envelope,response_bytes,created_at,completed_at) \
			 VALUES ('decodex/exact-command/1','owner-malformed-response','{\"operation\":\"hostile\"}', \
			 public.digest(convert_to('{\"operation\": \"hostile\"}'::jsonb::text,'UTF8'),'sha256'), \
			 'completed_success','success','{}',convert_to('{\"classification\":\"success\"}','UTF8'), \
			 statement_timestamp(),statement_timestamp())",
			"23514",
		),
		(
			"BEGIN; INSERT INTO decodex.exact_command_receipts( \
			 protocol_version,idempotency_key,request_envelope,request_digest,receipt_state) \
			 VALUES ('decodex/exact-command/1','owner-incomplete','{\"operation\":\"hostile\"}', \
			 public.digest(convert_to('{\"operation\": \"hostile\"}'::jsonb::text,'UTF8'),'sha256'),'executing'); COMMIT",
			"23514",
		),
		(
			"UPDATE decodex.exact_command_receipts SET response_bytes='\\x00' \
			 WHERE idempotency_key='role-bootstrap'",
			"23514",
		),
		(
			"DELETE FROM decodex.exact_command_receipts WHERE idempotency_key='role-bootstrap'",
			"23514",
		),
		("TRUNCATE decodex.exact_command_receipts", "23514"),
		(
			"BEGIN; UPDATE decodex.role_profiles SET current_revision=current_revision+1 \
			 WHERE role='advisor'; COMMIT",
			"23503",
		),
		("DELETE FROM decodex.role_profiles WHERE role='advisor'", "23514"),
		("UPDATE decodex.role_profile_revisions SET model='forged' WHERE role='advisor'", "23514"),
		("TRUNCATE decodex.role_profiles, decodex.role_profile_revisions CASCADE", "23514"),
	] {
		let error = schema_owner_client.batch_execute(statement).await.expect_err(statement);
		assert_eq!(error.code().map(|state| state.code()), Some(code), "{statement}");
	}
	let counts = schema_owner_client
		.query_one(
			"SELECT (SELECT count(*) FROM decodex.role_profiles), \
			 (SELECT count(*) FROM decodex.role_profile_revisions), \
			 (SELECT count(*) FROM decodex.exact_command_receipts WHERE receipt_state='executing'), \
			 (SELECT count(*) FROM decodex.activity WHERE aggregate_kind='role_profile'), \
			 (SELECT count(*) FROM decodex.outbox WHERE aggregate_kind='role_profile')",
			&[],
		)
		.await?;
	assert_eq!(counts.get::<_, i64>(0), 4);
	assert_eq!(counts.get::<_, i64>(1), 5);
	assert_eq!(counts.get::<_, i64>(2), 0);
	assert_eq!(counts.get::<_, i64>(3), 5);
	assert_eq!(counts.get::<_, i64>(4), 5);
	assert_eq!(
		schema_owner_client
			.query_one(
				"SELECT count(*) FROM decodex.exact_command_receipts \
				 WHERE idempotency_key='credential-profile'",
				&[],
			)
			.await?
			.get::<_, i64>(0),
		0,
		"credential-shaped requests never become receipt rows",
	);
	drop(schema_owner_client);
	schema_owner_task.await??;

	Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires an isolated PostgreSQL 18 V9 RoleProfile database"]
async fn postgres_exact_role_profile_commands() -> Result<(), Box<dyn std::error::Error>> {
	let (schema_owner, runtime) = owner_runtime_configs("DECODEX_TEST")?;
	assert_execution_path_contract(&runtime).await?;
	let store =
		PostgresStore::connect_runtime_fixture(runtime.clone(), expected_peer_uid()).await?;
	let profiles = bootstrap();
	let created = exercise_role_profile_commands(&store, &profiles).await?;
	assert_runtime_role_profile_restrictions(&runtime).await?;
	assert_schema_owner_role_profile_invariants(&schema_owner, &store, &profiles, created).await?;

	Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "requires an isolated PostgreSQL 18 V9 RoleProfile concurrency database"]
async fn postgres_exact_role_profile_concurrency() -> Result<(), Box<dyn std::error::Error>> {
	let (schema_owner, runtime) = owner_runtime_configs("DECODEX_TEST")?;
	let store =
		PostgresStore::connect_runtime_fixture(runtime.clone(), expected_peer_uid()).await?;
	let first = bootstrap();
	let mut second = first.clone();
	second.advisor.model = "gpt-5.6-advisor-alternate".into();

	let mut mixed = JoinSet::new();
	for index in 0..32 {
		let store = store.clone();
		let profiles = if index % 2 == 0 { first.clone() } else { second.clone() };
		mixed.spawn(
			async move { store.bootstrap_role_profiles("mixed-bootstrap", &profiles).await },
		);
	}
	let mut successes = 0;
	let mut conflicts = 0;
	while let Some(result) = mixed.join_next().await {
		match result? {
			Ok(RoleProfileCommandOutcome::Success(_)) => successes += 1,
			Err(StoreError::IdempotencyConflict) => conflicts += 1,
			Ok(RoleProfileCommandOutcome::Rejected(rejection)) => {
				panic!("mixed first bootstrap returned stable rejection: {rejection:?}")
			},
			Err(error) => return Err(error.into()),
		}
	}
	assert!(successes > 0);
	assert!(conflicts > 0);
	let (client, connection) = schema_owner.connect(NoTls).await?;
	let connection_task = tokio::spawn(connection);
	let winning_model: String = client
		.query_one(
			"SELECT revision.model FROM decodex.role_profiles AS profile \
			 JOIN decodex.role_profile_revisions AS revision \
			 ON revision.role=profile.role AND revision.revision=profile.current_revision \
			 WHERE profile.role='advisor'",
			&[],
		)
		.await?
		.get(0);
	let winner = if winning_model == first.advisor.model { first } else { second };

	let mut replay = JoinSet::new();
	for _ in 0..32 {
		let store = store.clone();
		let winner = winner.clone();
		replay
			.spawn(async move { store.bootstrap_role_profiles("mixed-bootstrap", &winner).await });
	}
	while let Some(result) = replay.join_next().await {
		assert!(matches!(result??, RoleProfileCommandOutcome::Success(_)));
		successes += 1;
	}
	let mut loser = winner.clone();
	loser.advisor.model.push_str("-conflict");
	for _ in 0..8 {
		if matches!(
			store.bootstrap_role_profiles("mixed-bootstrap", &loser).await,
			Err(StoreError::IdempotencyConflict)
		) {
			conflicts += 1;
		}
	}
	assert!(successes >= 32);
	assert!(conflicts >= 8);

	let update = RoleProfileConfiguration {
		model: "gpt-5.6-concurrent".into(),
		reasoning_effort: "high".into(),
		service_tier: "priority".into(),
		instructions: "Concurrent exact update.".into(),
		provenance: None,
	};
	let mut updates = JoinSet::new();
	for index in 0..32 {
		let store = store.clone();
		let update = update.clone();
		updates.spawn(async move {
			store
				.update_role_profile(
					&format!("concurrent-update-{index}"),
					RoleProfileRole::Task,
					1,
					&update,
				)
				.await
		});
	}
	let mut update_success = 0;
	let mut update_stale = 0;
	while let Some(result) = updates.join_next().await {
		match result?? {
			RoleProfileCommandOutcome::Success(_) => update_success += 1,
			RoleProfileCommandOutcome::Rejected(RoleProfileRejection::StaleRevision) =>
				update_stale += 1,
			other => panic!("unexpected concurrent update outcome: {other:?}"),
		}
	}
	assert_eq!((update_success, update_stale), (1, 31));
	let incomplete: i64 = client
		.query_one(
			"SELECT count(*) FROM decodex.exact_command_receipts WHERE receipt_state='executing'",
			&[],
		)
		.await?
		.get(0);
	assert_eq!(incomplete, 0);
	drop(client);
	connection_task.await??;

	Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires an isolated PostgreSQL 18 V9 RoleProfile rollback database"]
async fn postgres_exact_role_profile_atomic_rollback() -> Result<(), Box<dyn std::error::Error>> {
	let (schema_owner, runtime) = owner_runtime_configs("DECODEX_TEST")?;
	let store =
		PostgresStore::connect_runtime_fixture(runtime.clone(), expected_peer_uid()).await?;
	assert!(matches!(
		store.bootstrap_role_profiles("rollback-bootstrap", &bootstrap()).await?,
		RoleProfileCommandOutcome::Success(_)
	));
	let (admin, admin_connection) = schema_owner.connect(NoTls).await?;
	let admin_task = tokio::spawn(admin_connection);
	admin
		.batch_execute(
			"CREATE SEQUENCE public.xy1346_rollback_faults; \
			 CREATE TABLE public.xy1346_rollback_schedule(boundary text PRIMARY KEY); \
			 REVOKE ALL ON TABLE public.xy1346_rollback_schedule FROM PUBLIC; \
			 REVOKE ALL ON SEQUENCE public.xy1346_rollback_faults FROM PUBLIC; \
			 CREATE FUNCTION public.xy1346_raise_scheduled_role_profile_fault() \
			 RETURNS trigger LANGUAGE plpgsql AS $$ \
			 BEGIN \
			   IF session_user <> current_user AND EXISTS ( \
			     SELECT 1 FROM public.xy1346_rollback_schedule WHERE boundary=TG_ARGV[0] \
			   ) THEN \
			     PERFORM pg_catalog.nextval('public.xy1346_rollback_faults'); \
			     RAISE EXCEPTION 'scheduled XY-1346 rollback fault at %', TG_ARGV[0] \
			       USING ERRCODE='XX000'; \
			   END IF; \
			   RETURN NEW; \
			 END $$; \
			 CREATE TRIGGER xy1346_fault_receipt AFTER INSERT ON decodex.exact_command_receipts \
			 FOR EACH ROW EXECUTE FUNCTION public.xy1346_raise_scheduled_role_profile_fault('receipt'); \
			 CREATE TRIGGER xy1346_fault_domain AFTER INSERT ON decodex.role_profile_revisions \
			 FOR EACH ROW EXECUTE FUNCTION public.xy1346_raise_scheduled_role_profile_fault('domain'); \
			 CREATE TRIGGER xy1346_fault_activity AFTER INSERT ON decodex.activity \
			 FOR EACH ROW EXECUTE FUNCTION public.xy1346_raise_scheduled_role_profile_fault('activity'); \
			 CREATE TRIGGER xy1346_fault_outbox AFTER INSERT ON decodex.outbox \
			 FOR EACH ROW EXECUTE FUNCTION public.xy1346_raise_scheduled_role_profile_fault('outbox'); \
			 REVOKE ALL ON FUNCTION public.xy1346_raise_scheduled_role_profile_fault() FROM PUBLIC",
		)
		.await?;

	for (boundary, role) in [
		("receipt", RoleProfileRole::Advisor),
		("domain", RoleProfileRole::Lead),
		("activity", RoleProfileRole::Task),
		("outbox", RoleProfileRole::Reviewer),
	] {
		let key = format!("rollback-{boundary}");
		let configuration = configuration(&key);
		admin
			.execute(
				"INSERT INTO public.xy1346_rollback_schedule(boundary) VALUES ($1)",
				&[&boundary],
			)
			.await?;
		let before = role_profile_state(&admin).await?;
		assert!(matches!(
			store.update_role_profile(&key, role, 1, &configuration).await,
			Err(StoreError::Database(_))
		));
		assert_eq!(role_profile_state(&admin).await?, before, "rollback at {boundary}");
		admin
			.execute("DELETE FROM public.xy1346_rollback_schedule WHERE boundary=$1", &[&boundary])
			.await?;
		let RoleProfileCommandOutcome::Success(revision) =
			store.update_role_profile(&key, role, 1, &configuration).await?
		else {
			panic!("rollback retry at {boundary} must converge");
		};
		assert_eq!(revision.revision, 2);
	}
	assert_eq!(
		admin
			.query_one("SELECT last_value FROM public.xy1346_rollback_faults", &[])
			.await?
			.get::<_, i64>(0),
		4,
	);
	drop(admin);
	admin_task.await??;
	Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
#[ignore = "requires an isolated PostgreSQL 18 V9 RoleProfile retry database"]
async fn postgres_exact_role_profile_retry_convergence() -> Result<(), Box<dyn std::error::Error>> {
	let (schema_owner, runtime) = owner_runtime_configs("DECODEX_TEST")?;
	let store =
		PostgresStore::connect_runtime_fixture(runtime.clone(), expected_peer_uid()).await?;
	assert!(matches!(
		store.bootstrap_role_profiles("retry-bootstrap", &bootstrap()).await?,
		RoleProfileCommandOutcome::Success(_)
	));
	let (admin, admin_connection) = schema_owner.connect(NoTls).await?;
	let admin_task = tokio::spawn(admin_connection);
	admin
		.batch_execute(
			"CREATE SEQUENCE public.xy1346_serialization_attempts; \
			 CREATE SEQUENCE public.xy1346_deadlock_attempts; \
			 REVOKE ALL ON SEQUENCE public.xy1346_serialization_attempts, \
			   public.xy1346_deadlock_attempts FROM PUBLIC; \
			 CREATE FUNCTION public.xy1346_schedule_retryable_role_profile_fault() \
			 RETURNS trigger LANGUAGE plpgsql AS $$ \
			 DECLARE attempt bigint; \
			 BEGIN \
			   IF session_user <> current_user AND NEW.idempotency_key='serialization-retry' THEN \
			     attempt := pg_catalog.nextval('public.xy1346_serialization_attempts'); \
			     IF attempt=1 THEN \
			       RAISE EXCEPTION 'scheduled serialization retry' USING ERRCODE='40001'; \
			     END IF; \
			   ELSIF session_user <> current_user AND NEW.idempotency_key='deadlock-retry' THEN \
			     PERFORM pg_catalog.nextval('public.xy1346_deadlock_attempts'); \
			   END IF; \
			   RETURN NEW; \
			 END $$; \
			 CREATE TRIGGER xy1346_retryable_receipt_fault \
			 BEFORE INSERT ON decodex.exact_command_receipts FOR EACH ROW \
			 EXECUTE FUNCTION public.xy1346_schedule_retryable_role_profile_fault(); \
			 REVOKE ALL ON FUNCTION public.xy1346_schedule_retryable_role_profile_fault() FROM PUBLIC",
		)
		.await?;

	let RoleProfileCommandOutcome::Success(serialized) = store
		.update_role_profile(
			"serialization-retry",
			RoleProfileRole::Advisor,
			1,
			&configuration("serialization-retry"),
		)
		.await?
	else {
		panic!("serialization retry must converge");
	};
	assert_eq!(serialized.revision, 2);
	assert_eq!(
		admin
			.query_one("SELECT last_value FROM public.xy1346_serialization_attempts", &[])
			.await?
			.get::<_, i64>(0),
		2,
	);

	admin
		.batch_execute("BEGIN; SELECT role FROM decodex.role_profiles WHERE role='lead' FOR UPDATE")
		.await?;
	let blocker_pid: i32 = admin.query_one("SELECT pg_backend_pid()", &[]).await?.get(0);
	let deadlock_store = store.clone();
	let command_task = tokio::spawn(async move {
		deadlock_store
			.update_role_profile(
				"deadlock-retry",
				RoleProfileRole::Lead,
				1,
				&configuration("deadlock-retry"),
			)
			.await
	});
	let (observer, observer_connection) = schema_owner.connect(NoTls).await?;
	let observer_task = tokio::spawn(observer_connection);
	assert!(super::wait_for_any_blocked_by(&observer, blocker_pid).await?);
	drop(observer);
	observer_task.await??;

	let antagonist = tokio::spawn(async move {
		admin
			.batch_execute(
				"WITH request(value) AS (VALUES ('{\"operation\":\"deadlock-antagonist\"}'::jsonb)) \
				 INSERT INTO decodex.exact_command_receipts( \
				   protocol_version,idempotency_key,request_envelope,request_digest,receipt_state \
				 ) SELECT 'decodex/exact-command/1','deadlock-retry',value, \
				   public.digest(convert_to(value::text,'UTF8'),'sha256'),'executing' FROM request; \
				 ROLLBACK",
			)
			.await
	});
	let RoleProfileCommandOutcome::Success(deadlocked) =
		time::timeout(Duration::from_secs(20), command_task).await???
	else {
		panic!("deadlock retry must converge");
	};
	assert_eq!(deadlocked.revision, 2);
	time::timeout(Duration::from_secs(20), antagonist).await???;
	admin_task.await??;

	let (check, check_connection) = schema_owner.connect(NoTls).await?;
	let check_task = tokio::spawn(check_connection);
	assert!(
		check
			.query_one("SELECT last_value FROM public.xy1346_deadlock_attempts", &[])
			.await?
			.get::<_, i64>(0)
			>= 2,
		"the production command must retry after the executed deadlock",
	);
	assert_eq!(
		check
			.query_one(
				"SELECT count(*) FROM decodex.exact_command_receipts \
				 WHERE idempotency_key IN ('serialization-retry','deadlock-retry') \
				 AND receipt_state='completed_success'",
				&[],
			)
			.await?
			.get::<_, i64>(0),
		2,
	);
	drop(check);
	check_task.await??;
	Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires the isolated PostgreSQL 18 RoleProfile crash/recovery database"]
async fn postgres_exact_role_profile_crash_recovery() -> Result<(), Box<dyn std::error::Error>> {
	let sync = PathBuf::from(env::var("DECODEX_ROLE_PROFILE_RESTART_SYNC")?);
	let (schema_owner, runtime) = owner_runtime_configs("DECODEX_TEST")?;
	let store =
		PostgresStore::connect_runtime_fixture(runtime.clone(), expected_peer_uid()).await?;
	let profiles = bootstrap();
	assert!(matches!(
		store.bootstrap_role_profiles("crash-bootstrap", &profiles).await?,
		RoleProfileCommandOutcome::Success(_)
	));
	let (blocker, blocker_connection) = schema_owner.clone().connect(NoTls).await?;
	let blocker_task = tokio::spawn(blocker_connection);
	blocker
		.batch_execute("BEGIN; LOCK TABLE decodex.role_profiles IN ACCESS EXCLUSIVE MODE")
		.await?;
	let update = configuration("crash-update");
	let task_store = store.clone();
	let update_task = tokio::spawn(async move {
		task_store.update_role_profile("crash-update", RoleProfileRole::Lead, 1, &update).await
	});
	let blocker_pid: i32 = blocker.query_one("SELECT pg_backend_pid()", &[]).await?.get(0);
	let (observer, observer_connection) = schema_owner.clone().connect(NoTls).await?;
	let observer_task = tokio::spawn(observer_connection);
	assert!(super::wait_for_any_blocked_by(&observer, blocker_pid).await?);
	std::fs::write(sync.join("ready"), b"ready")?;
	for _ in 0..3_000 {
		if sync.join("restarted").exists() {
			break;
		}
		time::sleep(Duration::from_millis(10)).await;
	}
	if !sync.join("restarted").exists() {
		return Err("PostgreSQL restart fixture did not signal recovery".into());
	}
	assert!(update_task.await?.is_err(), "precommit connection loss must not report success");
	drop(observer);
	drop(blocker);
	let _ = observer_task.await;
	let _ = blocker_task.await;
	drop(store);

	let recovered =
		PostgresStore::connect_runtime_fixture(runtime.clone(), expected_peer_uid()).await?;
	assert!(matches!(
		recovered
			.update_role_profile(
				"crash-update",
				RoleProfileRole::Lead,
				1,
				&configuration("crash-update"),
			)
			.await?,
		RoleProfileCommandOutcome::Success(_)
	));
	let (client, connection) = schema_owner.connect(NoTls).await?;
	let connection_task = tokio::spawn(connection);
	let state = client
		.query_one(
			"SELECT \
			 (SELECT count(*) FROM decodex.exact_command_receipts WHERE receipt_state='executing'), \
			 (SELECT count(*) FROM decodex.role_profile_revisions WHERE role='lead' AND revision=2), \
			 (SELECT count(*) FROM decodex.activity WHERE aggregate_kind='role_profile' AND aggregate_id='lead' AND revision=2), \
			 (SELECT count(*) FROM decodex.outbox WHERE aggregate_kind='role_profile' AND aggregate_id='lead' AND aggregate_revision=2)",
			&[],
		)
		.await?;
	assert_eq!(
		[
			state.get::<_, i64>(0),
			state.get::<_, i64>(1),
			state.get::<_, i64>(2),
			state.get::<_, i64>(3),
		],
		[0, 1, 1, 1],
	);
	drop(client);
	connection_task.await??;
	Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires the populated PostgreSQL 18 V9 RoleProfile restore database"]
async fn postgres_exact_role_profile_restore() -> Result<(), Box<dyn std::error::Error>> {
	let (schema_owner, runtime) = owner_runtime_configs("DECODEX_TEST")?;
	let store =
		PostgresStore::connect_runtime_fixture(runtime.clone(), expected_peer_uid()).await?;
	assert!(matches!(
		store.bootstrap_role_profiles("role-bootstrap", &bootstrap()).await?,
		RoleProfileCommandOutcome::Success(_)
	));
	let (client, connection) = schema_owner.connect(NoTls).await?;
	let connection_task = tokio::spawn(connection);
	let valid: bool = client
		.query_one(
			"SELECT \
			 (SELECT count(*)=4 FROM decodex.role_profiles) AND \
			 (SELECT count(*)=5 FROM decodex.role_profile_revisions) AND \
			 (SELECT count(*)=0 FROM decodex.exact_command_receipts WHERE receipt_state='executing') AND \
			 NOT EXISTS (SELECT 1 FROM decodex.exact_command_receipts \
			  WHERE response_bytes IS NULL OR convert_from(response_bytes,'UTF8')::jsonb->'effect' IS DISTINCT FROM effect_envelope)",
			&[],
		)
		.await?
		.get(0);
	assert!(valid);
	drop(client);
	connection_task.await??;
	Ok(())
}
