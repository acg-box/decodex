use serde_json::Value;
use tokio_postgres::{Client, Config};

use super::{
	expected_peer_uid, isolated_blob_store,
	routing_decision::{RoutingFixture, create_stale_evidence_snapshot},
};
use decodex_core::{
	BlobStore, CodexExperimentCommandOutcome, CodexExperimentIdentity,
	CodexExperimentObservationKind,
	ContextPack, ContextPackInput, ContextPackPolicy, ContinuationCommandOutcome,
	ContinuationEffectBarrierState, ContinuationPlanKind, ContinuationRejection,
	PinnedContextSource, PossibleSideEffects,
};
use decodex_postgres::{
	BindCodexExperimentThread, CodexExperimentCreationFenceOutcome, PlanContinuation,
	ContinuationPlanEffect, PostgresStore, PrepareCodexExperiment,
	RecordCodexExperimentObservation, RouteAccount,
};

const SUBMITTED_RECEIPT_ID: &str = "f1000000-0000-4000-8000-000000000017";

pub(super) async fn assert_continuation_contract(
	store: &PostgresStore,
	owner: &Client,
	migration: &Config,
	runtime: &Config,
	routing: &RoutingFixture,
) -> Result<(), Box<dyn std::error::Error>> {
	let blob_store = isolated_blob_store()?;
	let fallback_pack = fallback_pack(owner, routing).await?;
	prepare_continuation_fixture(owner, routing).await?;
	assert_missing_fallback_contract(store, owner, routing, &blob_store, &fallback_pack).await?;
	assert_alternate_fallback_contracts(store, owner, routing, &blob_store, &fallback_pack).await?;
	let (same_thread, same_thread_request) =
		assert_same_thread_contract(store, routing, &blob_store, &fallback_pack).await?;
	assert_stale_revision_contract(store, owner, routing, &blob_store, &fallback_pack).await?;
	assert_lineage_and_restart_contract(
		owner,
		(migration, runtime),
		&blob_store,
		&fallback_pack,
		&same_thread_request,
		same_thread,
	)
	.await?;
	Ok(())
}

async fn prepare_continuation_fixture(
	owner: &Client,
	routing: &RoutingFixture,
) -> Result<(), Box<dyn std::error::Error>> {
	owner
		.execute(
			"INSERT INTO decodex.managed_run_submitted_turn_receipts(\
			 receipt_id,managed_run_id,project_id,runtime_session_id,runtime_session_revision,\
			 turn_id,submitted_at) SELECT $1::text::uuid,run.managed_run_id,run.project_id,\
			 run.runtime_session_id,run.runtime_session_revision,$2::text::uuid,\
			 pg_catalog.clock_timestamp() FROM decodex.managed_runs AS run\
			 WHERE run.managed_run_id=$3::text::uuid",
			&[
				&SUBMITTED_RECEIPT_ID,
				&"f2000000-0000-1000-8000-000000000017",
				&routing.selected_request.managed_run_id.as_str(),
			],
		)
		.await?;

	owner.batch_execute("BEGIN").await?;
	let rollback_response: Vec<u8> = owner
		.query_one(
			"SELECT decodex.plan_continuation_exact(\
			 'decodex/exact-command/1','v17-rollback',$1::text::uuid,$2::text::uuid,1,\
			 $3::text::uuid,$4::text::uuid,$5::text::uuid,$6::text::uuid,\
			 ''::bytea,repeat('0',64),repeat('0',64),1024,1,'unknown',false,0,\
			 ARRAY[]::text[],ARRAY[]::text[],ARRAY[]::bigint[],ARRAY[]::text[],\
			 ARRAY[]::bigint[],ARRAY[]::bigint[],ARRAY[]::text[],ARRAY[]::text[],\
			 ARRAY[]::text[],ARRAY[]::bigint[])",
			&[
				&uuid(0xf3, 1),
				&routing.selected.decision_id,
				&uuid(0xf3, 2),
				&uuid(0xf3, 3),
				&uuid(0xf3, 4),
				&uuid(0xf3, 5),
			],
		)
		.await?
		.get(0);
	owner.batch_execute("ROLLBACK").await?;
	assert!(!rollback_response.is_empty());
	assert_eq!(receipt_count(owner, "v17-rollback").await?, 0);
	Ok(())
}

