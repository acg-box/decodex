//! Inert, exactly-once continuation plans over persisted V16 decisions.

use decodex_core::{
	AccountId, BlobStore, ContextPack, ContinuationCommandOutcome, ContinuationPlan,
	ContinuationPlanKind, ContinuationRejection, ConversationId, ManagedRunId, RuntimeSessionId,
	SameThreadContinuationEvidence, MAX_INLINE_HISTORY_BYTES,
};
use serde_json::Value;
use sha2::{Digest as _, Sha256};

use crate::{
	ContextPackRecord, PostgresStore, StoreError,
	conversations::{
		context_disposition_sql, context_pack_referenced_hashes, context_source_sql,
		publish_verified_blob, side_effect_sql, validate_context_pack,
	},
	exact_commands::{EXACT_COMMAND_PROTOCOL, validate_exact_key},
};

/// Caller identities and optimistic coordinate for one V16 decision consumption.
/// No field can supply routing policy, candidates, evidence, exclusions, or selection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlanContinuation {
	pub operation_id: String,
	pub routing_decision_id: String,
	pub expected_managed_run_revision: i64,
	pub plan_id: String,
	pub fallback_runtime_session_id: String,
	pub fallback_account_snapshot_id: String,
	pub fallback_context_pack_id: String,
}

/// Strict committed readback. A fallback includes the fully byte- and manifest-verified pack.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContinuationPlanEffect {
	pub plan: ContinuationPlan,
	pub fallback_context_pack: Option<ContextPackRecord>,
}

