use std::time::Duration;

use tokio::task::JoinSet;
use tokio_postgres::{NoTls, error::SqlState};

use super::{expected_peer_uid, owner_runtime_configs};
use decodex_core::{
	AgentId, ProjectId, WorkItemCorrelationId, WorkItemId, WorkItemPriority, WorkItemProvenance,
	WorkItemState,
};
use decodex_postgres::{
	AcceptWorkItem, CreateWorkItem, OutboxReconciliation, PostgresStore, ReconciliationOutcome,
	UpdateWorkItem, WorkItemCommandOutcome, WorkItemReadinessBlockerKind, WorkItemRejection,
	WorkItemRelations,
};

const PROJECT_ID: &str = "21000000-0000-4000-8000-000000001343";
const LEAD_ID: &str = "31000000-0000-4000-8000-000000001343";
const OUTBOX_WORKER_ID: &str = "81000000-0000-4000-8000-000000001343";

fn work_item_id(value: u8) -> WorkItemId {
	WorkItemId::new(format!("51000000-0000-4000-8000-{value:012}"))
		.expect("fixture WorkItem ID is canonical")
}

fn provenance(value: u8) -> WorkItemProvenance {
	WorkItemProvenance::new(
		AgentId::new(LEAD_ID).expect("fixture Lead ID is canonical"),
		WorkItemCorrelationId::new(format!("61000000-0000-4000-8000-{value:012}"))
			.expect("fixture correlation ID is canonical"),
		format!("XY-1343 WorkItem fixture {value}"),
	)
	.expect("fixture provenance is bounded")
}

fn create(value: u8, relations: WorkItemRelations) -> CreateWorkItem {
	CreateWorkItem {
		work_item_id: work_item_id(value),
		project_id: ProjectId::new(PROJECT_ID).expect("fixture Project ID is canonical"),
		lead_agent_id: AgentId::new(LEAD_ID).expect("fixture Lead ID is canonical"),
		program_id: None,
		relations,
		title: format!("WorkItem {value}"),
		description: "Transactional PostgreSQL WorkItem authority fixture.".into(),
		priority: WorkItemPriority::High,
		acceptance_criteria: vec!["Canonical state is persisted.".into()],
		validation_criteria: vec!["Exact effects are auditable.".into()],
		provenance: provenance(value),
	}
}

fn update(
	value: u8,
	expected_revision: u64,
	target_state: WorkItemState,
	relations: WorkItemRelations,
) -> UpdateWorkItem {
	let created = create(value, relations.clone());
	UpdateWorkItem {
		work_item_id: created.work_item_id,
		project_id: created.project_id,
		expected_revision,
		program_id: None,
		relations,
		title: created.title,
		description: created.description,
		priority: created.priority,
		acceptance_criteria: created.acceptance_criteria,
		validation_criteria: created.validation_criteria,
		target_state,
		provenance: provenance(value.saturating_add(20)),
	}
}

async fn create_project(
	schema_owner: &tokio_postgres::Config,
) -> Result<(), Box<dyn std::error::Error>> {
	let (client, connection) = schema_owner.connect(NoTls).await?;
	let task = tokio::spawn(connection);
	client
		.query_one(
			"SELECT project_id FROM decodex.create_project(\
			 $1::text::decodex.canonical_uuid_v4_text,$2,$3,$3,'{}'::jsonb,\
			 $4::text::decodex.canonical_uuid_v4_text)",
			&[&PROJECT_ID, &"xy-1343/work-items", &"/srv/xy-1343-work-items", &LEAD_ID],
		)
		.await?;
	drop(client);
	task.await??;
	Ok(())
}

fn success(outcome: WorkItemCommandOutcome) -> Box<decodex_postgres::WorkItemCommandEffect> {
	match outcome {
		WorkItemCommandOutcome::Success(effect) => effect,
		WorkItemCommandOutcome::Rejected(rejection) => {
			panic!("unexpected rejection: {rejection:?}")
		},
	}
}

fn postgres_error_diagnostic(error: &tokio_postgres::Error) -> serde_json::Value {
	let database_error = error.as_db_error();
	serde_json::json!({
		"sqlstate": database_error.map(|error| error.code().code()),
		"message": database_error.map_or_else(|| error.to_string(), |error| error.message().into()),
		"constraint": database_error.and_then(|error| error.constraint()),
		"schema": database_error.and_then(|error| error.schema()),
		"table": database_error.and_then(|error| error.table()),
		"column": database_error.and_then(|error| error.column()),
		"detail": database_error.and_then(|error| error.detail()),
		"hint": database_error.and_then(|error| error.hint()),
		"where_context": database_error.and_then(|error| error.where_()),
	})
}

