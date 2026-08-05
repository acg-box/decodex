use serde_json::Value;
use tokio::task::JoinSet;
use tokio_postgres::{Client, Config, NoTls};

use super::routing_decision::{RoutingFixture, advance_stale_policy};
use decodex_core::WaitingUsageWakeCommandOutcome;
use decodex_postgres::{PostgresStore, RegisterWaitingUsageWake};

const PROTOCOL: &str = "decodex/exact-command/1";

pub(super) async fn assert_waiting_wake_contract(
	store: &PostgresStore,
	owner: &Client,
	schema_owner: &Config,
	runtime: &Config,
	routing: &RoutingFixture,
) -> Result<(), Box<dyn std::error::Error>> {
	assert_v19_catalog_authority(owner, runtime).await?;

	let ready_at = routing.waiting.decision.ready_at_micros.expect("waiting decision is timed");
	let registration_at = ready_at - 1;
	let equal_registration_at = ready_at - 2;
	let registration_operation = uuid(0xa8, 1);
	assert_registration_rollback(owner, routing, &registration_operation, registration_at).await?;

	let stale_ready =
		routing.stale_waiting.decision.ready_at_micros.expect("stale waiting decision is timed");
	let cancel_ready =
		routing.cancel_waiting.decision.ready_at_micros.expect("cancellation wait is timed");
	assert_eq!(stale_ready, cancel_ready);
	assert_eq!(stale_ready, ready_at + 1);
	let equal_ready_at = stale_ready;
	assert_registration_monotonicity(owner, routing).await?;
	let registered = register_waiting_wakes(
		owner,
		routing,
		&registration_operation,
		ready_at,
		registration_at,
		equal_registration_at,
		equal_ready_at,
	)
	.await?;
	let claimed =
		assert_initial_claims(owner, &registered, registration_at, ready_at, equal_ready_at)
			.await?;
	assert_supersession_and_cancellation(store, schema_owner, owner, &registered, equal_ready_at)
		.await?;
	let reclaimed = reclaim_waiting_wake(owner, &claimed).await?;
	assert_fire_contract(
		owner,
		routing,
		&registration_operation,
		&registered,
		&claimed,
		&reclaimed,
	)
	.await?;
	assert_restart_replay(runtime, routing, registration_operation, &registered).await?;
	assert_exact_chain(owner).await?;
	Ok(())
}

struct RegisteredWakes {
	registered_bytes: Vec<u8>,
	registered: Value,
	reordered_stale_bytes: Vec<u8>,
	reordered_stale: Value,
	reordered_cancel_bytes: Vec<u8>,
	reordered_cancel: Value,
	expected_wake_order: [(i64, i64, String); 3],
}

struct ClaimedWake {
	claimed_bytes: Vec<u8>,
	claimed: Value,
	claim_operation: String,
	claim_id: String,
	first_holder: String,
	first_expiry: i64,
	first_fence: String,
}

struct ReclaimedWake {
	reclaimed: Value,
	second_holder: String,
	second_fence: String,
}

async fn assert_registration_rollback(
	owner: &Client,
	routing: &RoutingFixture,
	registration_operation: &str,
	registration_at: i64,
) -> Result<(), Box<dyn std::error::Error>> {
	owner.batch_execute("BEGIN").await?;
	let rollback = internal_register(
		owner,
		"v19-register-rollback",
		registration_operation,
		&routing.waiting.decision_id,
		1,
		registration_at,
	)
	.await?;
	let rollback_effect = assert_success(&rollback, "registered");
	let rollback_outbox_id = integer(&rollback_effect["outbox_effects"][0], "id");
	owner.batch_execute("ROLLBACK").await?;
	assert_cluster_absent(
		owner,
		"v19-register-rollback",
		registration_operation,
		rollback_outbox_id,
	)
	.await?;
	Ok(())
}

async fn assert_registration_monotonicity(
	owner: &Client,
	routing: &RoutingFixture,
) -> Result<(), Box<dyn std::error::Error>> {
	owner.batch_execute("BEGIN").await?;
	let monotonic_error = internal_register(
		owner,
		"v19-register-before-authority",
		&uuid(0xa8, 2),
		&routing.stale_waiting.decision_id,
		1,
		0,
	)
	.await
	.expect_err("registration before locked decision/run authority is rejected");
	assert_eq!(
		monotonic_error.as_db_error().and_then(tokio_postgres::error::DbError::constraint),
		Some("waiting_usage_wake_authority_time_monotonic"),
	);
	owner.batch_execute("ROLLBACK").await?;
	Ok(())
}