async fn assert_missing_fallback_contract(
	store: &PostgresStore,
	owner: &Client,
	routing: &RoutingFixture,
	blob_store: &BlobStore,
	fallback_pack: &ContextPack,
) -> Result<(), Box<dyn std::error::Error>> {
	let missing_request = plan_request(1, &routing.selected.decision_id);
	let missing = continuation_success(
		store
			.plan_continuation(
				blob_store,
				"v17-missing-fallback",
				&missing_request,
				fallback_pack,
			)
			.await?,
	)?;
	assert_eq!(missing.plan.kind, ContinuationPlanKind::ContextPackFallback);
	assert!(missing.fallback_context_pack.is_some());
	assert_barrier(&missing.plan);
	let replay_inventory = continuation_effect_inventory(owner, &missing_request).await?;
	assert_eq!(replay_inventory["activity"].as_array().map(|rows| rows.len()), Some(3),);
	assert_eq!(replay_inventory["outbox"].as_array().map(|rows| rows.len()), Some(3),);
	let missing_bytes = receipt_bytes(owner, "v17-missing-fallback").await?;
	assert_eq!(
		store
			.plan_continuation(
				blob_store,
				"v17-missing-fallback",
				&missing_request,
				fallback_pack,
			)
			.await?,
		ContinuationCommandOutcome::Success(missing.clone()),
	);
	assert_eq!(receipt_bytes(owner, "v17-missing-fallback").await?, missing_bytes);
	assert_eq!(continuation_effect_inventory(owner, &missing_request).await?, replay_inventory,);
	assert_eq!(
		store
			.plan_continuation(
				blob_store,
				"v17-missing-fallback-replay",
				&missing_request,
				fallback_pack,
			)
			.await?,
		ContinuationCommandOutcome::Success(missing.clone()),
	);
	assert_eq!(receipt_bytes(owner, "v17-missing-fallback-replay").await?, missing_bytes);
	assert_eq!(continuation_effect_inventory(owner, &missing_request).await?, replay_inventory,);
	let fallback_lineage = owner
		.query_one(
			"SELECT session.revision=1 AND session.state='starting',\
			 snapshot.source_account_id=$4::text::uuid AND snapshot.source_revision=1,\
			 pack.context_pack_id=$3::text::uuid AND pack.conversation_id=plan.conversation_id,\
			 (SELECT count(*) FROM decodex.activity WHERE correlation_key=$5)=3,\
			 (SELECT count(*) FROM decodex.outbox AS work JOIN decodex.activity AS event\
			  ON work.effect_key='activity/'||event.sequence::text WHERE event.correlation_key=$5)=3\
			 FROM decodex.continuation_plans AS plan\
			 JOIN decodex.runtime_sessions AS session\
			  ON session.runtime_session_id=plan.fallback_runtime_session_id\
			 JOIN decodex.account_snapshots AS snapshot\
			  ON snapshot.account_snapshot_id=session.account_snapshot_id\
			 JOIN decodex.context_packs AS pack\
			  ON pack.context_pack_id=plan.fallback_context_pack_id\
			 WHERE plan.plan_id=$1::text::uuid AND session.runtime_session_id=$2::text::uuid",
			&[
				&missing.plan.plan_id,
				&missing_request.fallback_runtime_session_id,
				&missing_request.fallback_context_pack_id,
				&routing.selected_account_id.as_str(),
				&"v17-missing-fallback",
			],
		)
		.await?;
	for index in 0..5 {
		assert!(fallback_lineage.get::<_, bool>(index), "V17 fallback lineage {index}");
	}
	let conflicting = PlanContinuation { plan_id: uuid(0xf4, 99), ..missing_request.clone() };
	let conflicting_inventory = continuation_effect_inventory(owner, &conflicting).await?;
	assert_eq!(conflicting_inventory["activity"].as_array().map(|rows| rows.len()), Some(2),);
	assert_eq!(conflicting_inventory["outbox"].as_array().map(|rows| rows.len()), Some(2),);
	assert!(matches!(
		store
			.plan_continuation(
				blob_store,
				"v17-duplicate-consumption",
				&conflicting,
				fallback_pack,
			)
			.await?,
		ContinuationCommandOutcome::Rejected(ContinuationRejection::DecisionAlreadyConsumed)
	));
	assert_eq!(receipt_count(owner, "v17-duplicate-consumption").await?, 1);
	let conflicting_plan_rows: i64 = owner
		.query_one(
			"SELECT count(*) FROM decodex.continuation_plans WHERE plan_id=$1::text::uuid",
			&[&conflicting.plan_id],
		)
		.await?
		.get(0);
	assert_eq!(conflicting_plan_rows, 0);
	assert_no_continuation_effects(owner, "v17-duplicate-consumption", &conflicting).await?;
	assert_eq!(continuation_effect_inventory(owner, &conflicting).await?, conflicting_inventory,);
	Ok(())
}