async fn exercise_outbox_authority_case(
	runtime: &tokio_postgres::Config,
	label: &str,
	sql: &str,
) -> (bool, serde_json::Value) {
	let mut begin_status = "not_attempted".to_owned();
	let mut begin_error = serde_json::Value::Null;
	let mut execution_outcome = "not_attempted".to_owned();
	let mut execution_error = serde_json::Value::Null;
	let mut execution_matches = false;
	let mut rollback_status = "not_attempted".to_owned();
	let mut rollback_error = serde_json::Value::Null;
	let (connection_completion_status, connection_error) = match runtime.connect(NoTls).await {
		Err(error) => ("connect_failed".to_owned(), postgres_error_diagnostic(&error)),
		Ok((client, connection)) => {
			let connection_task = tokio::spawn(connection);
			match client.batch_execute("BEGIN").await {
				Ok(()) => begin_status = "began".into(),
				Err(error) => {
					begin_status = "begin_failed".into();
					begin_error = postgres_error_diagnostic(&error);
				},
			}
			if begin_status == "began" {
				match client.batch_execute(sql).await {
					Ok(()) => execution_outcome = "unexpected_success".into(),
					Err(error) => {
						execution_outcome = if error.as_db_error().is_some() {
							"server_error".into()
						} else {
							"non_database_error".into()
						};
						execution_matches = error.as_db_error().is_some_and(|error| {
							error.code() == &SqlState::INSUFFICIENT_PRIVILEGE
								&& error.constraint() == Some("work_item_event_namespace")
						});
						execution_error = postgres_error_diagnostic(&error);
					},
				}
			}
			match client.batch_execute("ROLLBACK").await {
				Ok(()) => rollback_status = "rolled_back".into(),
				Err(error) => {
					rollback_status = "rollback_failed".into();
					rollback_error = postgres_error_diagnostic(&error);
				},
			}
			drop(client);
			match connection_task.await {
				Ok(Ok(())) => ("completed".to_owned(), serde_json::Value::Null),
				Ok(Err(error)) =>
					("connection_failed".to_owned(), postgres_error_diagnostic(&error)),
				Err(error) => (
					"connection_task_failed".to_owned(),
					serde_json::json!({"message": error.to_string()}),
				),
			}
		},
	};

	let matched = begin_status == "began"
		&& execution_outcome == "server_error"
		&& execution_matches
		&& rollback_status == "rolled_back"
		&& connection_completion_status == "completed";
	let diagnostic = serde_json::json!({
		"label": label,
		"sql": sql,
		"assignment": sql.split_once(" SET ").map(|(_, tail)| tail),
		"execution_outcome": execution_outcome,
		"sqlstate": execution_error.get("sqlstate").cloned().unwrap_or(serde_json::Value::Null),
		"message": execution_error.get("message").cloned().unwrap_or(serde_json::Value::Null),
		"constraint": execution_error.get("constraint").cloned().unwrap_or(serde_json::Value::Null),
		"schema": execution_error.get("schema").cloned().unwrap_or(serde_json::Value::Null),
		"table": execution_error.get("table").cloned().unwrap_or(serde_json::Value::Null),
		"column": execution_error.get("column").cloned().unwrap_or(serde_json::Value::Null),
		"detail": execution_error.get("detail").cloned().unwrap_or(serde_json::Value::Null),
		"hint": execution_error.get("hint").cloned().unwrap_or(serde_json::Value::Null),
		"where_context": execution_error.get("where_context").cloned().unwrap_or(serde_json::Value::Null),
		"transaction_begin_status": begin_status,
		"transaction_begin_error": begin_error,
		"rollback_status": rollback_status,
		"rollback_error": rollback_error,
		"connection_completion_status": connection_completion_status,
		"connection_error": connection_error,
		"matches_expected_authority_failure": matched,
	});
	(matched, diagnostic)
}