async fn register_waiting_wakes(
	owner: &Client,
	routing: &RoutingFixture,
	registration_operation: &str,
	ready_at: i64,
	registration_at: i64,
	equal_registration_at: i64,
	equal_ready_at: i64,
) -> Result<RegisteredWakes, Box<dyn std::error::Error>> {
	let registered_bytes = internal_register(
		owner,
		"v19-register",
		registration_operation,
		&routing.waiting.decision_id,
		1,
		registration_at,
	)
	.await?;
	let registered = assert_success(&registered_bytes, "registered");
	assert_eq!(integer(&registered, "earliest_ready_at_micros"), ready_at);
	assert_eq!(integer(&registered, "registered_at_micros"), registration_at);
	assert_eq!(integer(&registered, "transitioned_at_micros"), registration_at);
	assert_transition_readback(owner, &registered_bytes).await?;

	let stale_registration_operation = uuid(0xa8, 9);
	let reordered_stale_bytes = internal_register(
		owner,
		"v19-register-stale",
		&stale_registration_operation,
		&routing.stale_waiting.decision_id,
		1,
		equal_registration_at,
	)
	.await?;
	let reordered_stale = assert_success(&reordered_stale_bytes, "registered");
	assert_eq!(text(&reordered_stale, "routing_policy_id"), routing.stale_policy_id,);
	let cancel_registration_operation = uuid(0xa8, 6);
	let reordered_cancel_bytes = internal_register(
		owner,
		"v19-register-cancel",
		&cancel_registration_operation,
		&routing.cancel_waiting.decision_id,
		1,
		equal_registration_at,
	)
	.await?;
	let reordered_cancel = assert_success(&reordered_cancel_bytes, "registered");
	for equal_ready in [&reordered_stale, &reordered_cancel] {
		assert_eq!(integer(equal_ready, "earliest_ready_at_micros"), equal_ready_at);
		assert_eq!(integer(equal_ready, "registered_at_micros"), equal_registration_at);
	}
	let mut expected_wake_order = [
		(
			integer(&registered, "earliest_ready_at_micros"),
			integer(&registered, "registered_at_micros"),
			text(&registered, "wake_id").to_owned(),
		),
		(
			integer(&reordered_stale, "earliest_ready_at_micros"),
			integer(&reordered_stale, "registered_at_micros"),
			text(&reordered_stale, "wake_id").to_owned(),
		),
		(
			integer(&reordered_cancel, "earliest_ready_at_micros"),
			integer(&reordered_cancel, "registered_at_micros"),
			text(&reordered_cancel, "wake_id").to_owned(),
		),
	];
	expected_wake_order.sort();
	assert_eq!(expected_wake_order[0].2.as_str(), text(&registered, "wake_id"));
	assert!(expected_wake_order[0].0 < expected_wake_order[1].0);
	assert!(expected_wake_order[0].1 > expected_wake_order[1].1);
	assert_eq!(expected_wake_order[1].0, expected_wake_order[2].0);
	assert_eq!(expected_wake_order[1].1, expected_wake_order[2].1);
	assert!(expected_wake_order[1].2 < expected_wake_order[2].2);
	Ok(RegisteredWakes {
		registered_bytes,
		registered,
		reordered_stale_bytes,
		reordered_stale,
		reordered_cancel_bytes,
		reordered_cancel,
		expected_wake_order,
	})
}