async fn assert_alternate_fallback_contracts(
	store: &PostgresStore,
	owner: &Client,
	routing: &RoutingFixture,
	blob_store: &BlobStore,
	fallback_pack: &ContextPack,
) -> Result<(), Box<dyn std::error::Error>> {
	let mismatch_decision = route_selected(store, routing, 2).await?;
	create_positive_experiment(store, routing, 2, &mismatch_decision.decision.snapshot_id, true)
		.await?;
	let mismatch = continuation_success(
		store
			.plan_continuation(
				blob_store,
				"v17-mismatch-fallback",
				&plan_request(2, &mismatch_decision.decision_id),
				fallback_pack,
			)
			.await?,
	)?;
	assert_eq!(mismatch.plan.kind, ContinuationPlanKind::ContextPackFallback);
	assert_barrier(&mismatch.plan);

	let stale_evidence_decision = route_selected(store, routing, 6).await?;
	let stale_snapshot_id = create_stale_evidence_snapshot(store, owner, routing).await?;
	create_positive_experiment(store, routing, 6, &stale_snapshot_id, true).await?;
	let stale_evidence = continuation_success(
		store
			.plan_continuation(
				blob_store,
				"v17-stale-evidence-fallback",
				&plan_request(6, &stale_evidence_decision.decision_id),
				fallback_pack,
			)
			.await?,
	)?;
	assert_ne!(stale_snapshot_id, stale_evidence_decision.decision.snapshot_id);
	assert_eq!(stale_evidence.plan.kind, ContinuationPlanKind::ContextPackFallback);
	assert_barrier(&stale_evidence.plan);

	let ambiguous_decision = route_selected(store, routing, 4).await?;
	create_ambiguous_creation_fence(store, routing, 4, &ambiguous_decision.decision.snapshot_id)
		.await?;
	let ambiguous = continuation_success(
		store
			.plan_continuation(
				blob_store,
				"v17-ambiguous-fallback",
				&plan_request(4, &ambiguous_decision.decision_id),
				fallback_pack,
			)
			.await?,
	)?;
	assert_eq!(ambiguous.plan.kind, ContinuationPlanKind::ContextPackFallback);
	assert_barrier(&ambiguous.plan);
	Ok(())
}