impl PostgresStore {
	/// Consume one exact selected V16 decision into one inert continuation plan.
	pub async fn plan_continuation(
		&self,
		blob_store: &BlobStore,
		idempotency_key: &str,
		request: &PlanContinuation,
		fallback_pack: &ContextPack,
	) -> Result<ContinuationCommandOutcome<ContinuationPlanEffect>, StoreError> {
		validate_exact_key(idempotency_key)?;
		for (value, label) in [
			(&request.operation_id, "continuation operation identity"),
			(&request.routing_decision_id, "routing decision identity"),
			(&request.plan_id, "continuation plan identity"),
			(&request.fallback_runtime_session_id, "fallback RuntimeSession identity"),
			(&request.fallback_account_snapshot_id, "fallback account snapshot identity"),
			(&request.fallback_context_pack_id, "fallback Context Pack identity"),
		] {
			validate_uuid(value, label)?;
		}
		if request.expected_managed_run_revision <= 0 {
			return Err(StoreError::InvalidInput("ManagedRun revision must be positive"));
		}
		validate_context_pack(
			&crate::PersistContextPack {
				context_pack_id: request.fallback_context_pack_id.clone(),
				pack_revision: 1,
			},
			fallback_pack,
		)?;

		let blob = (fallback_pack.bytes().len() > MAX_INLINE_HISTORY_BYTES).then(|| {
			(
				fallback_pack.digest(),
				i64::try_from(fallback_pack.bytes().len()).unwrap_or(i64::MAX),
			)
		});
		let referenced_hashes = context_pack_referenced_hashes(fallback_pack, blob);
		let capacity_hashes = blob.map(|(hash, _)| vec![hash]).unwrap_or_default();
		let _publication = if referenced_hashes.is_empty() {
			None
		} else {
			let publication = self.lock_blob_session(&referenced_hashes, &capacity_hashes).await?;
			if let Some((hash, _)) = blob {
				publish_verified_blob(blob_store, hash, fallback_pack.bytes())?;
			}
			Some(publication)
		};

		let source_kinds = fallback_pack
			.source_manifest()
			.iter()
			.map(|source| context_source_sql(source.kind()).to_owned())
			.collect::<Vec<_>>();
		let source_ids = fallback_pack
			.source_manifest()
			.iter()
			.map(|source| source.source_id().to_owned())
			.collect::<Vec<_>>();
		let source_revisions = fallback_pack
			.source_manifest()
			.iter()
			.map(|source| i64::try_from(source.revision()).unwrap_or(i64::MAX))
			.collect::<Vec<_>>();
		let content_digests = fallback_pack
			.source_manifest()
			.iter()
			.map(|source| source.content_digest().to_hex())
			.collect::<Vec<_>>();
		let original_lengths = fallback_pack
			.source_manifest()
			.iter()
			.map(|source| i64::try_from(source.original_byte_length()).unwrap_or(i64::MAX))
			.collect::<Vec<_>>();
		let included_lengths = fallback_pack
			.source_manifest()
			.iter()
			.map(|source| i64::try_from(source.included_byte_length()).unwrap_or(i64::MAX))
			.collect::<Vec<_>>();
		let included_digests = fallback_pack
			.source_manifest()
			.iter()
			.map(|source| source.included_digest().to_hex())
			.collect::<Vec<_>>();
		let dispositions = fallback_pack
			.source_manifest()
			.iter()
			.map(|source| context_disposition_sql(source.disposition()).to_owned())
			.collect::<Vec<_>>();
		let artifact_ids = fallback_pack
			.source_manifest()
			.iter()
			.map(|source| {
				source
					.artifact_reference()
					.map_or_else(String::new, |(id, _)| id.as_str().to_owned())
			})
			.collect::<Vec<_>>();
		let artifact_revisions = fallback_pack
			.source_manifest()
			.iter()
			.map(|source| {
				source
					.artifact_reference()
					.and_then(|(_, revision)| i64::try_from(revision).ok())
					.unwrap_or(0)
			})
			.collect::<Vec<_>>();
		let compiled_bytes = fallback_pack.bytes().to_vec();
		let max_bytes = i32::try_from(fallback_pack.policy().max_bytes()).unwrap_or(i32::MAX);
		let recent_item_limit =
			i32::try_from(fallback_pack.policy().recent_item_limit()).unwrap_or(i32::MAX);
		let omitted_source_count =
			i32::try_from(fallback_pack.omitted_source_count()).unwrap_or(i32::MAX);
		let possible_side_effects = side_effect_sql(fallback_pack.possible_side_effects());
		let compiled_digest = fallback_pack.digest().to_hex();
		let manifest_digest = fallback_pack.manifest_digest().to_hex();
		let truncated = fallback_pack.truncated();
		let response = self
			.execute_exact_with_retry(
				"SELECT decodex.plan_continuation_exact(\
				 $1,$2,$3::text::uuid,$4::text::uuid,$5,$6::text::uuid,$7::text::uuid,\
				 $8::text::uuid,$9::text::uuid,$10,$11,$12,$13,$14,$15,$16,$17,\
				 $18::text[],$19::text[],$20::bigint[],$21::text[],$22::bigint[],\
				 $23::bigint[],$24::text[],$25::text[],$26::text[],$27::bigint[])",
				&[
					&EXACT_COMMAND_PROTOCOL,
					&idempotency_key,
					&request.operation_id,
					&request.routing_decision_id,
					&request.expected_managed_run_revision,
					&request.plan_id,
					&request.fallback_runtime_session_id,
					&request.fallback_account_snapshot_id,
					&request.fallback_context_pack_id,
					&compiled_bytes,
					&compiled_digest,
					&manifest_digest,
					&max_bytes,
					&recent_item_limit,
					&possible_side_effects,
					&truncated,
					&omitted_source_count,
					&source_kinds,
					&source_ids,
					&source_revisions,
					&content_digests,
					&original_lengths,
					&included_lengths,
					&included_digests,
					&dispositions,
					&artifact_ids,
					&artifact_revisions,
				],
			)
			.await?;
		let (classification, effect) = parse_envelope(&response)?;
		if classification == "stable_domain_rejection" {
			return Ok(ContinuationCommandOutcome::Rejected(parse_rejection(&effect)?));
		}
		let plan = parse_plan(&effect)?;
		if plan.plan_id != request.plan_id
			|| plan.operation_id != request.operation_id
			|| plan.routing_decision_id != request.routing_decision_id
			|| plan.managed_run_revision != request.expected_managed_run_revision
		{
			return incompatible("stored continuation response is cross-linked");
		}
		verify_effect_rows(&effect, &plan)?;
		self.verify_plan_readback(&response, &effect, &plan).await?;
		let fallback_context_pack = match plan.kind {
			ContinuationPlanKind::SameThread => None,
			ContinuationPlanKind::ContextPackFallback => {
				let context_pack_id = plan
					.fallback_context_pack_id
					.as_deref()
					.ok_or_else(|| StoreError::Incompatible("fallback plan lost Context Pack".into()))?;
				let record = self.required_context_pack(blob_store, context_pack_id).await?;
				if record.compiled_digest != fallback_pack.digest()
					|| record.conversation_id != plan.conversation_id
					|| record.pack_revision
						!= positive_i64(&effect, "fallback_context_pack_revision")?
				{
					return incompatible("fallback Context Pack readback is cross-linked");
				}
				Some(record)
			},
		};
		Ok(ContinuationCommandOutcome::Success(ContinuationPlanEffect {
			plan,
			fallback_context_pack,
		}))
	}

