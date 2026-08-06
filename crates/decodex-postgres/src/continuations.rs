//! Inert, exactly-once continuation plans over persisted Routing Decisions.

use decodex_core::{
	AccountId, BlobStore, ContextPack, ContinuationCommandOutcome, ContinuationPlan,
	ContinuationPlanKind, ContinuationRejection, ConversationId, ExecutionConsumer,
	MAX_INLINE_HISTORY_BYTES, ManagedExecutionId, ManagedRunId, ProviderAttemptId,
	ProviderEvidenceId, RuntimeSessionId, SameThreadContinuationEvidence, TurnId,
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
	runtime_sessions::{
		RuntimeSessionAccountSnapshot, RuntimeSessionProfileSnapshot, StoredRuntimeSession,
		account_from_value, profile_from_value, session_state_from_sql,
	},
};

const PLAN_INITIAL_THREAD_CONTINUATION_SQL: &str = "SELECT decodex.plan_initial_thread_continuation_exact(\
	 $1,$2,$3::text::uuid,$4::text::uuid,$5,$6::text::uuid)";
const PLAN_EXISTING_SESSION_CONTINUATION_SQL: &str = "SELECT decodex.plan_continuation_exact(\
	 $1,$2,$3::text::uuid,$4::text::uuid,$5,$6::text::uuid,$7::text::uuid,\
	 $8::text::uuid,$9::text::uuid,$10,$11,$12,$13,$14,$15,$16,$17,\
	 $18::text[],$19::text[],$20::bigint[],$21::text[],$22::bigint[],\
	 $23::bigint[],$24::text[],$25::text[],$26::text[],$27::bigint[])";

#[cfg(all(test, feature = "test-support"))]
pub(crate) async fn prepare_continuation_plan_sql(
	client: &tokio_postgres::Client,
) -> Result<usize, StoreError> {
	const SOURCES: [&str; 2] =
		[PLAN_INITIAL_THREAD_CONTINUATION_SQL, PLAN_EXISTING_SESSION_CONTINUATION_SQL];
	for source in SOURCES {
		client.prepare(source).await?;
	}
	Ok(SOURCES.len())
}

/// Exact coordinates for existing-session continuation planning.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlanContinuation {
	/// Domain operation identity; the protocol-scoped exact-command idempotency key is separate.
	pub operation_id: String,
	/// Exact persisted selected Routing Decision to consume.
	pub routing_decision_id: String,
	/// Positive Conversation or ManagedRun revision that must match persisted decision lineage.
	pub expected_consumer_revision: i64,
	/// Caller-allocated identity for the one immutable continuation plan.
	pub plan_id: String,
	/// Preallocated RuntimeSession identity used only when fallback is selected.
	pub fallback_runtime_session_id: String,
	/// Preallocated selected-account snapshot identity used only for fallback.
	pub fallback_account_snapshot_id: String,
	/// Preallocated Context Pack identity used only for fallback.
	pub fallback_context_pack_id: String,
}

/// Exact coordinates for creating the first unfenced RuntimeSession and inert plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlanInitialThreadContinuation {
	/// Domain operation identity; the protocol-scoped exact-command key is separate.
	pub operation_id: String,
	/// Exact persisted selected Routing Decision to consume.
	pub routing_decision_id: String,
	/// Positive Conversation revision that must match the Routing Decision consumer lineage.
	pub expected_conversation_revision: i64,
	/// Caller-allocated identity for the immutable initial-thread plan.
	pub plan_id: String,
}

/// Strict committed continuation-plan readback.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContinuationPlanEffect {
	/// Exact inert continuation plan parsed from the committed Continuation Plan effect.
	pub plan: ContinuationPlan,
	/// First RuntimeSession and copied snapshots, present only for an initial-thread plan.
	pub runtime_session: Option<StoredRuntimeSession>,
	/// Fully verified Context Pack, present only for a Context-Pack fallback plan.
	pub fallback_context_pack: Option<ContextPackRecord>,
}

