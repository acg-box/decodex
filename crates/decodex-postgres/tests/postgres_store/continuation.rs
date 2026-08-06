use serde_json::Value;
use tokio_postgres::{Client, Config};

use super::{
	expected_peer_uid, isolated_blob_store,
	routing_decision::{RoutingFixture, set_account_registry_quota_usage},
};
use decodex_core::{
	BlobStore, ContextPack, ContextPackInput, ContextPackPolicy, ContinuationCommandOutcome,
	ContinuationPlanKind, ContinuationRejection, PinnedContextSource, PossibleSideEffects,
};
use decodex_postgres::{ContinuationPlanEffect, PlanContinuation, PostgresStore};

pub(super) async fn assert_continuation_contract(
	store: &PostgresStore,
	owner: &Client,
	runtime: &Config,
	routing: &RoutingFixture,
) -> Result<(), Box<dyn std::error::Error>> {
	let blob_store = isolated_blob_store()?;
	let fallback_pack = fallback_pack(owner, routing).await?;
	prepare_continuation_fixture(owner, routing).await?;
	set_account_registry_quota_usage(owner, routing.selected_account_id.as_str(), 100).await?;
	assert_stale_revision_contract(store, owner, routing, &blob_store, &fallback_pack).await?;
	let (fallback, fallback_request) =
		assert_missing_fallback_contract(store, owner, routing, &blob_store, &fallback_pack)
			.await?;
	assert_lineage_and_restart_contract(
		owner,
		runtime,
		&blob_store,
		&fallback_pack,
		&fallback_request,
		fallback,
	)
	.await?;
	set_account_registry_quota_usage(owner, routing.selected_account_id.as_str(), 25).await?;
	Ok(())
}