async fn assert_same_thread_contract(
	store: &PostgresStore,
	routing: &RoutingFixture,
	blob_store: &BlobStore,
	fallback_pack: &ContextPack,
) -> Result<(ContinuationPlanEffect, PlanContinuation), Box<dyn std::error::Error>> {
	let same_thread_decision = route_selected(store, routing, 3).await?;
	create_positive_experiment(
		store,
		routing,
		3,
		&same_thread_decision.decision.snapshot_id,
		false,
	)
	.await?;
	let same_thread_request = plan_request(3, &same_thread_decision.decision_id);
	let same_thread = continuation_success(
		store
			.plan_continuation(blob_store, "v17-same-thread", &same_thread_request, fallback_pack)
			.await?,
	)?;
	assert_eq!(same_thread.plan.kind, ContinuationPlanKind::SameThread);
	assert_eq!(
		same_thread.plan.codex_thread_id.as_deref(),
		Some(routing.selected_thread_id.as_str())
	);
	assert!(same_thread.plan.same_thread_evidence.is_some());
	assert!(same_thread.fallback_context_pack.is_none());
	assert_barrier(&same_thread.plan);
	Ok((same_thread, same_thread_request))
}

async fn assert_stale_revision_contract(
	store: &PostgresStore,
	owner: &Client,
	routing: &RoutingFixture,
	blob_store: &BlobStore,
	fallback_pack: &ContextPack,
) -> Result<(), Box<dyn std::error::Error>> {
	let stale_decision = route_selected(store, routing, 5).await?;
	let stale_request = PlanContinuation {
		expected_managed_run_revision: 2,
		..plan_request(5, &stale_decision.decision_id)
	};
	let stale_inventory = continuation_effect_inventory(owner, &stale_request).await?;
	assert_eq!(stale_inventory["activity"].as_array().map(|rows| rows.len()), Some(0),);
	assert_eq!(stale_inventory["outbox"].as_array().map(|rows| rows.len()), Some(0),);
	assert!(matches!(
		store
			.plan_continuation(blob_store, "v17-stale-revision", &stale_request, fallback_pack,)
			.await?,
		ContinuationCommandOutcome::Rejected(ContinuationRejection::StaleManagedRunRevision)
	));
	assert_eq!(receipt_count(owner, "v17-stale-revision").await?, 1);
	let stale_plan_rows: i64 = owner
		.query_one(
			"SELECT count(*) FROM decodex.continuation_plans WHERE plan_id=$1::text::uuid",
			&[&uuid(0xf5, 5)],
		)
		.await?
		.get(0);
	assert_eq!(stale_plan_rows, 0);
	assert_no_continuation_effects(owner, "v17-stale-revision", &stale_request).await?;
	assert_eq!(continuation_effect_inventory(owner, &stale_request).await?, stale_inventory,);
	Ok(())
}

async fn assert_lineage_and_restart_contract(
	owner: &Client,
	configs: (&Config, &Config),
	blob_store: &BlobStore,
	fallback_pack: &ContextPack,
	same_thread_request: &PlanContinuation,
	same_thread: ContinuationPlanEffect,
) -> Result<(), Box<dyn std::error::Error>> {
	let lineage = owner
		.query_one(
			"SELECT plan.effect_barrier_state::text,plan.effect_barrier_revision,\
			 plan.submitted_turn_receipt_count,NOT plan.replay_permitted,NOT plan.dispatch_enabled,\
			 (SELECT count(*) FROM decodex.managed_run_submitted_turn_receipts\
			  WHERE managed_run_id=plan.managed_run_id AND receipt_id=$2::text::uuid)=1,\
			 (SELECT count(*) FROM decodex.activity WHERE aggregate_kind='continuation_plan'\
			  AND aggregate_id=plan.plan_id::text AND event_kind='continuation_plan_created')=1,\
			 (SELECT count(*) FROM decodex.outbox WHERE aggregate_kind='continuation_plan'\
			  AND aggregate_id=plan.plan_id::text)=1\
			 FROM decodex.continuation_plans AS plan WHERE plan.plan_id=$1::text::uuid",
			&[&same_thread.plan.plan_id, &SUBMITTED_RECEIPT_ID],
		)
		.await?;
	assert_eq!(lineage.get::<_, String>(0), "guarded");
	assert_eq!(lineage.get::<_, i64>(1), 1);
	assert_eq!(lineage.get::<_, i64>(2), 1);
	for index in 3..8 {
		assert!(lineage.get::<_, bool>(index), "V17 lineage assertion {index}");
	}
	let (migration, runtime) = configs;
	let restarted =
		PostgresStore::connect(migration.clone(), runtime.clone(), expected_peer_uid()).await?;
	assert_eq!(
		restarted
			.plan_continuation(blob_store, "v17-same-thread", same_thread_request, fallback_pack,)
			.await?,
		ContinuationCommandOutcome::Success(same_thread),
	);
	Ok(())
}