async fn assert_initial_claims(
	owner: &Client,
	registered: &RegisteredWakes,
	registration_at: i64,
	ready_at: i64,
	equal_ready_at: i64,
) -> Result<ClaimedWake, Box<dyn std::error::Error>> {
	let before_ready = internal_claim(
		owner,
		"v19-claim-before-ready",
		&uuid(0xaa, 1),
		&uuid(0xab, 1),
		&uuid(0xac, 1),
		registration_at,
	)
	.await?;
	assert_rejection(&before_ready, "no_due_wake");

	let rollback_claim_key = "v19-claim-exact-ready-rollback";
	let rollback_claim_operation = uuid(0xaa, 2);
	owner.batch_execute("BEGIN").await?;
	let rollback_claim_bytes = internal_claim(
		owner,
		rollback_claim_key,
		&rollback_claim_operation,
		&uuid(0xab, 2),
		&uuid(0xac, 2),
		ready_at,
	)
	.await?;
	let rollback_claim = assert_success(&rollback_claim_bytes, "claimed");
	assert_eq!(text(&rollback_claim, "wake_id"), registered.expected_wake_order[0].2.as_str(),);
	assert_eq!(integer(&rollback_claim, "transitioned_at_micros"), ready_at);
	let rollback_claim_outbox_id = integer(&rollback_claim["outbox_effects"][0], "id");
	owner.batch_execute("ROLLBACK").await?;
	assert_claim_cluster_absent(
		owner,
		rollback_claim_key,
		&rollback_claim_operation,
		rollback_claim_outbox_id,
	)
	.await?;

	let claim_operation = uuid(0xaa, 5);
	let claim_id = uuid(0xab, 5);
	let first_holder = uuid(0xac, 5);
	let claimed_bytes = internal_claim(
		owner,
		"v19-claim-complete-order",
		&claim_operation,
		&claim_id,
		&first_holder,
		equal_ready_at,
	)
	.await?;
	let claimed = assert_success(&claimed_bytes, "claimed");
	assert_eq!(text(&claimed, "wake_id"), registered.expected_wake_order[0].2.as_str());
	assert_eq!(integer(&claimed, "transitioned_at_micros"), equal_ready_at);
	let first_expiry = integer(&claimed, "lease_expires_at_micros");
	assert_eq!(first_expiry, equal_ready_at + 60_000_000);
	let first_fence = text(&claimed, "lease_fence_id").to_owned();
	assert_transition_readback(owner, &claimed_bytes).await?;
	Ok(ClaimedWake {
		claimed_bytes,
		claimed,
		claim_operation,
		claim_id,
		first_holder,
		first_expiry,
		first_fence,
	})
}

async fn assert_supersession_and_cancellation(
	store: &PostgresStore,
	schema_owner: &Config,
	owner: &Client,
	registered: &RegisteredWakes,
	equal_ready_at: i64,
) -> Result<(), Box<dyn std::error::Error>> {
	let (supersede_registered, cancel_registered_bytes) =
		if text(&registered.reordered_stale, "wake_id")
			< text(&registered.reordered_cancel, "wake_id")
		{
			(&registered.reordered_stale, &registered.reordered_cancel_bytes)
		} else {
			(&registered.reordered_cancel, &registered.reordered_stale_bytes)
		};
	assert_eq!(text(supersede_registered, "wake_id"), registered.expected_wake_order[1].2.as_str(),);
	let cancel_registered = assert_success(cancel_registered_bytes, "registered");
	assert_eq!(text(&cancel_registered, "wake_id"), registered.expected_wake_order[2].2.as_str(),);
	advance_stale_policy(store, owner, text(supersede_registered, "routing_policy_id")).await?;
	let superseded_bytes = internal_claim(
		owner,
		"v19-claim-stale-lineage",
		&uuid(0xaa, 9),
		&uuid(0xab, 9),
		&uuid(0xac, 9),
		equal_ready_at,
	)
	.await?;
	let superseded = assert_success(&superseded_bytes, "superseded");
	assert_eq!(text(&superseded, "wake_id"), registered.expected_wake_order[1].2.as_str(),);
	assert_eq!(text(&superseded, "terminal_reason"), "policy_revision_stale");
	assert!(superseded.get("routing_resolution_request_id").is_some_and(Value::is_null));
	assert_transition_readback(owner, &superseded_bytes).await?;

	let cancelled_bytes =
		assert_cancellation_race(schema_owner, owner, cancel_registered_bytes, equal_ready_at)
			.await?;
	assert_transition_readback(owner, &cancelled_bytes).await?;
	let superseded_terminal = internal_cancel(
		owner,
		"v19-cancel-superseded",
		&uuid(0xae, 9),
		text(supersede_registered, "wake_id"),
		integer(&superseded, "revision"),
		text(&superseded, "transition_id"),
		equal_ready_at + 1,
	)
	.await?;
	assert_rejection(&superseded_terminal, "wake_terminal");
	Ok(())
}