	async fn verify_plan_readback(
		&self,
		response: &[u8],
		effect: &Value,
		plan: &ContinuationPlan,
	) -> Result<(), StoreError> {
		let client = self.pool().get().await?;
		let expected_revision = 1_i64;
		let row = client
			.query_opt(
				"SELECT * FROM decodex.read_continuation_plan_exact($1::text::uuid,$2)",
				&[&plan.plan_id, &expected_revision],
			)
			.await?
			.ok_or_else(|| StoreError::Incompatible("continuation receipt lost its plan".into()))?;
		let stored_response: Vec<u8> = row.get(0);
		let stored_effect: Value = row.get(1);
		let stored_kind: &str = row.get(2);
		let stored_thread: Option<String> = row.get(3);
		let stored_pack: Option<String> = row.get(4);
		let stored_session: Option<String> = row.get(5);
		if stored_response != response
			|| stored_effect != *effect
			|| stored_kind != plan_kind_sql(plan.kind)
			|| stored_thread != plan.codex_thread_id
			|| stored_pack != plan.fallback_context_pack_id
			|| stored_session.as_deref()
				!= plan.fallback_runtime_session_id.as_ref().map(RuntimeSessionId::as_str)
			|| row.get::<_, bool>(6)
			|| row.get::<_, bool>(7)
		{
			return incompatible("continuation plan strict readback differs from its receipt");
		}
		Ok(())
	}
}

fn parse_envelope(bytes: &[u8]) -> Result<(String, Value), StoreError> {
	let document: Value = serde_json::from_slice(bytes)
		.map_err(|_| StoreError::Incompatible("stored continuation response is malformed".into()))?;
	require_keys(&document, &["classification", "effect"])?;
	let classification = text(&document, "classification")?;
	if !matches!(classification, "completed_success" | "stable_domain_rejection") {
		return incompatible("stored continuation response classification is unknown");
	}
	let effect = document
		.get("effect")
		.filter(|value| value.is_object())
		.ok_or_else(|| StoreError::Incompatible("stored continuation effect is malformed".into()))?;
	verify_digest(effect)?;
	if text(effect, "operation")? != "plan_continuation" {
		return incompatible("stored continuation operation is cross-linked");
	}
	Ok((classification.to_owned(), effect.clone()))
}

fn parse_rejection(effect: &Value) -> Result<ContinuationRejection, StoreError> {
	require_keys(
		effect,
		&["effect_digest", "effect_digest_source", "operation", "rejection"],
	)?;
	match text(effect, "rejection")? {
		"invalid_input" => Ok(ContinuationRejection::InvalidInput),
		"missing_decision" => Ok(ContinuationRejection::MissingDecision),
		"decision_not_selected" => Ok(ContinuationRejection::DecisionNotSelected),
		"stale_managed_run_revision" => Ok(ContinuationRejection::StaleManagedRunRevision),
		"decision_already_consumed" => Ok(ContinuationRejection::DecisionAlreadyConsumed),
		"invalid_context_pack" => Ok(ContinuationRejection::InvalidContextPack),
		"fallback_identity_conflict" => Ok(ContinuationRejection::FallbackIdentityConflict),
		_ => incompatible("stored continuation rejection is unknown"),
	}
}