async fn prepare_continuation_fixture(
	owner: &Client,
	routing: &RoutingFixture,
) -> Result<(), Box<dyn std::error::Error>> {
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
				&routing.continuation.decision_id,
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
) -> Result<(ContinuationPlanEffect, PlanContinuation), Box<dyn std::error::Error>> {
	let missing_request = plan_request(1, &routing.continuation);
	let missing = continuation_success(
		store
			.plan_continuation(blob_store, "v17-missing-fallback", &missing_request, fallback_pack)
			.await?,
	)?;
	assert_eq!(missing.plan.kind, ContinuationPlanKind::ContextPackFallback);
	assert!(missing.fallback_context_pack.is_some());
	assert_inert_plan(&missing.plan);
	let replay_inventory = continuation_effect_inventory(owner, &missing_request).await?;
	assert_eq!(replay_inventory["activity"].as_array().map(|rows| rows.len()), Some(3),);
	assert_eq!(replay_inventory["outbox"].as_array().map(|rows| rows.len()), Some(3),);
	let missing_bytes = receipt_bytes(owner, "v17-missing-fallback").await?;
	assert_eq!(
		store
			.plan_continuation(blob_store, "v17-missing-fallback", &missing_request, fallback_pack,)
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
			concat!(
				"SELECT session.revision=1 AND session.state='starting',",
				"snapshot.source_account_id=$4::text::uuid AND snapshot.source_revision=1,",
				"pack.context_pack_id=$3::text::uuid AND pack.conversation_id=plan.conversation_id,",
				"plan.routing_evidence_id IS NULL,plan.routing_evidence_revision IS NULL,",
				"plan.schema_fingerprint IS NULL,plan.codex_experiment_id IS NULL,",
				"plan.codex_experiment_revision IS NULL,plan.codex_observation_id IS NULL,",
				"(SELECT count(*) FROM decodex.activity WHERE correlation_key=$5)=3,",
				"(SELECT count(*) FROM decodex.outbox AS work JOIN decodex.activity AS event ",
				"ON work.effect_key='activity/'||event.sequence::text WHERE event.correlation_key=$5)=3 ",
				"FROM decodex.continuation_plans AS plan ",
				"JOIN decodex.runtime_sessions AS session ",
				"ON session.runtime_session_id=plan.fallback_runtime_session_id ",
				"JOIN decodex.account_snapshots AS snapshot ",
				"ON snapshot.account_snapshot_id=session.account_snapshot_id ",
				"JOIN decodex.context_packs AS pack ",
				"ON pack.context_pack_id=plan.fallback_context_pack_id ",
				"WHERE plan.plan_id=$1::text::uuid AND session.runtime_session_id=$2::text::uuid",
			),
			&[
				&missing.plan.plan_id,
				&missing_request.fallback_runtime_session_id,
				&missing_request.fallback_context_pack_id,
				&routing.selected_account_id.as_str(),
				&"v17-missing-fallback",
			],
		)
		.await?;
	for index in 0..11 {
		assert!(fallback_lineage.get::<_, bool>(index), "V17 fallback lineage {index}");
	}
	let conflicting = PlanContinuation { plan_id: uuid(0xf4, 99), ..missing_request.clone() };
	let conflicting_inventory = continuation_effect_inventory(owner, &conflicting).await?;
	assert_eq!(conflicting_inventory["activity"].as_array().map(|rows| rows.len()), Some(2),);
	assert_eq!(conflicting_inventory["outbox"].as_array().map(|rows| rows.len()), Some(2),);
	assert!(matches!(
		store
			.plan_continuation(blob_store, "v17-duplicate-consumption", &conflicting, fallback_pack,)
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
	Ok((missing, missing_request))
}

async fn assert_stale_revision_contract(
	store: &PostgresStore,
	owner: &Client,
	routing: &RoutingFixture,
	blob_store: &BlobStore,
	fallback_pack: &ContextPack,
) -> Result<(), Box<dyn std::error::Error>> {
	let stale_request = PlanContinuation {
		expected_consumer_revision: 2,
		..plan_request(5, &routing.continuation)
	};
	let stale_inventory = continuation_effect_inventory(owner, &stale_request).await?;
	assert_eq!(stale_inventory["activity"].as_array().map(|rows| rows.len()), Some(0),);
	assert_eq!(stale_inventory["outbox"].as_array().map(|rows| rows.len()), Some(0),);
	assert!(matches!(
		store
			.plan_continuation(blob_store, "v17-stale-revision", &stale_request, fallback_pack,)
			.await?,
		ContinuationCommandOutcome::Rejected(ContinuationRejection::StaleConsumerRevision)
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
	runtime: &Config,
	blob_store: &BlobStore,
	fallback_pack: &ContextPack,
	fallback_request: &PlanContinuation,
	fallback: ContinuationPlanEffect,
) -> Result<(), Box<dyn std::error::Error>> {
	let lineage = owner
		.query_one(
			concat!(
				"SELECT NOT plan.replay_permitted,NOT plan.dispatch_enabled,",
				"plan.consumer_kind='conversation_turn',",
				"plan.consumer_conversation_id IS NOT NULL ",
				"AND plan.conversation_revision=1 AND plan.turn_id IS NOT NULL,",
				"plan.managed_run_id IS NULL AND plan.managed_run_revision IS NULL ",
				"AND plan.managed_execution_id IS NULL,",
				"(SELECT count(*) FROM decodex.activity WHERE aggregate_kind='continuation_plan'",
				"AND aggregate_id=plan.plan_id::text AND event_kind='continuation_plan_created')=1,",
				"(SELECT count(*) FROM decodex.outbox WHERE aggregate_kind='continuation_plan'",
				"AND aggregate_id=plan.plan_id::text)=1 ",
				"FROM decodex.continuation_plans AS plan WHERE plan.plan_id=$1::text::uuid",
			),
			&[&fallback.plan.plan_id],
		)
		.await?;
	for index in 0..6 {
		assert!(lineage.get::<_, bool>(index), "V17 lineage assertion {index}");
	}
	let restarted =
		PostgresStore::connect_runtime_fixture(runtime.clone(), expected_peer_uid()).await?;
	assert_eq!(
		restarted
			.plan_continuation(blob_store, "v17-missing-fallback", fallback_request, fallback_pack,)
			.await?,
		ContinuationCommandOutcome::Success(fallback),
	);
	Ok(())
}

async fn fallback_pack(
	owner: &Client,
	routing: &RoutingFixture,
) -> Result<ContextPack, Box<dyn std::error::Error>> {
	let conversation_id: String = owner
		.query_one(
			concat!(
				"SELECT conversation_id::text FROM decodex.runtime_sessions ",
				"WHERE runtime_session_id=$1::text::uuid",
			),
			&[&routing.continuation_runtime_session_id.as_str()],
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
			"PostgreSQL retains the exact inert continuation plan.",
		)?,
		optional_sources: vec![],
	})?)
}

fn plan_request(
	marker: u8,
	binding: &decodex_postgres::QuickTaskContinuationBinding,
) -> PlanContinuation {
	PlanContinuation {
		operation_id: uuid(0xf4, marker),
		routing_decision_id: binding.decision_id.clone(),
		expected_consumer_revision: 1,
		plan_id: uuid(0xf5, marker),
		fallback_runtime_session_id: uuid(0xf6, marker),
		fallback_account_snapshot_id: binding.account_snapshot_id.clone(),
		fallback_context_pack_id: uuid(0xf8, marker),
	}
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

fn assert_inert_plan(plan: &decodex_core::ContinuationPlan) {
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
				"SELECT pg_catalog.jsonb_build_object('activity',COALESCE((",
				"SELECT pg_catalog.jsonb_agg(pg_catalog.jsonb_build_object(",
				"'sequence',sequence,'aggregate_kind',aggregate_kind,'aggregate_id',aggregate_id,",
				"'revision',revision,'event_kind',event_kind,'correlation_key',correlation_key,",
				"'payload',payload) ORDER BY sequence) FROM decodex.activity WHERE ",
				"(aggregate_kind='continuation_plan' AND aggregate_id=$1) OR ",
				"(aggregate_kind='runtime_session' AND aggregate_id=$2) OR ",
				"(aggregate_kind='context_pack' AND aggregate_id=$3)), '[]'::jsonb),",
				"'outbox',COALESCE((SELECT pg_catalog.jsonb_agg(",
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
			concat!(
				"SELECT (SELECT count(*) FROM decodex.continuation_plans ",
				"WHERE kind='same_thread')=0,",
				"(SELECT count(*) FROM decodex.continuation_plans ",
				"WHERE kind='context_pack_fallback' AND fallback_context_pack_id IS NOT NULL ",
				"AND fallback_runtime_session_id IS NOT NULL)=1,",
				"(SELECT bool_and(consumer_kind='conversation_turn' ",
				"AND consumer_conversation_id IS NOT NULL AND conversation_revision=1 ",
				"AND turn_id IS NOT NULL AND managed_run_id IS NULL ",
				"AND managed_run_revision IS NULL AND managed_execution_id IS NULL ",
				"AND NOT replay_permitted ",
				"AND NOT dispatch_enabled)",
				"FROM decodex.continuation_plans)",
			),
			&[],
		)
		.await?;
	for index in 0..3 {
		assert!(row.get::<_, bool>(index), "restored V17 assertion {index}");
	}
	Ok(())
}