async fn create_ambiguous_creation_fence(
	store: &PostgresStore,
	routing: &RoutingFixture,
	marker: u8,
	snapshot_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
	let experiment_id = uuid(0xe8, marker);
	let identity = CodexExperimentIdentity {
		experiment_id: experiment_id.clone(),
		managed_run_id: routing.selected_request.managed_run_id.clone(),
		managed_run_revision: 1,
		routing_snapshot_id: snapshot_id.to_owned(),
		account_id: routing.selected_account_id.clone(),
		account_revision: 1,
		role_profile_revision: 1,
		build_id: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
		repository_cwd: "/srv/vnext-acceptance".into(),
		thread_title: "V17 ambiguous creation fence".into(),
	};
	assert!(matches!(
		store
			.prepare_codex_experiment("v17-ambiguous-prepare", &PrepareCodexExperiment { identity },)
			.await?,
		CodexExperimentCommandOutcome::Applied(_)
	));
	let attempt_id = uuid(0xe9, marker);
	assert!(matches!(
		store
			.mark_codex_experiment_creation_possible(
				"v17-ambiguous-fence",
				&experiment_id,
				1,
				&attempt_id,
			)
			.await?,
		CodexExperimentCreationFenceOutcome::Fresh(_)
	));
	assert!(matches!(
		store
			.mark_codex_experiment_creation_possible(
				"v17-ambiguous-fence",
				&experiment_id,
				1,
				&attempt_id,
			)
			.await?,
		CodexExperimentCreationFenceOutcome::ReplayedAmbiguous { .. }
	));
	Ok(())
}

async fn fallback_pack(
	owner: &Client,
	routing: &RoutingFixture,
) -> Result<ContextPack, Box<dyn std::error::Error>> {
	let conversation_id: String = owner
		.query_one(
			"SELECT conversation_id::text FROM decodex.runtime_sessions\
			 WHERE runtime_session_id=$1::text::uuid",
			&[&routing.selected_runtime_session_id.as_str()],
		)
		.await?
		.get(0);
	Ok(decodex_core::compile_context_pack(ContextPackInput {
		conversation_id: decodex_core::ConversationId::new(conversation_id)?,
		possible_side_effects: PossibleSideEffects::Unknown,
		policy: ContextPackPolicy::new(4096, 4)?,
		pinned: PinnedContextSource::new(
			"vnext-acceptance-source",
			1,
			"PostgreSQL retains the exact continuation barrier.",
		)?,
		optional_sources: vec![],
	})?)
}

fn plan_request(marker: u8, decision_id: &str) -> PlanContinuation {
	PlanContinuation {
		operation_id: uuid(0xf4, marker),
		routing_decision_id: decision_id.to_owned(),
		expected_managed_run_revision: 1,
		plan_id: uuid(0xf5, marker),
		fallback_runtime_session_id: uuid(0xf6, marker),
		fallback_account_snapshot_id: uuid(0xf7, marker),
		fallback_context_pack_id: uuid(0xf8, marker),
	}
}