struct ContinuationPackParameters {
	compiled_bytes: Vec<u8>,
	compiled_digest: String,
	manifest_digest: String,
	max_bytes: i32,
	recent_item_limit: i32,
	possible_side_effects: &'static str,
	truncated: bool,
	omitted_source_count: i32,
	source_kinds: Vec<String>,
	source_ids: Vec<String>,
	source_revisions: Vec<i64>,
	content_digests: Vec<String>,
	original_lengths: Vec<i64>,
	included_lengths: Vec<i64>,
	included_digests: Vec<String>,
	dispositions: Vec<String>,
	artifact_ids: Vec<String>,
	artifact_revisions: Vec<i64>,
}

impl ContinuationPackParameters {
	fn new(fallback_pack: &ContextPack) -> Self {
		let sources = fallback_pack.source_manifest();
		Self {
			compiled_bytes: fallback_pack.bytes().to_vec(),
			compiled_digest: fallback_pack.digest().to_hex(),
			manifest_digest: fallback_pack.manifest_digest().to_hex(),
			max_bytes: i32::try_from(fallback_pack.policy().max_bytes()).unwrap_or(i32::MAX),
			recent_item_limit: i32::try_from(fallback_pack.policy().recent_item_limit())
				.unwrap_or(i32::MAX),
			possible_side_effects: side_effect_sql(fallback_pack.possible_side_effects()),
			truncated: fallback_pack.truncated(),
			omitted_source_count: i32::try_from(fallback_pack.omitted_source_count())
				.unwrap_or(i32::MAX),
			source_kinds: sources
				.iter()
				.map(|source| context_source_sql(source.kind()).to_owned())
				.collect(),
			source_ids: sources.iter().map(|source| source.source_id().to_owned()).collect(),
			source_revisions: sources
				.iter()
				.map(|source| i64::try_from(source.revision()).unwrap_or(i64::MAX))
				.collect(),
			content_digests: sources
				.iter()
				.map(|source| source.content_digest().to_hex())
				.collect(),
			original_lengths: sources
				.iter()
				.map(|source| i64::try_from(source.original_byte_length()).unwrap_or(i64::MAX))
				.collect(),
			included_lengths: sources
				.iter()
				.map(|source| i64::try_from(source.included_byte_length()).unwrap_or(i64::MAX))
				.collect(),
			included_digests: sources
				.iter()
				.map(|source| source.included_digest().to_hex())
				.collect(),
			dispositions: sources
				.iter()
				.map(|source| context_disposition_sql(source.disposition()).to_owned())
				.collect(),
			artifact_ids: sources
				.iter()
				.map(|source| {
					source
						.artifact_reference()
						.map_or_else(String::new, |(id, _)| id.as_str().to_owned())
				})
				.collect(),
			artifact_revisions: sources
				.iter()
				.map(|source| {
					source
						.artifact_reference()
						.and_then(|(_, revision)| i64::try_from(revision).ok())
						.unwrap_or(0)
				})
				.collect(),
		}
	}
}

impl PostgresStore {
	/// Consume one selected L0 decision and atomically create the first RuntimeSession and plan.
	pub async fn plan_initial_thread_continuation(
		&self,
		idempotency_key: &str,
		request: &PlanInitialThreadContinuation,
	) -> Result<ContinuationCommandOutcome<ContinuationPlanEffect>, StoreError> {
		validate_exact_key(idempotency_key)?;
		for (value, label) in [
			(&request.operation_id, "continuation operation identity"),
			(&request.routing_decision_id, "routing decision identity"),
			(&request.plan_id, "continuation plan identity"),
		] {
			validate_uuid(value, label)?;
		}
		if request.expected_conversation_revision <= 0 {
			return Err(StoreError::InvalidInput("Conversation revision must be positive"));
		}

		let response = self
			.execute_exact_with_retry(
				PLAN_INITIAL_THREAD_CONTINUATION_SQL,
				&[
					&EXACT_COMMAND_PROTOCOL,
					&idempotency_key,
					&request.operation_id,
					&request.routing_decision_id,
					&request.expected_conversation_revision,
					&request.plan_id,
				],
			)
			.await?;
		let (classification, effect) =
			parse_envelope(&response, "plan_initial_thread_continuation")?;
		if classification == "stable_domain_rejection" {
			return Ok(ContinuationCommandOutcome::Rejected(parse_rejection(&effect)?));
		}
		let plan = parse_plan(&effect)?;
		if plan.kind != ContinuationPlanKind::InitialThread
			|| plan.plan_id != request.plan_id
			|| plan.operation_id != request.operation_id
			|| plan.routing_decision_id != request.routing_decision_id
			|| plan.consumer.domain_revision() != request.expected_conversation_revision
		{
			return incompatible("stored initial-thread continuation is cross-linked");
		}
		if plan.source_runtime_session_revision != 1 {
			return incompatible("initial RuntimeSession revision is not fresh");
		}
		let runtime_session = Some(parse_created_runtime_session(
			&effect,
			&plan,
			&plan.source_runtime_session_id,
			"initial",
		)?);
		verify_effect_rows(&effect, &plan)?;
		self.verify_plan_readback(&response, &effect, &plan).await?;
		Ok(ContinuationCommandOutcome::Success(ContinuationPlanEffect {
			plan,
			runtime_session,
			fallback_context_pack: None,
		}))
	}