async fn reclaim_waiting_wake(
	owner: &Client,
	claimed: &ClaimedWake,
) -> Result<ReclaimedWake, Box<dyn std::error::Error>> {
	let pre_expiry = internal_claim(
		owner,
		"v19-reclaim-before-expiry",
		&uuid(0xaa, 3),
		&uuid(0xab, 3),
		&uuid(0xac, 3),
		claimed.first_expiry - 1,
	)
	.await?;
	assert_rejection(&pre_expiry, "no_due_wake");

	let reclaim_operation = uuid(0xaa, 4);
	let second_holder = uuid(0xac, 4);
	let reclaimed_bytes = internal_claim(
		owner,
		"v19-reclaim-exact-expiry",
		&reclaim_operation,
		&uuid(0xab, 4),
		&second_holder,
		claimed.first_expiry,
	)
	.await?;
	let reclaimed = assert_success(&reclaimed_bytes, "reclaimed");
	let second_fence = text(&reclaimed, "lease_fence_id").to_owned();
	assert_ne!(second_fence, claimed.first_fence);
	assert_eq!(integer(&reclaimed, "transitioned_at_micros"), claimed.first_expiry);
	assert_transition_readback(owner, &reclaimed_bytes).await?;
	Ok(ReclaimedWake { reclaimed, second_holder, second_fence })
}

async fn assert_fire_contract(
	owner: &Client,
	routing: &RoutingFixture,
	registration_operation: &str,
	registered: &RegisteredWakes,
	claimed: &ClaimedWake,
	reclaimed: &ReclaimedWake,
) -> Result<(), Box<dyn std::error::Error>> {
	let wake_id = text(&reclaimed.reclaimed, "wake_id");
	let reclaimed_revision = integer(&reclaimed.reclaimed, "revision");
	let reclaimed_transition = text(&reclaimed.reclaimed, "transition_id");
	let stale_fence = internal_fire(
		owner,
		"v19-fire-stale-fence",
		&uuid(0xad, 1),
		wake_id,
		reclaimed_revision,
		reclaimed_transition,
		&reclaimed.second_holder,
		&claimed.first_fence,
		claimed.first_expiry,
	)
	.await?;
	assert_rejection(&stale_fence, "lease_lost");

	let stale_tip = internal_fire(
		owner,
		"v19-fire-stale-tip",
		&uuid(0xad, 2),
		wake_id,
		integer(&claimed.claimed, "revision"),
		text(&claimed.claimed, "transition_id"),
		&reclaimed.second_holder,
		&reclaimed.second_fence,
		claimed.first_expiry,
	)
	.await?;
	assert_rejection(&stale_tip, "stale_wake_tip");

	let fire_at = claimed.first_expiry + 1;
	let fired_bytes = internal_fire(
		owner,
		"v19-fire-valid",
		&uuid(0xad, 3),
		wake_id,
		reclaimed_revision,
		reclaimed_transition,
		&reclaimed.second_holder,
		&reclaimed.second_fence,
		fire_at,
	)
	.await?;
	let fired = assert_success(&fired_bytes, "fired");
	assert_eq!(integer(&fired, "transitioned_at_micros"), fire_at);
	assert!(fired.get("routing_resolution_request_id").and_then(Value::as_str).is_some());
	assert_eq!(fired.get("fresh_routing_resolution_only"), Some(&Value::Bool(true)));
	assert_eq!(fired.get("prior_decision_reusable"), Some(&Value::Bool(false)));
	assert_eq!(fired.get("production_enabled"), Some(&Value::Bool(false)));
	for forbidden in [
		"candidates",
		"quota_evidence",
		"eligibility",
		"exclusions",
		"selected_account",
		"credentials",
		"continuation",
		"dispatch",
		"retry_authority",
	] {
		assert!(fired.get(forbidden).is_none(), "fired result leaked {forbidden}");
	}
	assert_transition_readback(owner, &fired_bytes).await?;

	let terminal = internal_cancel(
		owner,
		"v19-cancel-fired",
		&uuid(0xae, 1),
		wake_id,
		integer(&fired, "revision"),
		text(&fired, "transition_id"),
		fire_at + 1,
	)
	.await?;
	assert_rejection(&terminal, "wake_terminal");

	let cross_key_claim = internal_claim(
		owner,
		"v19-claim-cross-key-after-fire",
		&claimed.claim_operation,
		&claimed.claim_id,
		&claimed.first_holder,
		fire_at + 2,
	)
	.await?;
	assert_eq!(cross_key_claim, claimed.claimed_bytes);
	let cross_key_registration = internal_register(
		owner,
		"v19-register-cross-key-after-fire",
		registration_operation,
		&routing.waiting.decision_id,
		1,
		fire_at + 3,
	)
	.await?;
	assert_eq!(cross_key_registration, registered.registered_bytes);
	Ok(())
}