async fn deliver_work_item_outbox(
	store: &PostgresStore,
	owner: &tokio_postgres::Client,
) -> Result<i64, Box<dyn std::error::Error>> {
	let protected_id = owner
		.query_one("SELECT min(id) FROM decodex.outbox WHERE aggregate_kind='work_item'", &[])
		.await?
		.get::<_, Option<i64>>(0)
		.expect("WorkItem command must emit an outbox row");
	owner
		.execute(
			"UPDATE decodex.outbox SET available_at = CASE WHEN id=$1 THEN clock_timestamp() \
			 ELSE clock_timestamp() + interval '1 hour' END",
			&[&protected_id],
		)
		.await?;

	let claim = store
		.claim_outbox(OUTBOX_WORKER_ID, 1, Duration::from_secs(1))
		.await?
		.pop()
		.expect("the selected WorkItem outbox row must be claimable");
	assert_eq!(claim.id, protected_id);
	store.begin_outbox_effect(claim.id, OUTBOX_WORKER_ID, &claim.claim_token).await?;
	store
		.record_outbox_receipt(
			claim.id,
			OUTBOX_WORKER_ID,
			&claim.claim_token,
			&serde_json::json!({"provider_receipt": "xy-1343"}),
		)
		.await?;
	store
		.reconcile_outbox(
			claim.id,
			OUTBOX_WORKER_ID,
			&claim.claim_token,
			&OutboxReconciliation {
				readback: serde_json::json!({"effect_key": claim.effect_key, "observed": true}),
				outcome: ReconciliationOutcome::EffectPresent,
			},
			Duration::from_millis(1),
			Duration::from_secs(1),
		)
		.await?;
	let state: String = owner
		.query_one("SELECT state::text FROM decodex.outbox WHERE id=$1", &[&claim.id])
		.await?
		.get(0);
	assert_eq!(state, "delivered");
	Ok(claim.id)
}

async fn select_work_item_outbox_authority_rows(
	owner: &tokio_postgres::Client,
	delivered_id: i64,
) -> Result<(i64, i64), Box<dyn std::error::Error>> {
	let authority_id = owner
		.query_one(
			"SELECT min(id) FROM decodex.outbox \
			 WHERE aggregate_kind='work_item' AND id<>$1 \
			 AND state='pending' AND effect_state='not_started' \
			 AND lease_holder IS NULL AND claim_token IS NULL \
			 AND lease_acquired_at IS NULL AND lease_expires_at IS NULL \
			 AND delivered_at IS NULL AND dead_lettered_at IS NULL",
			&[&delivered_id],
		)
		.await?
		.get::<_, Option<i64>>(0)
		.expect("an undelivered pending WorkItem outbox row must exist");

	let generic_id: i64 = owner
		.query_one(
			"INSERT INTO decodex.outbox( \
			 effect_key,aggregate_kind,aggregate_id,aggregate_revision,payload,available_at \
			 ) VALUES ('xy-1343-generic-outbox','generic','generic-1',1, \
			 '{\"fixture\":\"generic\"}'::jsonb,clock_timestamp()+interval '1 hour') \
			 RETURNING id",
			&[],
		)
		.await?
		.get(0);
	Ok((authority_id, generic_id))
}

fn work_item_outbox_authority_cases(
	authority_id: i64,
	generic_id: i64,
) -> Vec<(&'static str, String)> {
	let mut cases = vec![
		(
			"effect_key",
			format!(
				"UPDATE decodex.outbox SET effect_key=effect_key||'/rewrite' WHERE id={authority_id}"
			),
		),
		(
			"aggregate_kind_old_linked_escape",
			format!("UPDATE decodex.outbox SET aggregate_kind='other' WHERE id={authority_id}"),
		),
		(
			"aggregate_id",
			format!("UPDATE decodex.outbox SET aggregate_id='rewritten' WHERE id={authority_id}"),
		),
		(
			"aggregate_revision",
			format!(
				"UPDATE decodex.outbox SET aggregate_revision=aggregate_revision+1 WHERE id={authority_id}"
			),
		),
		(
			"payload_old_linked_escape",
			format!(
				"UPDATE decodex.outbox SET payload='{{\"kind\":\"other\"}}'::jsonb WHERE id={authority_id}"
			),
		),
		(
			"created_at",
			format!(
				"UPDATE decodex.outbox SET created_at=created_at-interval '1 microsecond' WHERE id={authority_id}"
			),
		),
		(
			"generic_to_work_item_new_linked_escape",
			format!(
				"UPDATE decodex.outbox SET aggregate_kind='work_item',aggregate_id='{}', \
				 payload=jsonb_build_object('work_item_id','{}') WHERE id={generic_id}",
				work_item_id(1),
				work_item_id(1),
			),
		),
	];
	// PostgreSQL identity sequences are not transactional. Keep DEFAULT last because its
	// rejected UPDATE still advances outbox_id_seq; no subsequent assertion depends on IDs.
	cases.push((
		"id_identity_default",
		format!("UPDATE decodex.outbox SET id=DEFAULT WHERE id={authority_id}"),
	));
	cases
}