async fn route_selected(
	store: &PostgresStore,
	routing: &RoutingFixture,
	marker: u8,
) -> Result<decodex_postgres::PersistedRoutingDecision, Box<dyn std::error::Error>> {
	let outcome = store
		.route_account(
			&format!("v17-selected-decision-{marker}"),
			&RouteAccount { operation_id: uuid(0xf9, marker), ..routing.selected_request.clone() },
		)
		.await?;
	match outcome {
		decodex_core::RoutingCommandOutcome::Success(value) => Ok(value),
		decodex_core::RoutingCommandOutcome::Rejected(rejection) =>
			Err(format!("V17 selected fixture rejected: {}", rejection.code).into()),
	}
}

async fn create_positive_experiment(
	store: &PostgresStore,
	routing: &RoutingFixture,
	marker: u8,
	snapshot_id: &str,
	mismatched_thread: bool,
) -> Result<(), Box<dyn std::error::Error>> {
	let experiment_id = uuid(0xea, marker);
	let marker_text = format!("decodex.experiment.v1:{experiment_id}");
	let identity = CodexExperimentIdentity {
		experiment_id: experiment_id.clone(),
		managed_run_id: routing.selected_request.managed_run_id.clone(),
		managed_run_revision: 1,
		routing_snapshot_id: snapshot_id.to_owned(),
		account_id: routing.selected_account_id.clone(),
		account_revision: 1,
		role_profile_revision: 1,
		build_id: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
		repository_cwd: "/srv/vnext-acceptance".into(),
		thread_title: format!("V17 positive evidence {marker_text}"),
	};
	assert!(matches!(
		store
			.prepare_codex_experiment(
				&format!("v17-experiment-prepare-{marker}"),
				&PrepareCodexExperiment { identity: identity.clone() },
			)
			.await?,
		CodexExperimentCommandOutcome::Applied(_)
	));
	let attempt_id = uuid(0xeb, marker);
	assert!(matches!(
		store
			.mark_codex_experiment_creation_possible(
				&format!("v17-experiment-fence-{marker}"),
				&experiment_id,
				1,
				&attempt_id,
			)
			.await?,
		CodexExperimentCreationFenceOutcome::Fresh(_)
	));
	let thread_id =
		if mismatched_thread { uuid(0xec, marker) } else { routing.selected_thread_id.clone() };
	assert!(matches!(
		store
			.bind_codex_experiment_thread(
				&format!("v17-experiment-bind-{marker}"),
				&BindCodexExperimentThread {
					experiment_id: experiment_id.clone(),
					expected_revision: 2,
					attempt_id,
					thread_id: thread_id.clone(),
					response_id: uuid(0xed, marker),
					response_title: identity.thread_title,
					response_cwd: identity.repository_cwd,
					response_marker: marker_text.clone(),
					ephemeral: false,
				},
			)
			.await?,
		CodexExperimentCommandOutcome::Applied(_)
	));
	assert!(matches!(
		store
			.record_codex_experiment_observation(
				&format!("v17-experiment-observe-{marker}"),
				&RecordCodexExperimentObservation {
					experiment_id,
					expected_revision: 3,
					observation_id: uuid(0xee, marker),
					kind: CodexExperimentObservationKind::ThreadReadItem,
					thread_id,
					marker: marker_text,
					source_id: format!("thread-read-{marker}"),
					fact_digest: "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
						.into(),
				},
			)
			.await?,
		CodexExperimentCommandOutcome::Applied(_)
	));
	Ok(())
}

fn continuation_success<T>(
	outcome: ContinuationCommandOutcome<T>,
) -> Result<T, Box<dyn std::error::Error>> {
	match outcome {
		ContinuationCommandOutcome::Success(value) => Ok(value),
		ContinuationCommandOutcome::Rejected(rejection) =>
			Err(format!("continuation rejected: {rejection:?}").into()),
	}
}