async fn assert_restart_replay(
	runtime: &Config,
	routing: &RoutingFixture,
	registration_operation: String,
	registered: &RegisteredWakes,
) -> Result<(), Box<dyn std::error::Error>> {
	let restarted =
		PostgresStore::connect_runtime_fixture(runtime.clone(), super::expected_peer_uid()).await?;
	let restart_replay = restarted
		.register_waiting_usage_wake(
			"v19-register",
			&RegisterWaitingUsageWake {
				operation_id: registration_operation,
				routing_decision_id: routing.waiting.decision_id.clone(),
				expected_managed_run_revision: 1,
			},
		)
		.await?;
	assert!(matches!(restart_replay, WaitingUsageWakeCommandOutcome::Success(
		ref transition
	) if transition.transition_id == text(&registered.registered, "transition_id")));
	Ok(())
}

async fn assert_cancellation_race(
	schema_owner: &Config,
	owner: &Client,
	registered_bytes: &[u8],
	ready_at: i64,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
	let registered = assert_success(registered_bytes, "registered");
	let wake_id = text(&registered, "wake_id").to_owned();
	let revision = integer(&registered, "revision");
	let transition_id = text(&registered, "transition_id").to_owned();

	let (client_a, connection_a) = schema_owner.clone().connect(NoTls).await?;
	let (client_b, connection_b) = schema_owner.clone().connect(NoTls).await?;
	let connection_a = tokio::spawn(connection_a);
	let connection_b = tokio::spawn(connection_b);
	let mut racers = JoinSet::new();
	for (client, marker) in [(client_a, 7_u8), (client_b, 8_u8)] {
		let wake_id = wake_id.clone();
		let transition_id = transition_id.clone();
		racers.spawn(async move {
			let response = internal_cancel(
				&client,
				&format!("v19-cancel-race-{marker}"),
				&uuid(0xae, marker),
				&wake_id,
				revision,
				&transition_id,
				ready_at,
			)
			.await;
			drop(client);
			response
		});
	}
	let mut cancelled = None;
	let mut terminal_rejections = 0;
	while let Some(result) = racers.join_next().await {
		let response = result??;
		let envelope = envelope(&response);
		match envelope["classification"].as_str() {
			Some("success") => {
				assert_eq!(envelope["effect"]["transition_kind"], "cancelled");
				assert!(cancelled.replace(response).is_none());
			},
			Some("stable_domain_rejection") => {
				assert_eq!(envelope["effect"]["rejection"], "wake_terminal");
				terminal_rejections += 1;
			},
			other =>
				return Err(format!("unexpected cancellation race classification: {other:?}").into()),
		}
	}
	connection_a.await??;
	connection_b.await??;
	assert_eq!(terminal_rejections, 1);
	let cancelled = cancelled.expect("one cancellation wins");

	let replay = internal_register(
		owner,
		"v19-register-cancel-cross-key",
		text(&registered, "operation_id"),
		text(&registered, "routing_decision_id"),
		integer(&registered, "managed_run_revision"),
		ready_at + 1,
	)
	.await?;
	assert_eq!(replay.as_slice(), registered_bytes);
	Ok(cancelled)
}