fn parse_plan(effect: &Value) -> Result<ContinuationPlan, StoreError> {
	require_keys(
		effect,
		&[
			"activity_effects",
			"codex_experiment_id",
			"codex_experiment_revision",
			"codex_observation_id",
			"codex_thread_id",
			"conversation_id",
			"dispatch_enabled",
			"effect_barrier_revision",
			"effect_barrier_state",
			"effect_digest",
			"effect_digest_source",
			"fallback_context_pack_id",
			"fallback_context_pack_revision",
			"fallback_runtime_session_id",
			"kind",
			"managed_run_id",
			"managed_run_revision",
			"operation",
			"operation_id",
			"outbox_effects",
			"plan_id",
			"planned_at_micros",
			"replay_permitted",
			"routing_decision_id",
			"routing_evidence_id",
			"routing_evidence_revision",
			"schema_fingerprint",
			"selected_account_id",
			"source_runtime_session_id",
			"source_runtime_session_revision",
			"submitted_turn_receipt_count",
		],
	)?;
	let kind = match text(effect, "kind")? {
		"same_thread" => ContinuationPlanKind::SameThread,
		"context_pack_fallback" => ContinuationPlanKind::ContextPackFallback,
		_ => return incompatible("stored continuation kind is unknown"),
	};
	let same_thread_evidence = match kind {
		ContinuationPlanKind::SameThread => Some(SameThreadContinuationEvidence {
			routing_evidence_id: optional_text(effect, "routing_evidence_id")?
				.ok_or_else(|| StoreError::Incompatible("same-thread evidence is absent".into()))?
				.to_owned(),
			routing_evidence_revision: optional_positive_i64(effect, "routing_evidence_revision")?
				.ok_or_else(|| StoreError::Incompatible("same-thread evidence revision is absent".into()))?,
			schema_fingerprint: optional_text(effect, "schema_fingerprint")?
				.ok_or_else(|| StoreError::Incompatible("same-thread schema is absent".into()))?
				.to_owned(),
			experiment_id: optional_text(effect, "codex_experiment_id")?
				.ok_or_else(|| StoreError::Incompatible("same-thread experiment is absent".into()))?
				.to_owned(),
			experiment_revision: optional_positive_i64(effect, "codex_experiment_revision")?
				.ok_or_else(|| StoreError::Incompatible("same-thread experiment revision is absent".into()))?,
			observation_id: optional_text(effect, "codex_observation_id")?
				.ok_or_else(|| StoreError::Incompatible("same-thread observation is absent".into()))?
				.to_owned(),
		}),
		ContinuationPlanKind::ContextPackFallback => {
			for key in [
				"routing_evidence_id",
				"schema_fingerprint",
				"codex_experiment_id",
				"codex_observation_id",
			] {
				if optional_text(effect, key)?.is_some() {
					return incompatible("fallback plan carries same-thread evidence");
				}
			}
			if optional_positive_i64(effect, "routing_evidence_revision")?.is_some()
				|| optional_positive_i64(effect, "codex_experiment_revision")?.is_some()
			{
				return incompatible("fallback plan carries same-thread evidence revision");
			}
			None
		},
	};
	let codex_thread_id = optional_text(effect, "codex_thread_id")?.map(str::to_owned);
	let fallback_context_pack_id =
		optional_text(effect, "fallback_context_pack_id")?.map(str::to_owned);
	let fallback_runtime_session_id = optional_text(effect, "fallback_runtime_session_id")?
		.map(|value| RuntimeSessionId::new(value.to_owned()))
		.transpose()
		.map_err(|_| StoreError::Incompatible("fallback RuntimeSession identity is invalid".into()))?;
	match kind {
		ContinuationPlanKind::SameThread
			if codex_thread_id.is_none()
				|| fallback_context_pack_id.is_some()
				|| fallback_runtime_session_id.is_some()
				|| optional_positive_i64(effect, "fallback_context_pack_revision")?.is_some() =>
		{
			return incompatible("same-thread plan has fallback fields");
		},
		ContinuationPlanKind::ContextPackFallback
			if codex_thread_id.is_some()
				|| fallback_context_pack_id.is_none()
				|| fallback_runtime_session_id.is_none()
				|| optional_positive_i64(effect, "fallback_context_pack_revision")?.is_none() =>
		{
			return incompatible("fallback plan is incomplete");
		},
		_ => {},
	}
	if boolean(effect, "replay_permitted")? || boolean(effect, "dispatch_enabled")? {
		return incompatible("continuation plan unexpectedly authorizes execution");
	}
	let submitted = unsigned(effect, "submitted_turn_receipt_count")?;
	if submitted > i64::MAX as u64 {
		return incompatible("submitted-turn receipt count is invalid");
	}
	Ok(ContinuationPlan {
		plan_id: uuid_text(effect, "plan_id")?,
		operation_id: uuid_text(effect, "operation_id")?,
		routing_decision_id: uuid_text(effect, "routing_decision_id")?,
		managed_run_id: ManagedRunId::new(uuid_text(effect, "managed_run_id")?)
			.map_err(|_| StoreError::Incompatible("ManagedRun identity is invalid".into()))?,
		managed_run_revision: positive_i64(effect, "managed_run_revision")?,
		conversation_id: ConversationId::new(uuid_text(effect, "conversation_id")?)
			.map_err(|_| StoreError::Incompatible("Conversation identity is invalid".into()))?,
		source_runtime_session_id: RuntimeSessionId::new(uuid_text(
			effect,
			"source_runtime_session_id",
		)?)
		.map_err(|_| StoreError::Incompatible("source RuntimeSession identity is invalid".into()))?,
		source_runtime_session_revision: positive_i64(effect, "source_runtime_session_revision")?,
		selected_account_id: AccountId::new(uuid_text(effect, "selected_account_id")?)
			.map_err(|_| StoreError::Incompatible("selected account identity is invalid".into()))?,
		kind,
		codex_thread_id,
		fallback_context_pack_id,
		fallback_runtime_session_id,
		same_thread_evidence,
		replay_permitted: false,
		dispatch_enabled: false,
		planned_at_micros: positive_i64(effect, "planned_at_micros")?,
	})
}

