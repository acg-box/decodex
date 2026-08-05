use tokio::task::JoinSet;
use tokio_postgres::{Client, Config};

use super::expected_peer_uid;
use decodex_core::{
	AccountOperationId, AccountProvider, CodexCapability, ConversationId, CredentialBinding,
	CredentialFingerprint, CredentialStoreSchemaVersion, CredentialVersion, ExecutionConsumer,
	ManagedExecutionId, ManagedRunId, ProcessBootIdentity, ProcessControlKind,
	ProcessExecutionAuthorization, ProcessExecutionEpochId, ProcessGenerationAccountBinding,
	ProcessGenerationId, ProcessGenerationIntent, ProcessIdentity, ProcessIsolationKind,
	ProcessRunnerIdentity, ProcessStartIdentity, ProviderIdentity, RoutingCapabilityState,
	RoutingCommandOutcome, RoutingDecisionKind, RoutingMemberDisposition, RuntimeSessionId, TurnId,
};
use decodex_postgres::{
	AccountId, AccountState, CommandIdentity, CreateConversation, CreateRuntimeSession,
	CreateRuntimeSessionAccountSnapshot, PersistedRoutingDecision, PostgresStore,
	PrepareProcessGenerationOutcome, ProcessGenerationMutationOutcome, PublishRoutingEvidence,
	ReplaceRoutingPolicy, RoleProfileRole, RouteAccount, RoutingPolicyMemberInput,
	RuntimeSessionCommandOutcome, StoreError,
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
	"64a98c3328d1eba74aaf18a3995523e07fd2f1395bc6fb4a121b74338c404a29";
const CURRENT_CODEX_EXECUTABLE_SHA256: &str =
	"d96ae1ca1ff6fc8587842fa04c92d3ee4d31651a811c2f89b65fcfd9c28473e2";

#[derive(Clone)]
pub(super) struct RoutingFixture {
	pub selected: PersistedRoutingDecision,
	pub waiting: PersistedRoutingDecision,
	pub cancel_waiting: PersistedRoutingDecision,
	pub stale_waiting: PersistedRoutingDecision,
	pub selected_request: RouteAccount,
	pub selected_account_id: AccountId,
	pub selected_runtime_session_id: RuntimeSessionId,
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

	let restarted =
		PostgresStore::connect_runtime_fixture(runtime.clone(), expected_peer_uid()).await?;
	assert_eq!(
		restarted.route_account("v16-selected", &setup.selected_request).await?,
		RoutingCommandOutcome::Success(selected.clone()),
	);

	Ok(RoutingFixture {
		selected,
		waiting,
		cancel_waiting,
		stale_waiting,
		selected_request: setup.selected_request,
		selected_account_id: AccountId::new(SELECTED_ACCOUNT_ID)?,
		selected_runtime_session_id: setup.selected_run.runtime_session_id,
		stale_policy_id: STALE_POLICY_ID.to_owned(),
	})
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

	let selected_consumer = ExecutionConsumer::ConversationTurn {
		conversation_id: selected_run.conversation_id.clone(),
		conversation_revision: 1,
		source_runtime_session_id: Some(selected_run.runtime_session_id.clone()),
		source_runtime_session_revision: Some(1),
		turn_id: selected_run.turn_id.clone(),
	};
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
	let ExecutionConsumer::ConversationTurn {
		conversation_id,
		conversation_revision,
		source_runtime_session_id,
		source_runtime_session_revision,
		turn_id,
	} = &selected_request.consumer
	else {
		return Err("selected V16 fixture is not an ordinary Conversation Turn".into());
	};
	owner.batch_execute("BEGIN").await?;
	let rolled_back: Vec<u8> = owner
		.query_one(
			"SELECT decodex.route_account_exact('decodex/exact-command/1',\
			 'v16-rollback',$1::text::uuid,$2::text::uuid,1,'conversation_turn',\
			 $3::text::uuid,$4,$5::text::uuid,$6,$7::text::uuid,NULL,NULL,NULL)",
			&[
				&uuid(0xb6, 1),
				&SELECTED_POLICY_ID,
				&conversation_id.as_str(),
				conversation_revision,
				&source_runtime_session_id
					.as_ref()
					.expect("selected source runtime session is present")
					.as_str(),
				source_runtime_session_revision,
				&turn_id.as_str(),
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
	let thread_id = uuid(0xc3, marker);
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
				codex_thread_id: Some(thread_id.clone()),
				initial_state: decodex_core::RuntimeSessionState::Active,
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
	owner
		.execute(
			"INSERT INTO decodex.turns(\
			 turn_id,conversation_id,runtime_session_id,sequence,role)\
			 VALUES($1::text::uuid,$2::text::uuid,$3::text::uuid,1,'user')",
			&[&turn_id.as_str(), &conversation_id.as_str(), &runtime_session_id.as_str()],
		)
		.await?;
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
				build_identity: "codex-cli 0.146.0-alpha.9.2".to_owned(),
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