async fn assert_v19_catalog_authority(
	owner: &Client,
	runtime: &Config,
) -> Result<(), Box<dyn std::error::Error>> {
	let runtime_role = runtime.get_user().ok_or("runtime role is absent")?;
	let row = owner
		.query_one(
			concat!(
				"WITH internal AS (SELECT proc.oid,proc.proowner,proc.proacl,proc.prosecdef,",
				"proc.provolatile,proc.proparallel,proc.proconfig,",
				"proc.proname,proc.pronargs ",
				"FROM pg_catalog.pg_proc AS proc JOIN pg_catalog.pg_namespace AS namespace ",
				"ON namespace.oid=proc.pronamespace WHERE namespace.nspname='decodex' ",
				"AND proc.proname IN ('register_waiting_usage_wake_exact_internal',",
				"'claim_due_waiting_usage_wake_exact_internal',",
				"'fire_waiting_usage_wake_exact_internal',",
				"'cancel_waiting_usage_wake_exact_internal')), ",
				"wrappers AS (SELECT proc.oid,proc.prosecdef,proc.proname,proc.pronargs ",
				"FROM pg_catalog.pg_proc AS proc ",
				"JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid=proc.pronamespace ",
				"WHERE namespace.nspname='decodex' AND proc.proname IN ",
				"('register_waiting_usage_wake_exact','claim_due_waiting_usage_wake_exact',",
				"'fire_waiting_usage_wake_exact','cancel_waiting_usage_wake_exact')) ",
				"SELECT (SELECT count(*)=4 AND bool_and(NOT prosecdef AND provolatile='v' ",
				"AND proparallel='u' AND proconfig=ARRAY['search_path=pg_catalog, decodex']) ",
				"FROM internal),(SELECT bool_and(NOT pg_catalog.has_function_privilege(",
				"$1,oid,'EXECUTE')) FROM internal),(SELECT NOT EXISTS (SELECT 1 FROM internal ",
				"CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(proacl,",
				"pg_catalog.acldefault('f',proowner))) AS acl WHERE acl.privilege_type='EXECUTE' ",
				"AND acl.grantee<>internal.proowner)),(SELECT count(*)=4 AND bool_and(prosecdef ",
				"AND pronargs=CASE proname WHEN 'register_waiting_usage_wake_exact' THEN 5 ",
				"WHEN 'claim_due_waiting_usage_wake_exact' THEN 5 ",
				"WHEN 'fire_waiting_usage_wake_exact' THEN 8 ",
				"WHEN 'cancel_waiting_usage_wake_exact' THEN 6 END) FROM wrappers),",
				"(SELECT count(*)=4 AND bool_and(pg_catalog.has_function_privilege(",
				"$1,oid,'EXECUTE')) FROM wrappers)",
			),
			&[&runtime_role],
		)
		.await?;
	let invariants = [
		"four internal functions retain exact metadata and settings",
		"runtime role cannot execute the four internal functions",
		"internal function ACLs grant EXECUTE only to each owner",
		"four wrappers retain exact security and arity metadata",
		"runtime role can execute all four wrappers",
	];
	for (index, invariant) in invariants.iter().enumerate() {
		assert!(row.get::<_, bool>(index), "V19 catalog authority invariant failed: {invariant}",);
	}
	Ok(())
}

async fn assert_cluster_absent(
	owner: &Client,
	key: &str,
	operation_id: &str,
	outbox_id: i64,
) -> Result<(), Box<dyn std::error::Error>> {
	let row = owner
		.query_one(
			concat!(
				"SELECT (SELECT count(*) FROM decodex.exact_command_receipts ",
				"WHERE idempotency_key=$1)=0,",
				"(SELECT count(*) FROM decodex.waiting_usage_wake_transitions ",
				"WHERE operation_id=$2::text::uuid)=0,",
				"(SELECT count(*) FROM decodex.waiting_usage_wake_heads ",
				"WHERE registration_operation_id=$2::text::uuid)=0,",
				"(SELECT count(*) FROM decodex.activity WHERE correlation_key=$2)=0,",
				"(SELECT count(*) FROM decodex.outbox WHERE id=$3)=0",
			),
			&[&key, &operation_id, &outbox_id],
		)
		.await?;
	for index in 0..5 {
		assert!(row.get::<_, bool>(index), "rolled-back cluster component {index} survived");
	}
	Ok(())
}