fn assert_barrier(plan: &decodex_core::ContinuationPlan) {
	assert_eq!(plan.effect_barrier_state, ContinuationEffectBarrierState::Guarded);
	assert_eq!(plan.effect_barrier_revision, 1);
	assert_eq!(plan.submitted_turn_receipt_count, 1);
	assert!(!plan.replay_permitted);
	assert!(!plan.dispatch_enabled);
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

async fn continuation_effect_inventory(
	client: &Client,
	request: &PlanContinuation,
) -> Result<Value, tokio_postgres::Error> {
	let row = client
		.query_one(
			concat!(
				"SELECT pg_catalog.jsonb_build_object('activity',pg_catalog.coalesce((",
				"SELECT pg_catalog.jsonb_agg(pg_catalog.jsonb_build_object(",
				"'sequence',sequence,'aggregate_kind',aggregate_kind,'aggregate_id',aggregate_id,",
				"'revision',revision,'event_kind',event_kind,'correlation_key',correlation_key,",
				"'payload',payload) ORDER BY sequence) FROM decodex.activity WHERE ",
				"(aggregate_kind='continuation_plan' AND aggregate_id=$1) OR ",
				"(aggregate_kind='runtime_session' AND aggregate_id=$2) OR ",
				"(aggregate_kind='context_pack' AND aggregate_id=$3)), '[]'::jsonb),",
				"'outbox',pg_catalog.coalesce((SELECT pg_catalog.jsonb_agg(",
				"pg_catalog.jsonb_build_object('id',id,'effect_key',effect_key,",
				"'aggregate_kind',aggregate_kind,'aggregate_id',aggregate_id,",
				"'aggregate_revision',aggregate_revision,'payload',payload) ORDER BY id) ",
				"FROM decodex.outbox WHERE ",
				"(aggregate_kind='continuation_plan' AND aggregate_id=$1) OR ",
				"(aggregate_kind='runtime_session' AND aggregate_id=$2) OR ",
				"(aggregate_kind='context_pack' AND aggregate_id=$3)), '[]'::jsonb))",
			),
			&[
				&request.plan_id,
				&request.fallback_runtime_session_id,
				&request.fallback_context_pack_id,
			],
		)
		.await?;
	Ok(row.get(0))
}

async fn assert_no_continuation_effects(
	client: &Client,
	key: &str,
	request: &PlanContinuation,
) -> Result<(), tokio_postgres::Error> {
	let row = client
		.query_one(
			concat!(
				"SELECT (SELECT count(*) FROM decodex.activity WHERE correlation_key=$1 ",
				"OR (aggregate_kind='continuation_plan' AND aggregate_id=$2)),",
				"(SELECT count(*) FROM decodex.outbox WHERE ",
				"(aggregate_kind='continuation_plan' AND aggregate_id=$2) ",
				"OR payload->>'continuation_plan_id'=$2 ",
				"OR payload->'payload'->>'continuation_plan_id'=$2)",
			),
			&[&key, &request.plan_id],
		)
		.await?;
	assert_eq!(row.get::<_, i64>(0), 0);
	assert_eq!(row.get::<_, i64>(1), 0);
	Ok(())
}

fn uuid(prefix: u8, marker: u8) -> String {
	format!("{prefix:02x}000000-0000-4000-8000-{marker:012}")
}

pub(super) async fn assert_restored_continuation_contract(
	client: &Client,
) -> Result<(), Box<dyn std::error::Error>> {
	let row = client
		.query_one(
			"SELECT (SELECT count(*) FROM decodex.continuation_plans\
			  WHERE kind='same_thread' AND NOT replay_permitted AND NOT dispatch_enabled)=1,\
			 (SELECT count(*) FROM decodex.continuation_plans\
			  WHERE kind='context_pack_fallback' AND fallback_context_pack_id IS NOT NULL\
			  AND fallback_runtime_session_id IS NOT NULL)=4,\
			 (SELECT bool_and(submitted_turn_receipt_count=1 AND effect_barrier_revision=1)\
			  FROM decodex.continuation_plans)",
			&[],
		)
		.await?;
	for index in 0..3 {
		assert!(row.get::<_, bool>(index), "restored V17 assertion {index}");
	}
	Ok(())
}