async fn assert_work_item_outbox_authority_cases(
	runtime: &tokio_postgres::Config,
	cases: Vec<(&'static str, String)>,
) -> Result<(), Box<dyn std::error::Error>> {
	let mut diagnostics = Vec::with_capacity(cases.len());
	let mut mismatch_count = 0usize;
	for (label, sql) in cases {
		let (matched, diagnostic) = exercise_outbox_authority_case(runtime, label, &sql).await;
		if !matched {
			mismatch_count += 1;
		}
		diagnostics.push(diagnostic);
	}
	if mismatch_count != 0 {
		return Err(std::io::Error::other(format!(
			"{mismatch_count} WorkItem outbox authority cases mismatched:\n{}",
			serde_json::Value::Array(diagnostics),
		))
		.into());
	}
	Ok(())
}

async fn assert_work_item_outbox_delivery_and_authority(
	store: &PostgresStore,
	schema_owner: &tokio_postgres::Config,
	runtime: &tokio_postgres::Config,
) -> Result<(), Box<dyn std::error::Error>> {
	let (owner, owner_connection) = schema_owner.connect(NoTls).await?;
	let owner_task = tokio::spawn(owner_connection);
	let delivered_id = deliver_work_item_outbox(store, &owner).await?;
	let (authority_id, generic_id) =
		select_work_item_outbox_authority_rows(&owner, delivered_id).await?;
	let cases = work_item_outbox_authority_cases(authority_id, generic_id);
	assert_work_item_outbox_authority_cases(runtime, cases).await?;
	drop(owner);
	owner_task.await??;
	Ok(())
}

async fn assert_create_and_concurrent_replay(
	store: &PostgresStore,
) -> Result<CreateWorkItem, Box<dyn std::error::Error>> {
	let dependency = create(1, WorkItemRelations::default());
	let first = success(store.create_work_item("work-item-create-1", &dependency).await?);
	assert_eq!(first.work_item.work_item.revision(), 1);
	assert_eq!(
		store.create_work_item("work-item-create-1", &dependency).await?,
		WorkItemCommandOutcome::Success(first),
		"same exact request must replay immutable stored bytes",
	);
	let contested = create(7, WorkItemRelations::default());
	let mut concurrent_replays = JoinSet::new();
	for _ in 0..8 {
		let store = store.clone();
		let contested = contested.clone();
		concurrent_replays.spawn(async move {
			store.create_work_item("work-item-concurrent-create-7", &contested).await
		});
	}
	let mut converged = None;
	while let Some(result) = concurrent_replays.join_next().await {
		let outcome = result??;
		assert!(matches!(outcome, WorkItemCommandOutcome::Success(_)));
		if let Some(expected) = &converged {
			assert_eq!(&outcome, expected, "concurrent exact retries must converge");
		} else {
			converged = Some(outcome);
		}
	}
	Ok(dependency)
}

async fn assert_dependency_readiness_blocked(
	store: &PostgresStore,
	project_id: &ProjectId,
	dependency: &CreateWorkItem,
) -> Result<(), Box<dyn std::error::Error>> {
	let dependent_relations = WorkItemRelations {
		depends_on_ids: vec![dependency.work_item_id.clone()],
		..WorkItemRelations::default()
	};
	let dependent = create(2, dependent_relations.clone());
	success(store.create_work_item("work-item-create-2", &dependent).await?);
	success(
		store
			.update_work_item(
				"work-item-plan-2",
				&update(2, 1, WorkItemState::Planned, dependent_relations.clone()),
			)
			.await?,
	);
	let blocked = success(
		store
			.assess_work_item_readiness(
				"work-item-ready-2",
				&dependent.work_item_id,
				project_id,
				2,
				&provenance(42),
			)
			.await?,
	);
	assert_eq!(blocked.work_item.work_item.state(), WorkItemState::Blocked);
	assert_eq!(blocked.work_item.work_item.revision(), 3);
	assert_eq!(blocked.readiness_blockers.len(), 1);
	assert_eq!(
		blocked.readiness_blockers[0].kind,
		WorkItemReadinessBlockerKind::DependencyIncomplete,
	);
	Ok(())
}

async fn assert_ready_and_running_guard(
	store: &PostgresStore,
	project_id: &ProjectId,
) -> Result<(), Box<dyn std::error::Error>> {
	let ready_item = create(3, WorkItemRelations::default());
	success(store.create_work_item("work-item-create-3", &ready_item).await?);
	success(
		store
			.update_work_item(
				"work-item-plan-3",
				&update(3, 1, WorkItemState::Planned, WorkItemRelations::default()),
			)
			.await?,
	);
	let ready = success(
		store
			.assess_work_item_readiness(
				"work-item-ready-3",
				&ready_item.work_item_id,
				project_id,
				2,
				&provenance(43),
			)
			.await?,
	);
	assert_eq!(ready.work_item.work_item.state(), WorkItemState::Ready);
	assert!(ready.readiness_blockers.is_empty());
	store.guard_work_item_running_resume(&ready_item.work_item_id, project_id, 3).await?;
	Ok(())
}

async fn assert_cycle_rejected(store: &PostgresStore) -> Result<(), Box<dyn std::error::Error>> {
	let cycle_a = create(4, WorkItemRelations::default());
	let cycle_b = create(5, WorkItemRelations::default());
	success(store.create_work_item("work-item-create-4", &cycle_a).await?);
	success(store.create_work_item("work-item-create-5", &cycle_b).await?);
	success(
		store
			.update_work_item(
				"work-item-edge-4",
				&update(
					4,
					1,
					WorkItemState::Planned,
					WorkItemRelations {
						depends_on_ids: vec![cycle_b.work_item_id.clone()],
						..WorkItemRelations::default()
					},
				),
			)
			.await?,
	);
	let cycle_request = update(
		5,
		1,
		WorkItemState::Planned,
		WorkItemRelations {
			depends_on_ids: vec![cycle_a.work_item_id.clone()],
			..WorkItemRelations::default()
		},
	);
	assert_eq!(
		store.update_work_item("work-item-cycle-5", &cycle_request).await?,
		WorkItemCommandOutcome::Rejected(WorkItemRejection::DependencyCycle),
	);
	let cycle_b_stored = store
		.read_work_item(&cycle_b.work_item_id)
		.await?
		.expect("cycle fixture WorkItem must remain stored");
	assert_eq!(cycle_b_stored.work_item.revision(), 1);
	assert!(cycle_b_stored.edges.is_empty(), "rejected candidate graph must roll back");
	Ok(())
}

async fn assert_lead_acceptance(
	store: &PostgresStore,
	project_id: &ProjectId,
) -> Result<(), Box<dyn std::error::Error>> {
	let accepted_item = create(6, WorkItemRelations::default());
	success(store.create_work_item("work-item-create-6", &accepted_item).await?);
	success(
		store
			.update_work_item(
				"work-item-plan-6",
				&update(6, 1, WorkItemState::Planned, WorkItemRelations::default()),
			)
			.await?,
	);
	success(
		store
			.assess_work_item_readiness(
				"work-item-ready-6",
				&accepted_item.work_item_id,
				project_id,
				2,
				&provenance(46),
			)
			.await?,
	);
	let mut fixture_config = tokio_postgres::Config::new();
	fixture_config
		.host(std::env::var("PGHOST")?)
		.port(std::env::var("PGPORT")?.parse()?)
		.user(std::env::var("PGUSER")?)
		.dbname("decodex_xy1343_work_items");
	let (fixture_client, fixture_connection) = fixture_config.connect(NoTls).await?;
	let fixture_task = tokio::spawn(fixture_connection);
	fixture_client
		.batch_execute(
			"BEGIN; SET LOCAL session_replication_role='replica'; \
			 WITH operation AS (SELECT clock_timestamp() AS operation_time) \
			 UPDATE decodex.work_items SET state='review', revision=revision+1, \
			 last_changed_by='31000000-0000-4000-8000-000000001343', \
			 last_correlation_id='61000000-0000-4000-8000-000000000046', \
			 last_provenance='XY-1343 external review-state fixture', \
			 updated_at=operation.operation_time FROM operation \
			 WHERE work_item_id='51000000-0000-4000-8000-000000000006'; COMMIT;",
		)
		.await?;
	drop(fixture_client);
	fixture_task.await??;
	let acceptance = AcceptWorkItem {
		acceptance_id: "71000000-0000-4000-8000-000000001343".into(),
		work_item_id: accepted_item.work_item_id.clone(),
		project_id: project_id.clone(),
		expected_revision: 4,
		provenance: provenance(47),
		criteria_provenance: "Exact revision-four criteria snapshot.".into(),
		evidence_summary: "Focused PostgreSQL evidence passed.".into(),
		evidence_provenance: "XY-1343 focused harness.".into(),
	};
	let accepted = success(store.accept_work_item("work-item-accept-6", &acceptance).await?);
	assert_eq!(accepted.work_item.work_item.state(), WorkItemState::Review);
	assert_eq!(accepted.work_item.work_item.revision(), 4);
	assert_eq!(accepted.work_item.accepted_revision, Some(4));
	assert_eq!(
		store
			.accept_work_item(
				"work-item-duplicate-accept-6",
				&AcceptWorkItem {
					acceptance_id: "71000000-0000-4000-8000-000000001344".into(),
					..acceptance
				},
			)
			.await?,
		WorkItemCommandOutcome::Rejected(WorkItemRejection::DuplicateAcceptance),
	);
	Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the focused isolated PostgreSQL 18 V11 harness"]
async fn postgres_exact_work_item_commands() -> Result<(), Box<dyn std::error::Error>> {
	let (schema_owner, runtime) = owner_runtime_configs("DECODEX_TEST")?;
	create_project(&schema_owner).await?;
	let store =
		PostgresStore::connect_runtime_fixture(runtime.clone(), expected_peer_uid()).await?;
	let project_id = ProjectId::new(PROJECT_ID)?;
	let dependency = assert_create_and_concurrent_replay(&store).await?;
	assert_dependency_readiness_blocked(&store, &project_id, &dependency).await?;
	assert_ready_and_running_guard(&store, &project_id).await?;
	assert_cycle_rejected(&store).await?;
	assert_lead_acceptance(&store, &project_id).await?;

	let page = store.query_work_items(&project_id, None, None, 16).await?;
	assert_eq!(page.len(), 7);
	assert!(page.windows(2).all(|pair| pair[0].work_item.id() < pair[1].work_item.id()));
	assert!(page.iter().all(|item| item.work_item.state() != WorkItemState::Done));
	assert_work_item_outbox_delivery_and_authority(&store, &schema_owner, &runtime).await?;
	store.close();
	Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the focused isolated PostgreSQL 18 V11 restore harness"]
async fn postgres_exact_work_item_restore() -> Result<(), Box<dyn std::error::Error>> {
	let (schema_owner, runtime) = owner_runtime_configs("DECODEX_TEST")?;
	let store =
		PostgresStore::connect_runtime_fixture(runtime.clone(), expected_peer_uid()).await?;
	let project_id = ProjectId::new(PROJECT_ID)?;
	let page = store.query_work_items(&project_id, None, None, 16).await?;
	assert_eq!(page.len(), 7);
	let accepted = store
		.read_work_item(&work_item_id(6))
		.await?
		.expect("accepted fixture WorkItem must survive restore");
	assert_eq!(accepted.work_item.state(), WorkItemState::Review);
	assert_eq!(accepted.work_item.revision(), 4);
	assert_eq!(accepted.accepted_revision, Some(4));
	let (client, connection) = schema_owner.connect(NoTls).await?;
	let task = tokio::spawn(connection);
	let counts = client
		.query_one(
			"SELECT (SELECT count(*) FROM decodex.exact_command_receipts \
			 WHERE request_envelope->>'operation' LIKE '%work_item%' \
			 AND receipt_state='completed_success'), \
			 (SELECT count(*) FROM decodex.activity WHERE aggregate_kind='work_item'), \
			 (SELECT count(*) FROM decodex.outbox WHERE aggregate_kind='work_item'), \
			 (SELECT count(*) FROM decodex.work_item_acceptances)",
			&[],
		)
		.await?;
	assert_eq!(counts.get::<_, i64>(0), counts.get::<_, i64>(1));
	assert_eq!(counts.get::<_, i64>(1), counts.get::<_, i64>(2));
	assert_eq!(counts.get::<_, i64>(3), 1);
	let mutation = client
		.execute("UPDATE decodex.work_item_acceptances SET evidence_summary='mutated'", &[])
		.await
		.expect_err("immutable acceptance UPDATE must fail");
	assert_eq!(
		mutation.as_db_error().and_then(|error| error.constraint()),
		Some("work_item_acceptances_immutable"),
	);
	drop(client);
	task.await??;
	store.close();
	Ok(())
}