async fn assert_claim_cluster_absent(
	owner: &Client,
	key: &str,
	operation_id: &str,
	outbox_id: i64,
) -> Result<(), Box<dyn std::error::Error>> {
	let row = owner
		.query_one(
			concat!(
				"SELECT (SELECT count(*) FROM decodex.exact_command_receipts ",
				"WHERE idempotency_key=$1)=0,",
				"(SELECT count(*) FROM decodex.waiting_usage_wake_transitions ",
				"WHERE operation_id=$2::text::uuid)=0,",
				"(SELECT count(*) FROM decodex.activity WHERE correlation_key=$2)=0,",
				"(SELECT count(*) FROM decodex.outbox WHERE id=$3)=0",
			),
			&[&key, &operation_id, &outbox_id],
		)
		.await?;
	for index in 0..4 {
		assert!(row.get::<_, bool>(index), "rolled-back claim component {index} survived",);
	}
	Ok(())
}

async fn assert_transition_readback(
	owner: &Client,
	response: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
	let parsed = envelope(response);
	let kind = text(&parsed["effect"], "transition_kind").to_owned();
	let effect = assert_success(response, &kind);
	let row = owner
		.query_one(
			concat!(
				"SELECT effect_envelope,response_bytes FROM ",
				"decodex.read_waiting_usage_wake_transition_exact(",
				"$1::text::uuid,$2::text::uuid)",
			),
			&[&text(&effect, "transition_id"), &text(&effect, "operation_id")],
		)
		.await?;
	assert_eq!(row.get::<_, Value>(0), effect);
	assert_eq!(row.get::<_, Vec<u8>>(1), response);
	Ok(())
}

async fn assert_exact_chain(owner: &Client) -> Result<(), Box<dyn std::error::Error>> {
	let row = owner
		.query_one(
			concat!(
				"SELECT (SELECT count(*) FROM decodex.waiting_usage_wake_heads)=3,",
				"(SELECT count(*) FROM decodex.waiting_usage_wake_heads ",
				"WHERE state='fired')=1,",
				"(SELECT count(*) FROM decodex.waiting_usage_wake_heads ",
				"WHERE state='cancelled')=1,",
				"(SELECT count(*) FROM decodex.waiting_usage_wake_heads ",
				"WHERE state='superseded')=1,",
				"(SELECT count(*) FROM decodex.waiting_usage_wake_transitions)=8,",
				"(SELECT count(*) FROM decodex.waiting_usage_wake_transitions ",
				"WHERE transition_kind='registered')=3,",
				"(SELECT count(*) FROM decodex.waiting_usage_wake_transitions ",
				"WHERE transition_kind='claimed')=1,",
				"(SELECT count(*) FROM decodex.waiting_usage_wake_transitions ",
				"WHERE transition_kind='reclaimed')=1,",
				"(SELECT count(*) FROM decodex.waiting_usage_wake_transitions ",
				"WHERE transition_kind='fired')=1,",
				"(SELECT count(*) FROM decodex.waiting_usage_wake_transitions ",
				"WHERE transition_kind='cancelled')=1,",
				"(SELECT count(*) FROM decodex.waiting_usage_wake_transitions ",
				"WHERE transition_kind='superseded')=1,",
				"NOT EXISTS (SELECT 1 FROM decodex.waiting_usage_wake_heads AS head ",
				"JOIN decodex.waiting_usage_wake_transitions AS transition ",
				"ON transition.transition_id=head.transition_id ",
				"WHERE head.revision<>transition.revision ",
				"OR head.state<>transition.state),",
				"(SELECT count(*) FROM decodex.activity ",
				"WHERE aggregate_kind='waiting_usage_wake')=8,",
				"(SELECT count(*) FROM decodex.outbox ",
				"WHERE aggregate_kind='waiting_usage_wake')=8",
			),
			&[],
		)
		.await?;
	for index in 0..14 {
		assert!(row.get::<_, bool>(index), "V19 exact chain assertion {index}");
	}
	Ok(())
}

async fn internal_register(
	client: &Client,
	key: &str,
	operation_id: &str,
	decision_id: &str,
	revision: i64,
	now_micros: i64,
) -> Result<Vec<u8>, tokio_postgres::Error> {
	Ok(client
		.query_one(
			concat!(
				"SELECT decodex.register_waiting_usage_wake_exact_internal(",
				"$1,$2,$3::text::uuid,$4::text::uuid,$5,",
				"TIMESTAMPTZ 'epoch'",
				" + ($6::bigint / 1000000) * INTERVAL '1 second'",
				" + ($6::bigint % 1000000) * INTERVAL '1 microsecond')",
			),
			&[&PROTOCOL, &key, &operation_id, &decision_id, &revision, &now_micros],
		)
		.await?
		.get(0))
}