	/// Atomically choose same-thread continuation or one Context-Pack fallback successor.
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
		if request.expected_consumer_revision <= 0 {
			return Err(StoreError::InvalidInput("execution consumer revision must be positive"));
		}
		validate_context_pack(
			&crate::PersistContextPack {
				context_pack_id: request.fallback_context_pack_id.clone(),
				pack_revision: 1,
			},
			fallback_pack,
		)?;

		let blob = (fallback_pack.bytes().len() > MAX_INLINE_HISTORY_BYTES).then(|| {
			(fallback_pack.digest(), i64::try_from(fallback_pack.bytes().len()).unwrap_or(i64::MAX))
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
		let parameters = ContinuationPackParameters::new(fallback_pack);

		let response =
			self.execute_existing_session_plan(idempotency_key, request, &parameters).await?;
		let (classification, effect) = parse_envelope(&response, "plan_continuation")?;
		if classification == "stable_domain_rejection" {
			return Ok(ContinuationCommandOutcome::Rejected(parse_rejection(&effect)?));
		}
		let plan = parse_plan(&effect)?;
		if plan.plan_id != request.plan_id
			|| plan.operation_id != request.operation_id
			|| plan.routing_decision_id != request.routing_decision_id
			|| plan.consumer.domain_revision() != request.expected_consumer_revision
		{
			return incompatible("stored existing-session continuation is cross-linked");
		}
		verify_effect_rows(&effect, &plan)?;
		self.verify_plan_readback(&response, &effect, &plan).await?;
		let (runtime_session, fallback_context_pack) = match plan.kind {
			ContinuationPlanKind::SameThread => {
				if !matches!(effect.get("runtime_session_snapshot"), Some(Value::Null))
					|| !matches!(effect.get("profile_snapshot"), Some(Value::Null))
					|| !matches!(effect.get("account_snapshot"), Some(Value::Null))
				{
					return incompatible("same-thread continuation created successor state");
				}
				(None, None)
			},
			ContinuationPlanKind::ContextPackFallback => {
				let context_pack_id =
					plan.fallback_context_pack_id.as_deref().ok_or_else(|| {
						StoreError::Incompatible("fallback plan lost Context Pack".into())
					})?;
				let record = self.required_context_pack(blob_store, context_pack_id).await?;
				if record.compiled_digest != fallback_pack.digest()
					|| record.conversation_id != plan.conversation_id
					|| record.pack_revision
						!= positive_i64(&effect, "fallback_context_pack_revision")?
				{
					return incompatible("fallback Context Pack readback is cross-linked");
				}
				let runtime_session_id =
					plan.fallback_runtime_session_id.as_ref().ok_or_else(|| {
						StoreError::Incompatible("fallback plan lost RuntimeSession".into())
					})?;
				(
					Some(parse_created_runtime_session(
						&effect,
						&plan,
						runtime_session_id,
						"fallback",
					)?),
					Some(record),
				)
			},
			ContinuationPlanKind::InitialThread => {
				return incompatible("existing-session continuation returned initial-thread state");
			},
		};
		Ok(ContinuationCommandOutcome::Success(ContinuationPlanEffect {
			plan,
			runtime_session,
			fallback_context_pack,
		}))
	}

	async fn execute_existing_session_plan(
		&self,
		idempotency_key: &str,
		request: &PlanContinuation,
		parameters: &ContinuationPackParameters,
	) -> Result<Vec<u8>, StoreError> {
		self.execute_exact_with_retry(
			PLAN_EXISTING_SESSION_CONTINUATION_SQL,
			&[
				&EXACT_COMMAND_PROTOCOL,
				&idempotency_key,
				&request.operation_id,
				&request.routing_decision_id,
				&request.expected_consumer_revision,
				&request.plan_id,
				&request.fallback_runtime_session_id,
				&request.fallback_account_snapshot_id,
				&request.fallback_context_pack_id,
				&parameters.compiled_bytes,
				&parameters.compiled_digest,
				&parameters.manifest_digest,
				&parameters.max_bytes,
				&parameters.recent_item_limit,
				&parameters.possible_side_effects,
				&parameters.truncated,
				&parameters.omitted_source_count,
				&parameters.source_kinds,
				&parameters.source_ids,
				&parameters.source_revisions,
				&parameters.content_digests,
				&parameters.original_lengths,
				&parameters.included_lengths,
				&parameters.included_digests,
				&parameters.dispositions,
				&parameters.artifact_ids,
				&parameters.artifact_revisions,
			],
		)
		.await
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

fn parse_envelope(bytes: &[u8], expected_operation: &str) -> Result<(String, Value), StoreError> {
	let document: Value = serde_json::from_slice(bytes).map_err(|_| {
		StoreError::Incompatible("stored continuation response is malformed".into())
	})?;
	require_keys(&document, &["classification", "effect"])?;
	let classification = text(&document, "classification")?;
	if !matches!(classification, "success" | "stable_domain_rejection") {
		return incompatible("stored continuation response classification is unknown");
	}
	let effect = document.get("effect").filter(|value| value.is_object()).ok_or_else(|| {
		StoreError::Incompatible("stored continuation effect is malformed".into())
	})?;
	verify_digest(effect)?;
	if text(effect, "operation")? != expected_operation {
		return incompatible("stored continuation operation is cross-linked");
	}
	Ok((classification.to_owned(), effect.clone()))
}

fn parse_rejection(effect: &Value) -> Result<ContinuationRejection, StoreError> {
	require_keys(effect, &["effect_digest", "effect_digest_source", "operation", "rejection"])?;
	match text(effect, "rejection")? {
		"invalid_input" => Ok(ContinuationRejection::InvalidInput),
		"missing_decision" => Ok(ContinuationRejection::MissingDecision),
		"decision_not_selected" => Ok(ContinuationRejection::DecisionNotSelected),
		"stale_consumer_revision" => Ok(ContinuationRejection::StaleConsumerRevision),
		"decision_already_consumed" => Ok(ContinuationRejection::DecisionAlreadyConsumed),
		"same_thread_unavailable" => Ok(ContinuationRejection::SameThreadUnavailable),
		"selected_account_drift" => Ok(ContinuationRejection::SelectedAccountDrift),
		"selected_account_readiness_required" =>
			Ok(ContinuationRejection::SelectedAccountReadinessRequired),
		"selected_account_quota_required" =>
			Ok(ContinuationRejection::SelectedAccountQuotaRequired),
		"invalid_context_pack" => Ok(ContinuationRejection::InvalidContextPack),
		"fallback_identity_conflict" => Ok(ContinuationRejection::FallbackIdentityConflict),
		_ => incompatible("stored continuation rejection is unknown"),
	}
}

fn parse_same_thread_evidence(
	effect: &Value,
	kind: ContinuationPlanKind,
	consumer: &ExecutionConsumer,
) -> Result<Option<SameThreadContinuationEvidence>, StoreError> {
	match kind {
		ContinuationPlanKind::InitialThread => {
			if !matches!(consumer, ExecutionConsumer::ConversationTurn { .. }) {
				return incompatible("initial-thread plan has a non-Conversation consumer");
			}
			for key in [
				"routing_evidence_id",
				"schema_fingerprint",
				"codex_experiment_id",
				"codex_observation_id",
				"same_thread_provider_attempt_id",
				"same_thread_provider_evidence_id",
			] {
				if optional_text(effect, key)?.is_some() {
					return incompatible("initial-thread plan carries continuation evidence");
				}
			}
			if optional_positive_i64(effect, "routing_evidence_revision")?.is_some()
				|| optional_positive_i64(effect, "codex_experiment_revision")?.is_some()
				|| optional_positive_i64(effect, "same_thread_provider_attempt_revision")?.is_some()
			{
				return incompatible("initial-thread plan carries an evidence revision");
			}
			Ok(None)
		},
		ContinuationPlanKind::SameThread =>
			parse_selected_same_thread_evidence(effect, consumer).map(Some),
		ContinuationPlanKind::ContextPackFallback => {
			for key in [
				"routing_evidence_id",
				"schema_fingerprint",
				"codex_experiment_id",
				"codex_observation_id",
				"same_thread_provider_attempt_id",
				"same_thread_provider_evidence_id",
			] {
				if optional_text(effect, key)?.is_some() {
					return incompatible("fallback plan carries same-thread evidence");
				}
			}
			if optional_positive_i64(effect, "routing_evidence_revision")?.is_some()
				|| optional_positive_i64(effect, "codex_experiment_revision")?.is_some()
				|| optional_positive_i64(effect, "same_thread_provider_attempt_revision")?.is_some()
			{
				return incompatible("fallback plan carries same-thread evidence revision");
			}
			Ok(None)
		},
	}
}

fn parse_selected_same_thread_evidence(
	effect: &Value,
	consumer: &ExecutionConsumer,
) -> Result<SameThreadContinuationEvidence, StoreError> {
	match consumer {
		ExecutionConsumer::ManagedRunExecution { .. } =>
			Ok(SameThreadContinuationEvidence::CausalExperiment {
				routing_evidence_id: optional_text(effect, "routing_evidence_id")?
					.ok_or_else(|| {
						StoreError::Incompatible("same-thread routing evidence is absent".into())
					})?
					.to_owned(),
				routing_evidence_revision: optional_positive_i64(
					effect,
					"routing_evidence_revision",
				)?
				.ok_or_else(|| {
					StoreError::Incompatible(
						"same-thread routing evidence revision is absent".into(),
					)
				})?,
				schema_fingerprint: optional_text(effect, "schema_fingerprint")?
					.ok_or_else(|| StoreError::Incompatible("same-thread schema is absent".into()))?
					.to_owned(),
				experiment_id: optional_text(effect, "codex_experiment_id")?
					.ok_or_else(|| {
						StoreError::Incompatible("same-thread experiment is absent".into())
					})?
					.to_owned(),
				experiment_revision: optional_positive_i64(effect, "codex_experiment_revision")?
					.ok_or_else(|| {
						StoreError::Incompatible("same-thread experiment revision is absent".into())
					})?,
				observation_id: optional_text(effect, "codex_observation_id")?
					.ok_or_else(|| {
						StoreError::Incompatible("same-thread observation is absent".into())
					})?
					.to_owned(),
			}),
		ExecutionConsumer::ConversationTurn { .. } =>
			Ok(SameThreadContinuationEvidence::ProviderAttempt {
				attempt_id: ProviderAttemptId::new(
					optional_text(effect, "same_thread_provider_attempt_id")?
						.ok_or_else(|| {
							StoreError::Incompatible("same-thread ProviderAttempt is absent".into())
						})?
						.to_owned(),
				)
				.map_err(|_| {
					StoreError::Incompatible(
						"same-thread ProviderAttempt identity is invalid".into(),
					)
				})?,
				attempt_revision: optional_positive_i64(
					effect,
					"same_thread_provider_attempt_revision",
				)?
				.ok_or_else(|| {
					StoreError::Incompatible(
						"same-thread ProviderAttempt revision is absent".into(),
					)
				})?,
				evidence_id: ProviderEvidenceId::new(
					optional_text(effect, "same_thread_provider_evidence_id")?
						.ok_or_else(|| {
							StoreError::Incompatible(
								"same-thread provider evidence is absent".into(),
							)
						})?
						.to_owned(),
				)
				.map_err(|_| {
					StoreError::Incompatible(
						"same-thread provider evidence identity is invalid".into(),
					)
				})?,
			}),
	}
}

fn parse_consumer(
	effect: &Value,
	kind: ContinuationPlanKind,
) -> Result<ExecutionConsumer, StoreError> {
	match text(effect, "consumer_kind")? {
		"conversation_turn" => {
			let (source_runtime_session_id, source_runtime_session_revision) = match kind {
				ContinuationPlanKind::InitialThread => (None, None),
				ContinuationPlanKind::SameThread | ContinuationPlanKind::ContextPackFallback => (
					Some(
						RuntimeSessionId::new(uuid_text(effect, "source_runtime_session_id")?)
							.map_err(|_| {
								StoreError::Incompatible(
									"source RuntimeSession identity is invalid".into(),
								)
							})?,
					),
					Some(positive_i64(effect, "source_runtime_session_revision")?),
				),
			};
			Ok(ExecutionConsumer::ConversationTurn {
				conversation_id: ConversationId::new(uuid_text(
					effect,
					"consumer_conversation_id",
				)?)
				.map_err(|_| {
					StoreError::Incompatible("consumer Conversation identity is invalid".into())
				})?,
				conversation_revision: positive_i64(effect, "conversation_revision")?,
				source_runtime_session_id,
				source_runtime_session_revision,
				turn_id: TurnId::new(uuid_text(effect, "turn_id")?).map_err(|_| {
					StoreError::Incompatible("consumer Turn identity is invalid".into())
				})?,
			})
		},
		"managed_run_execution" => Ok(ExecutionConsumer::ManagedRunExecution {
			managed_run_id: ManagedRunId::new(uuid_text(effect, "managed_run_id")?)
				.map_err(|_| StoreError::Incompatible("ManagedRun identity is invalid".into()))?,
			managed_run_revision: positive_i64(effect, "managed_run_revision")?,
			execution_id: ManagedExecutionId::new(uuid_text(effect, "managed_execution_id")?)
				.map_err(|_| {
					StoreError::Incompatible("ManagedRun execution identity is invalid".into())
				})?,
		}),
		_ => incompatible("stored execution consumer kind is unknown"),
	}
}

const CONTINUATION_PLAN_EFFECT_KEYS: &[&str] = &[
	"activity_effects",
	"account_snapshot",
	"codex_experiment_id",
	"codex_experiment_revision",
	"codex_observation_id",
	"codex_thread_id",
	"conversation_id",
	"consumer_conversation_id",
	"consumer_kind",
	"conversation_revision",
	"dispatch_enabled",
	"effect_digest",
	"effect_digest_source",
	"fallback_context_pack_id",
	"fallback_context_pack_revision",
	"fallback_runtime_session_id",
	"kind",
	"managed_run_id",
	"managed_run_revision",
	"managed_execution_id",
	"operation",
	"operation_id",
	"outbox_effects",
	"plan_id",
	"planned_at_micros",
	"profile_snapshot",
	"replay_permitted",
	"routing_decision_id",
	"routing_evidence_id",
	"routing_evidence_revision",
	"runtime_session_snapshot",
	"schema_fingerprint",
	"same_thread_provider_attempt_id",
	"same_thread_provider_attempt_revision",
	"same_thread_provider_evidence_id",
	"selected_account_id",
	"source_runtime_session_id",
	"source_runtime_session_revision",
	"turn_id",
];

fn parse_plan(effect: &Value) -> Result<ContinuationPlan, StoreError> {
	require_keys(effect, CONTINUATION_PLAN_EFFECT_KEYS)?;
	let kind = match text(effect, "kind")? {
		"initial_thread" => ContinuationPlanKind::InitialThread,
		"same_thread" => ContinuationPlanKind::SameThread,
		"context_pack_fallback" => ContinuationPlanKind::ContextPackFallback,
		_ => return incompatible("stored continuation kind is not available to runtime callers"),
	};
	let consumer = parse_consumer(effect, kind)?;
	let same_thread_evidence = parse_same_thread_evidence(effect, kind, &consumer)?;
	let codex_thread_id = optional_text(effect, "codex_thread_id")?.map(str::to_owned);
	let fallback_context_pack_id =
		optional_text(effect, "fallback_context_pack_id")?.map(str::to_owned);
	let fallback_runtime_session_id = optional_text(effect, "fallback_runtime_session_id")?
		.map(|value| RuntimeSessionId::new(value.to_owned()))
		.transpose()
		.map_err(|_| {
			StoreError::Incompatible("fallback RuntimeSession identity is invalid".into())
		})?;
	let fallback_context_pack_revision =
		optional_positive_i64(effect, "fallback_context_pack_revision")?;
	match kind {
		ContinuationPlanKind::InitialThread
			if codex_thread_id.is_some()
				|| fallback_context_pack_id.is_some()
				|| fallback_runtime_session_id.is_some()
				|| fallback_context_pack_revision.is_some() =>
		{
			return incompatible("initial-thread plan has thread or successor fields");
		},
		ContinuationPlanKind::SameThread
			if codex_thread_id.is_none()
				|| fallback_context_pack_id.is_some()
				|| fallback_runtime_session_id.is_some()
				|| fallback_context_pack_revision.is_some() =>
		{
			return incompatible("same-thread plan has successor fields");
		},
		ContinuationPlanKind::ContextPackFallback
			if codex_thread_id.is_some()
				|| fallback_context_pack_id.is_none()
				|| fallback_runtime_session_id.is_none()
				|| fallback_context_pack_revision.is_none() =>
		{
			return incompatible("fallback plan is incomplete");
		},
		_ => {},
	}
	if boolean(effect, "replay_permitted")? || boolean(effect, "dispatch_enabled")? {
		return incompatible("continuation plan unexpectedly authorizes execution");
	}
	let conversation_id = ConversationId::new(uuid_text(effect, "conversation_id")?)
		.map_err(|_| StoreError::Incompatible("Conversation identity is invalid".into()))?;
	if matches!(
		&consumer,
		ExecutionConsumer::ConversationTurn {
			conversation_id: consumer_conversation_id,
			..
		} if *consumer_conversation_id != conversation_id
	) {
		return incompatible("ordinary continuation consumer is cross-linked");
	}
	Ok(ContinuationPlan {
		plan_id: uuid_text(effect, "plan_id")?,
		operation_id: uuid_text(effect, "operation_id")?,
		routing_decision_id: uuid_text(effect, "routing_decision_id")?,
		consumer,
		conversation_id,
		source_runtime_session_id: RuntimeSessionId::new(uuid_text(
			effect,
			"source_runtime_session_id",
		)?)
		.map_err(|_| {
			StoreError::Incompatible("source RuntimeSession identity is invalid".into())
		})?,
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

fn parse_created_runtime_session(
	effect: &Value,
	plan: &ContinuationPlan,
	expected_runtime_session_id: &RuntimeSessionId,
	shape: &str,
) -> Result<StoredRuntimeSession, StoreError> {
	let session = effect
		.get("runtime_session_snapshot")
		.filter(|value| value.is_object())
		.ok_or_else(|| StoreError::Incompatible(format!("{shape} RuntimeSession is absent")))?;
	let profile_value = effect
		.get("profile_snapshot")
		.filter(|value| value.is_object())
		.ok_or_else(|| StoreError::Incompatible(format!("{shape} profile snapshot is absent")))?;
	let account_value = effect
		.get("account_snapshot")
		.filter(|value| value.is_object())
		.ok_or_else(|| StoreError::Incompatible(format!("{shape} account snapshot is absent")))?;
	require_keys(
		session,
		&[
			"account_snapshot_id",
			"codex_thread_id",
			"conversation_id",
			"created_at",
			"ended_at",
			"last_known_turn_id",
			"profile_snapshot_id",
			"revision",
			"runtime_session_id",
			"state",
			"updated_at",
		],
	)?;
	require_keys(
		profile_value,
		&[
			"created_at",
			"instructions",
			"instructions_digest",
			"model",
			"profile_snapshot_id",
			"provenance",
			"reasoning_effort",
			"role",
			"service_tier",
			"source_profile_id",
			"source_revision",
		],
	)?;
	require_keys(
		account_value,
		&[
			"account_snapshot_id",
			"created_at",
			"display_label",
			"observed_state",
			"source_account_id",
			"source_revision",
		],
	)?;
	let profile: RuntimeSessionProfileSnapshot = profile_from_value(profile_value)?;
	let account: RuntimeSessionAccountSnapshot = account_from_value(account_value)?;
	let runtime_session_id = RuntimeSessionId::new(uuid_text(session, "runtime_session_id")?)
		.map_err(|_| {
			StoreError::Incompatible("initial RuntimeSession identity is invalid".into())
		})?;
	let conversation_id = ConversationId::new(uuid_text(session, "conversation_id")?)
		.map_err(|_| StoreError::Incompatible("initial Conversation identity is invalid".into()))?;
	let state = session_state_from_sql(text(session, "state")?)?;
	let revision = positive_i64(session, "revision")?;
	if runtime_session_id != *expected_runtime_session_id
		|| conversation_id != plan.conversation_id
		|| revision != 1
		|| state != decodex_core::RuntimeSessionState::Starting
		|| optional_text(session, "codex_thread_id")?.is_some()
		|| optional_text(session, "last_known_turn_id")?.is_some()
		|| optional_text(session, "ended_at")?.is_some()
		|| text(session, "profile_snapshot_id")? != profile.profile_snapshot_id
		|| text(session, "account_snapshot_id")? != account.account_snapshot_id
		|| account.source_account_id != plan.selected_account_id
	{
		return incompatible("created RuntimeSession readback is cross-linked");
	}
	Ok(StoredRuntimeSession {
		runtime_session_id,
		conversation_id,
		profile_snapshot: profile,
		account_snapshot: account,
		codex_thread_id: None,
		last_known_turn_id: None,
		state,
		revision,
		created_at: text(session, "created_at")?.to_owned(),
		updated_at: text(session, "updated_at")?.to_owned(),
		ended_at: None,
	})
}

fn verify_effect_rows(effect: &Value, plan: &ContinuationPlan) -> Result<(), StoreError> {
	let activity = array(effect, "activity_effects")?;
	let outbox = array(effect, "outbox_effects")?;
	let expected = match plan.kind {
		ContinuationPlanKind::InitialThread | ContinuationPlanKind::SameThread => 1,
		ContinuationPlanKind::ContextPackFallback => 3,
	};
	if activity.len() != expected || outbox.len() != expected {
		return incompatible("continuation audit/outbox effect count is incomplete");
	}
	let expected_kinds = match plan.kind {
		ContinuationPlanKind::InitialThread | ContinuationPlanKind::SameThread =>
			vec![("continuation_plan", plan.plan_id.clone(), 1, "continuation_plan_created")],
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
			&["aggregate_id", "aggregate_kind", "event_kind", "payload", "revision", "sequence"],
		)?;
		require_keys(
			outbox,
			&["aggregate_id", "aggregate_kind", "aggregate_revision", "effect_key", "id"],
		)?;
		let sequence = positive_i64(activity, "sequence")?;
		let payload =
			activity.get("payload").filter(|value| value.is_object()).ok_or_else(|| {
				StoreError::Incompatible("continuation activity payload is malformed".into())
			})?;
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
	let object = value.as_object().ok_or_else(|| {
		StoreError::Incompatible("stored continuation object is malformed".into())
	})?;
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
		Some(value) => value.as_i64().filter(|number| *number > 0).map(Some).ok_or_else(|| {
			StoreError::Incompatible(format!("stored continuation {key} is malformed"))
		}),
		None => incompatible("stored continuation optional revision is missing"),
	}
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
	if is_uuid(value) { Ok(()) } else { Err(StoreError::InvalidInput(label)) }
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
		ContinuationPlanKind::InitialThread => "initial_thread",
		ContinuationPlanKind::SameThread => "same_thread",
		ContinuationPlanKind::ContextPackFallback => "context_pack_fallback",
	}
}
fn incompatible<T>(message: &str) -> Result<T, StoreError> {
	Err(StoreError::Incompatible(message.into()))
}