fn verify_effect_rows(effect: &Value, plan: &ContinuationPlan) -> Result<(), StoreError> {
	let activity = array(effect, "activity_effects")?;
	let outbox = array(effect, "outbox_effects")?;
	let expected = match plan.kind {
		ContinuationPlanKind::SameThread => 1,
		ContinuationPlanKind::ContextPackFallback => 3,
	};
	if activity.len() != expected || outbox.len() != expected {
		return incompatible("continuation audit/outbox effect count is incomplete");
	}
	let expected_kinds = match plan.kind {
		ContinuationPlanKind::SameThread => {
			vec![("continuation_plan", plan.plan_id.clone(), 1, "continuation_plan_created")]
		},
		ContinuationPlanKind::ContextPackFallback => vec![
			("continuation_plan", plan.plan_id.clone(), 1, "continuation_plan_created"),
			(
				"context_pack",
				plan.fallback_context_pack_id.clone().ok_or_else(|| {
					StoreError::Incompatible("parsed fallback plan lost its Context Pack".into())
				})?,
				positive_i64(effect, "fallback_context_pack_revision")?,
				"context_pack_persisted",
			),
			(
				"runtime_session",
				plan.fallback_runtime_session_id
					.as_ref()
					.ok_or_else(|| {
						StoreError::Incompatible(
							"parsed fallback plan lost its RuntimeSession".into(),
						)
					})?
					.as_str()
					.to_owned(),
				1,
				"runtime_session_created",
			),
		],
	};
	for ((activity, outbox), (kind, id, revision, event_kind)) in
		activity.iter().zip(outbox).zip(expected_kinds)
	{
		require_keys(
			activity,
			&[
				"aggregate_id",
				"aggregate_kind",
				"event_kind",
				"payload",
				"revision",
				"sequence",
			],
		)?;
		require_keys(
			outbox,
			&[
				"aggregate_id",
				"aggregate_kind",
				"aggregate_revision",
				"effect_key",
				"id",
			],
		)?;
		let sequence = positive_i64(activity, "sequence")?;
		let payload = activity
			.get("payload")
			.filter(|value| value.is_object())
			.ok_or_else(|| StoreError::Incompatible("continuation activity payload is malformed".into()))?;
		let expected_effect_key = format!("activity/{sequence}");
		positive_i64(outbox, "id")?;
		if text(activity, "aggregate_kind")? != kind
			|| text(activity, "aggregate_id")? != id
			|| positive_i64(activity, "revision")? != revision
			|| text(activity, "event_kind")? != event_kind
			|| text(payload, "continuation_plan_id")? != plan.plan_id
			|| text(payload, "routing_decision_id")? != plan.routing_decision_id
			|| text(outbox, "aggregate_kind")? != kind
			|| text(outbox, "aggregate_id")? != id
			|| positive_i64(outbox, "aggregate_revision")? != revision
			|| text(outbox, "effect_key")? != expected_effect_key
		{
			return incompatible("continuation audit/outbox effect is cross-linked");
		}
	}
	Ok(())
}