async fn internal_claim(
	client: &Client,
	key: &str,
	operation_id: &str,
	claim_id: &str,
	holder_id: &str,
	now_micros: i64,
) -> Result<Vec<u8>, tokio_postgres::Error> {
	Ok(client
		.query_one(
			concat!(
				"SELECT decodex.claim_due_waiting_usage_wake_exact_internal(",
				"$1,$2,$3::text::uuid,$4::text::uuid,$5::text::uuid,",
				"TIMESTAMPTZ 'epoch'",
				" + ($6::bigint / 1000000) * INTERVAL '1 second'",
				" + ($6::bigint % 1000000) * INTERVAL '1 microsecond')",
			),
			&[&PROTOCOL, &key, &operation_id, &claim_id, &holder_id, &now_micros],
		)
		.await?
		.get(0))
}

#[allow(clippy::too_many_arguments)]
async fn internal_fire(
	client: &Client,
	key: &str,
	operation_id: &str,
	wake_id: &str,
	revision: i64,
	transition_id: &str,
	holder_id: &str,
	fence_id: &str,
	now_micros: i64,
) -> Result<Vec<u8>, tokio_postgres::Error> {
	Ok(client
		.query_one(
			concat!(
				"SELECT decodex.fire_waiting_usage_wake_exact_internal(",
				"$1,$2,$3::text::uuid,$4::text::uuid,$5,$6::text::uuid,",
				"$7::text::uuid,$8::text::uuid,",
				"TIMESTAMPTZ 'epoch'",
				" + ($9::bigint / 1000000) * INTERVAL '1 second'",
				" + ($9::bigint % 1000000) * INTERVAL '1 microsecond')",
			),
			&[
				&PROTOCOL,
				&key,
				&operation_id,
				&wake_id,
				&revision,
				&transition_id,
				&holder_id,
				&fence_id,
				&now_micros,
			],
		)
		.await?
		.get(0))
}

async fn internal_cancel(
	client: &Client,
	key: &str,
	operation_id: &str,
	wake_id: &str,
	revision: i64,
	transition_id: &str,
	now_micros: i64,
) -> Result<Vec<u8>, tokio_postgres::Error> {
	Ok(client
		.query_one(
			concat!(
				"SELECT decodex.cancel_waiting_usage_wake_exact_internal(",
				"$1,$2,$3::text::uuid,$4::text::uuid,$5,$6::text::uuid,",
				"TIMESTAMPTZ 'epoch'",
				" + ($7::bigint / 1000000) * INTERVAL '1 second'",
				" + ($7::bigint % 1000000) * INTERVAL '1 microsecond')",
			),
			&[&PROTOCOL, &key, &operation_id, &wake_id, &revision, &transition_id, &now_micros],
		)
		.await?
		.get(0))
}

fn envelope(response: &[u8]) -> Value {
	serde_json::from_slice(response).expect("wake response is canonical JSON")
}

fn assert_success(response: &[u8], kind: &str) -> Value {
	let envelope = envelope(response);
	assert_eq!(envelope["classification"], "success");
	assert_eq!(envelope["effect"]["transition_kind"], kind);
	envelope["effect"].clone()
}

fn assert_rejection(response: &[u8], rejection: &str) {
	let envelope = envelope(response);
	assert_eq!(envelope["classification"], "stable_domain_rejection");
	assert_eq!(envelope["effect"]["rejection"], rejection);
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
	value[key].as_str().unwrap_or_else(|| panic!("wake effect {key} is not text"))
}

fn integer(value: &Value, key: &str) -> i64 {
	value[key].as_i64().unwrap_or_else(|| panic!("wake effect {key} is not an integer"))
}

fn uuid(prefix: u8, marker: u8) -> String {
	format!("{prefix:02x}000000-0000-4000-8000-{marker:012}")
}

pub(super) async fn assert_restored_waiting_wake_contract(
	client: &Client,
) -> Result<(), Box<dyn std::error::Error>> {
	assert_exact_chain(client).await
}