fn verify_digest(effect: &Value) -> Result<(), StoreError> {
	let source = text(effect, "effect_digest_source")?;
	let digest = text(effect, "effect_digest")?;
	if !is_hex_digest(digest) || hex_sha256(source.as_bytes()) != digest {
		return incompatible("stored continuation effect digest is invalid");
	}
	let source_value: Value = serde_json::from_str(source)
		.map_err(|_| StoreError::Incompatible("continuation digest source is malformed".into()))?;
	let mut projection = effect
		.as_object()
		.ok_or_else(|| StoreError::Incompatible("continuation effect is malformed".into()))?
		.clone();
	for key in ["effect_digest", "effect_digest_source", "activity_effects", "outbox_effects"] {
		projection.remove(key);
	}
	if source_value != Value::Object(projection) {
		return incompatible("continuation effect differs from its digest source");
	}
	Ok(())
}

fn require_keys(value: &Value, expected: &[&str]) -> Result<(), StoreError> {
	let object = value
		.as_object()
		.ok_or_else(|| StoreError::Incompatible("stored continuation object is malformed".into()))?;
	let mut actual = object.keys().map(String::as_str).collect::<Vec<_>>();
	actual.sort_unstable();
	let mut expected = expected.to_vec();
	expected.sort_unstable();
	if actual == expected {
		Ok(())
	} else {
		incompatible("stored continuation object has missing or unknown keys")
	}
}

fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, StoreError> {
	value
		.get(key)
		.and_then(Value::as_str)
		.ok_or_else(|| StoreError::Incompatible(format!("stored continuation {key} is malformed")))
}
fn optional_text<'a>(value: &'a Value, key: &str) -> Result<Option<&'a str>, StoreError> {
	match value.get(key) {
		Some(Value::Null) => Ok(None),
		Some(Value::String(value)) => Ok(Some(value)),
		_ => incompatible("stored continuation optional text is malformed"),
	}
}
fn positive_i64(value: &Value, key: &str) -> Result<i64, StoreError> {
	value
		.get(key)
		.and_then(Value::as_i64)
		.filter(|number| *number > 0)
		.ok_or_else(|| StoreError::Incompatible(format!("stored continuation {key} is malformed")))
}
fn optional_positive_i64(value: &Value, key: &str) -> Result<Option<i64>, StoreError> {
	match value.get(key) {
		Some(Value::Null) => Ok(None),
		Some(value) => value
			.as_i64()
			.filter(|number| *number > 0)
			.map(Some)
			.ok_or_else(|| StoreError::Incompatible(format!("stored continuation {key} is malformed"))),
		None => incompatible("stored continuation optional revision is missing"),
	}
}
fn unsigned(value: &Value, key: &str) -> Result<u64, StoreError> {
	value
		.get(key)
		.and_then(Value::as_u64)
		.ok_or_else(|| StoreError::Incompatible(format!("stored continuation {key} is malformed")))
}
fn boolean(value: &Value, key: &str) -> Result<bool, StoreError> {
	value
		.get(key)
		.and_then(Value::as_bool)
		.ok_or_else(|| StoreError::Incompatible(format!("stored continuation {key} is malformed")))
}
fn array<'a>(value: &'a Value, key: &str) -> Result<&'a [Value], StoreError> {
	value
		.get(key)
		.and_then(Value::as_array)
		.map(Vec::as_slice)
		.ok_or_else(|| StoreError::Incompatible(format!("stored continuation {key} is malformed")))
}
fn uuid_text(value: &Value, key: &str) -> Result<String, StoreError> {
	let value = text(value, key)?;
	if is_uuid(value) {
		Ok(value.to_owned())
	} else {
		incompatible("stored continuation UUID is malformed")
	}
}
fn validate_uuid(value: &str, label: &'static str) -> Result<(), StoreError> {
	if is_uuid(value) {
		Ok(())
	} else {
		Err(StoreError::InvalidInput(label))
	}
}
fn is_uuid(value: &str) -> bool {
	value.len() == 36
		&& value.bytes().enumerate().all(|(index, byte)| match index {
			8 | 13 | 18 | 23 => byte == b'-',
			_ => byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte),
		})
}
fn is_hex_digest(value: &str) -> bool {
	value.len() == 64
		&& value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
fn hex_sha256(bytes: &[u8]) -> String {
	Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}
const fn plan_kind_sql(kind: ContinuationPlanKind) -> &'static str {
	match kind {
		ContinuationPlanKind::SameThread => "same_thread",
		ContinuationPlanKind::ContextPackFallback => "context_pack_fallback",
	}
}
fn incompatible<T>(message: &str) -> Result<T, StoreError> {
	Err(StoreError::Incompatible(message.into()))
}
