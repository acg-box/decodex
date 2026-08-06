use serde_json::Value;

use crate::{
	PostgresStore, RoleProfileRole, StoreError,
	exact_commands::{EXACT_COMMAND_PROTOCOL, validate_exact_effect_digest, validate_exact_key},
};
use decodex_core::{
	AccountId, AccountState, ConversationId, ProcessExecutionEpochId, ProcessGenerationId,
	ProviderAttemptId, ProviderEvidenceId, ProviderTerminalOutcome, RuntimeSessionId,
	RuntimeSessionState, TurnId,
};

const FENCE_RUNTIME_SESSION_THREAD_START_SQL: &str = "SELECT response_bytes,replayed FROM \
	 decodex.fence_runtime_session_thread_start_exact(\
	 $1,$2,$3::text::uuid,$4,$5::text::uuid,$6,$7::text::uuid,$8,\
	 $9::text::uuid,$10::text::uuid,$11,$12::text::uuid,$13,$14)";
const BIND_RUNTIME_SESSION_THREAD_SQL: &str = "SELECT response_bytes,replayed FROM \
	 decodex.bind_runtime_session_thread_exact(\
	 $1,$2,$3::text::uuid,$4,$5::text::uuid,$6,$7::text::uuid,$8,\
	 $9::text::uuid,$10,$11,$12,$13,$14,$15,$16::text::uuid)";
const ACKNOWLEDGE_RUNTIME_SESSION_TURN_SQL: &str = "SELECT response_bytes,replayed FROM \
	 decodex.acknowledge_runtime_session_turn_exact(\
	 $1,$2,$3::text::uuid,$4,$5::text::uuid,$6,$7::text::uuid,$8,\
	 $9::text::uuid,$10,$11::text::uuid,$12,$13::text::uuid,\
	 $14::text::decodex.provider_attempt_terminal_outcome,\
	 $15::text::uuid,$16)";
const READ_ORDINARY_RUNTIME_SESSION_FOR_RESUME_SQL: &str = "SELECT conversation_revision,runtime_session_id::text,\
	 runtime_session_revision,codex_thread_id::text,model,reasoning_effort,\
	 instructions,source_account_id::text,source_account_revision,\
	 next_turn_sequence,thread_start_request_id,thread_start_request_sha256,\
	 thread_start_response_id,thread_start_response_sha256,has_acknowledged_turn,\
	 has_active_turn,\
	 has_unresolved_provider_attempt,conversation_status,profile_role \
	 FROM decodex.read_ordinary_runtime_session_for_resume_exact($1::text::uuid)";
const PREPARE_QUICK_TASK_PROCESS_GENERATION_SQL: &str = "SELECT response_bytes,replayed FROM \
	 decodex.prepare_quick_task_process_generation_exact(\
	 $1,$2,$3::text::uuid,$4,$5::text::uuid,$6,$7::text::uuid,$8,\
	 $9::text::uuid,$10::text::uuid,$11::text::uuid,$12::text::uuid)";
const READ_QUICK_TASK_THREAD_ESTABLISHMENT_SQL: &str = "SELECT \
	 decodex.read_quick_task_thread_establishment_exact(\
	 $1::text::uuid,$2,$3::text::uuid,$4,$5::text::uuid,$6,\
	 $7::text::uuid,$8::text::uuid,$9::text::uuid,$10::text::uuid)";

#[cfg(all(test, feature = "test-support"))]
pub(crate) async fn prepare_runtime_session_thread_establishment_sql(
	client: &tokio_postgres::Client,
) -> Result<usize, StoreError> {
	const SOURCES: [&str; 6] = [
		FENCE_RUNTIME_SESSION_THREAD_START_SQL,
		BIND_RUNTIME_SESSION_THREAD_SQL,
		ACKNOWLEDGE_RUNTIME_SESSION_TURN_SQL,
		READ_ORDINARY_RUNTIME_SESSION_FOR_RESUME_SQL,
		PREPARE_QUICK_TASK_PROCESS_GENERATION_SQL,
		READ_QUICK_TASK_THREAD_ESTABLISHMENT_SQL,
	];
	for source in SOURCES {
		client.prepare(source).await?;
	}
	Ok(SOURCES.len())
}

/// Exact Conversation, RuntimeSession, and active revision-1 Turn coordinates before spawn.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrepareQuickTaskProcessGeneration {
	/// Owning ordinary Conversation.
	pub conversation_id: ConversationId,
	/// Exact current Conversation revision.
	pub expected_conversation_revision: i64,
	/// Starting RuntimeSession selected by the Continuation Plan.
	pub runtime_session_id: RuntimeSessionId,
	/// Exact unfenced RuntimeSession revision.
	pub expected_runtime_session_revision: i64,
	/// Active user Turn admitted atomically with ordinal-0 history.
	pub turn_id: TurnId,
	/// Must be the fresh active Turn revision 1.
	pub expected_turn_revision: i64,
	/// Exact immutable Continuation Plan.
	pub continuation_plan_id: String,
	/// Exact immutable L0 Routing Decision.
	pub routing_decision_id: String,
	/// Selected account bound to the RuntimeSession snapshot.
	pub selected_account_id: AccountId,
	/// Caller-allocated ProcessGeneration identity.
	pub process_generation_id: ProcessGenerationId,
}

/// Durable credential-negative readback of Quick Task ProcessGeneration admission.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuickTaskProcessGenerationReadback {
	/// Exact request coordinates retained by the admission receipt.
	pub request: PrepareQuickTaskProcessGeneration,
	/// Revision of the pre-spawn admission receipt; this is not ProcessGeneration state.
	pub admission_revision: Option<i64>,
	/// Stable pre-spawn rejection, absent for success and unknown classifications.
	pub rejection: Option<QuickTaskProcessGenerationRejection>,
}

/// Positive stable reason why ProcessGeneration admission could not be granted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuickTaskProcessGenerationRejection {
	/// The selected Turn does not exist at the exact Conversation/RuntimeSession coordinates.
	MissingTurn,
	/// The selected Turn exists but is not active.
	InactiveTurn,
	/// The selected Turn is not revision 1.
	StaleTurn,
	/// Some other exact lineage coordinate is absent or stale.
	AuthorityUnavailable,
	/// A closed request value is invalid.
	InvalidInput,
}

/// One-use admission to the ordinary ProcessGeneration spawn owner.
#[derive(Debug, Eq, PartialEq)]
pub struct FreshQuickTaskProcessGeneration {
	protocol_version: &'static str,
	idempotency_key: String,
	readback: QuickTaskProcessGenerationReadback,
}
impl FreshQuickTaskProcessGeneration {
	/// Inspect exact admission coordinates without reconstructing spawn authority.
	pub const fn readback(&self) -> &QuickTaskProcessGenerationReadback {
		&self.readback
	}

	/// Return the exact generation identity covered by this one-use admission.
	pub fn generation_id(&self) -> &ProcessGenerationId {
		&self.readback.request.process_generation_id
	}

	pub(crate) const fn protocol_version(&self) -> &'static str {
		self.protocol_version
	}

	pub(crate) fn idempotency_key(&self) -> &str {
		&self.idempotency_key
	}
}

/// Closed Quick Task ProcessGeneration admission result.
#[derive(Debug, Eq, PartialEq)]
pub enum PrepareQuickTaskProcessGenerationOutcome {
	/// The exact admission committed now; only this variant may reach process spawn.
	Fresh(FreshQuickTaskProcessGeneration),
	/// The exact admission was already durable and cannot spawn again.
	Replayed(QuickTaskProcessGenerationReadback),
	/// Positive stable authority facts rejected the request before spawn.
	Rejected(QuickTaskProcessGenerationReadback),
	/// Persistence or effect receipts cannot establish a definite pre-spawn result.
	Unknown(QuickTaskProcessGenerationReadback),
}

/// Exact immutable coordinates used to reconcile RuntimeSession Thread Establishment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReconcileQuickTaskThreadEstablishment {
	/// Owning ordinary Conversation.
	pub conversation_id: ConversationId,
	/// Exact current Conversation revision.
	pub expected_conversation_revision: i64,
	/// RuntimeSession selected by the Continuation Plan.
	pub runtime_session_id: RuntimeSessionId,
	/// RuntimeSession revision before its thread-start fence.
	pub expected_runtime_session_revision: i64,
	/// Selected active user Turn.
	pub turn_id: TurnId,
	/// Must be the fresh active Turn revision 1.
	pub expected_turn_revision: i64,
	/// Exact immutable Continuation Plan.
	pub continuation_plan_id: String,
	/// Exact immutable L0 Routing Decision.
	pub routing_decision_id: String,
	/// Exact account selected by that Routing Decision.
	pub selected_account_id: AccountId,
	/// Exact ProcessGeneration used for the attempted start.
	pub process_generation_id: ProcessGenerationId,
}

/// Source of positive proof that no provider thread-start effect can exist.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuickTaskPreEffectEvidenceKind {
	/// Exact ProcessGeneration admission was stably rejected before spawn authority existed.
	AdmissionRejected,
	/// The ordinary process owner recorded that no child process was created.
	SpawnNotCreated,
	/// The exact ProcessGeneration has positive death evidence and no thread-start fence.
	ProcessDead,
}

/// Positive proof that no provider thread-start effect can exist for one exact attempt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuickTaskThreadStartNonEffect {
	/// Exact ProcessGeneration revision when ordinary process state exists.
	pub process_generation_revision: Option<i64>,
	/// Closed kind of positive pre-effect evidence.
	pub kind: QuickTaskPreEffectEvidenceKind,
	/// Exact positive receipt or ProcessGeneration evidence identity.
	pub evidence_id: String,
}

/// Credential-negative reconciliation of exact ProcessGeneration and RuntimeSession receipts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QuickTaskThreadEstablishmentReadback {
	/// A successful thread response is durably bound to the RuntimeSession.
	Bound(RuntimeSessionThreadBindingReadback),
	/// A thread-start fence exists without a matching durable binding.
	Fenced(RuntimeSessionThreadFenceReadback),
	/// Positive evidence proves the process never reached a provider thread-start effect.
	DefinitelyNotStarted(QuickTaskThreadStartNonEffect),
	/// Exact receipts cannot prove a bound thread or a pre-effect failure.
	Unknown,
}

/// Complete caller-observed, non-secret account facts consumed by one exact creation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateRuntimeSessionAccountSnapshot {
	/// Caller-selected immutable snapshot identity.
	pub account_snapshot_id: String,
	/// Stable non-secret source account identity.
	pub source_account_id: AccountId,
	/// Exact display label observed at binding time.
	pub display_label: String,
	/// Exact inert account state observed at binding time.
	pub observed_state: AccountState,
	/// Positive source account revision observed at binding time.
	pub source_revision: i64,
}

/// Complete immutable non-secret account snapshot returned by PostgreSQL.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeSessionAccountSnapshot {
	/// Caller-selected immutable snapshot identity.
	pub account_snapshot_id: String,
	/// Stable non-secret source account identity.
	pub source_account_id: AccountId,
	/// Exact display label observed at binding time.
	pub display_label: String,
	/// Exact inert account state observed at binding time.
	pub observed_state: AccountState,
	/// Positive source account revision observed at binding time.
	pub source_revision: i64,
	/// PostgreSQL-authored immutable creation timestamp.
	pub created_at: String,
}

/// Immutable full RoleProfile revision selected by PostgreSQL at session creation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeSessionProfileSnapshot {
	/// PostgreSQL-generated immutable snapshot identity.
	pub profile_snapshot_id: String,
	/// Selected global role identity.
	pub role: RoleProfileRole,
	/// Selected immutable RoleProfile revision.
	pub source_revision: i64,
	/// Exact selected model.
	pub model: String,
	/// Exact selected reasoning effort.
	pub reasoning_effort: String,
	/// Exact selected service tier.
	pub service_tier: String,
	/// Digest of the exact selected instruction bytes.
	pub instructions_digest: String,
	/// Exact selected instruction bytes represented as UTF-8.
	pub instructions: String,
	/// Optional exact selected provenance.
	pub provenance: Option<String>,
	/// PostgreSQL-authored immutable creation timestamp.
	pub created_at: String,
}

/// Typed inputs consumed by the exact RuntimeSession creation command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateRuntimeSession {
	/// Caller-selected RuntimeSession identity.
	pub runtime_session_id: RuntimeSessionId,
	/// Existing logical Conversation target.
	pub conversation_id: ConversationId,
	/// Role whose one current immutable revision PostgreSQL must select.
	pub role: RoleProfileRole,
	/// Complete non-secret account snapshot identity and facts.
	pub account_snapshot: CreateRuntimeSessionAccountSnapshot,
	/// Must be null. RuntimeSession Thread Establishment binds a thread only through the exact
	/// fence/bind authority.
	pub codex_thread_id: Option<String>,
	/// Must be `starting`; RuntimeSession activation is owned by the exact bind command.
	pub initial_state: RuntimeSessionState,
}

/// Complete committed RuntimeSession and immutable snapshot readback.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredRuntimeSession {
	/// Stable RuntimeSession identity.
	pub runtime_session_id: RuntimeSessionId,
	/// Parent logical Conversation identity.
	pub conversation_id: ConversationId,
	/// PostgreSQL-selected full immutable RoleProfile snapshot.
	pub profile_snapshot: RuntimeSessionProfileSnapshot,
	/// Exact immutable non-secret account snapshot.
	pub account_snapshot: RuntimeSessionAccountSnapshot,
	/// Optional immutable Codex thread correlation.
	pub codex_thread_id: Option<String>,
	/// Always null at creation and immutable in this command slice.
	pub last_known_turn_id: Option<String>,
	/// Current persisted lifecycle state.
	pub state: RuntimeSessionState,
	/// Positive optimistic revision.
	pub revision: i64,
	/// PostgreSQL-authored creation timestamp.
	pub created_at: String,
	/// PostgreSQL-authored current revision timestamp.
	pub updated_at: String,
	/// PostgreSQL-authored terminal timestamp.
	pub ended_at: Option<String>,
}

/// Complete canonical effect returned by a successful exact RuntimeSession command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeSessionCommandEffect {
	/// Complete current RuntimeSession and immutable snapshots.
	pub runtime_session: StoredRuntimeSession,
	/// State before the command; null only for creation.
	pub prior_state: Option<RuntimeSessionState>,
	/// State after the command.
	pub new_state: RuntimeSessionState,
	/// Revision before the command; null only for creation.
	pub prior_revision: Option<i64>,
	/// Revision after the command.
	pub new_revision: i64,
	/// Canonical append-only activity identity.
	pub activity_sequence: i64,
	/// Exact activity payload stored by PostgreSQL.
	pub activity_payload: Value,
	/// Canonical outbox identity.
	pub outbox_id: i64,
	/// Exact outbox payload stored by PostgreSQL.
	pub outbox_payload: Value,
}

/// Stable domain rejection committed and replayed by an exact RuntimeSession command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeSessionRejection {
	/// The referenced Conversation, profile, or RuntimeSession does not exist.
	MissingTarget,
	/// The requested RuntimeSession identity already exists.
	DuplicateTarget,
	/// The expected RuntimeSession revision is no longer current.
	StaleRevision,
	/// The requested initial state or state transition is illegal.
	IllegalTransition,
	/// The supplied account snapshot facts are not valid non-secret snapshot facts.
	InvalidAccountSnapshot,
	/// An existing account snapshot identity is bound to different immutable facts.
	AccountSnapshotConflict,
}

/// Parsed exact command result; stable rejections are values, not infrastructure errors.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimeSessionCommandOutcome<T> {
	/// The command committed and returned the authoritative RuntimeSession snapshot.
	Success(T),
	/// The command committed a stable domain rejection.
	Rejected(RuntimeSessionRejection),
}

/// Exact credential-negative facts required to fence one existing RuntimeSession thread start.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FenceRuntimeSessionThreadStart {
	/// Owning ordinary Conversation.
	pub conversation_id: ConversationId,
	/// Exact current Conversation revision.
	pub expected_conversation_revision: i64,
	/// Existing unfenced RuntimeSession selected by the initial continuation plan.
	pub runtime_session_id: RuntimeSessionId,
	/// Exact Continuation Plan source RuntimeSession revision.
	pub expected_revision: i64,
	/// Selected active user Turn.
	pub turn_id: TurnId,
	/// Must be the fresh active Turn revision 1.
	pub expected_turn_revision: i64,
	/// Exact initial Continuation Plan identity.
	pub continuation_plan_id: String,
	/// Exact ready ProcessGeneration identity.
	pub process_generation_id: ProcessGenerationId,
	/// Exact ready ProcessGeneration revision.
	pub process_generation_revision: i64,
	/// Exact active execution epoch bound to the ProcessGeneration.
	pub process_execution_epoch_id: ProcessExecutionEpochId,
	/// Positive JSON-RPC request identity for the one `thread/start` call.
	pub thread_start_request_id: i64,
	/// Lowercase SHA-256 of the exact `thread/start` request bytes.
	pub thread_start_request_sha256: String,
}

/// Durable readback of one committed RuntimeSession thread-start fence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeSessionThreadFenceReadback {
	/// Exact receipt key that owns this fence.
	pub fence_idempotency_key: String,
	/// Owning ordinary Conversation.
	pub conversation_id: ConversationId,
	/// Exact Conversation revision locked by the fence.
	pub conversation_revision: i64,
	/// Existing RuntimeSession whose revision advanced.
	pub runtime_session_id: RuntimeSessionId,
	/// RuntimeSession revision before the fence.
	pub prior_revision: i64,
	/// RuntimeSession revision after the fence.
	pub revision: i64,
	/// Exact active user Turn locked by the fence.
	pub turn_id: TurnId,
	/// Exact active Turn revision, always 1.
	pub turn_revision: i64,
	/// Exact initial Continuation Plan.
	pub continuation_plan_id: String,
	/// Exact selected Routing Decision.
	pub routing_decision_id: String,
	/// Exact account selected by Routing Decision and bound to the ready generation.
	pub selected_account_id: AccountId,
	/// Exact ready ProcessGeneration.
	pub process_generation_id: ProcessGenerationId,
	/// Exact ready ProcessGeneration revision.
	pub process_generation_revision: i64,
	/// Exact active execution epoch.
	pub process_execution_epoch_id: ProcessExecutionEpochId,
	/// Exact positive `thread/start` request identity.
	pub thread_start_request_id: i64,
	/// Lowercase SHA-256 of the exact `thread/start` request bytes.
	pub thread_start_request_sha256: String,
	/// Append-only RuntimeSession activity identity.
	pub activity_sequence: i64,
	/// Transactional outbox identity for that activity.
	pub outbox_id: i64,
}

/// Newly committed one-call `thread/start` authority.
///
/// This type is intentionally not `Clone`. A receipt replay returns only
/// [`RuntimeSessionThreadFenceReadback`] and cannot reconstruct this capability.
#[derive(Debug, Eq, PartialEq)]
pub struct FreshRuntimeSessionThreadStart {
	readback: RuntimeSessionThreadFenceReadback,
}

impl FreshRuntimeSessionThreadStart {
	/// Inspect the exact durable fence without consuming its one-call authority.
	pub const fn readback(&self) -> &RuntimeSessionThreadFenceReadback {
		&self.readback
	}

	/// Consume the one-call authority after a typed successful response is available.
	pub fn into_binding(
		self,
		successful_response: SuccessfulRuntimeSessionThreadStart,
	) -> BindRuntimeSessionThread {
		BindRuntimeSessionThread {
			conversation_id: self.readback.conversation_id,
			expected_conversation_revision: self.readback.conversation_revision,
			runtime_session_id: self.readback.runtime_session_id,
			expected_revision: self.readback.revision,
			turn_id: self.readback.turn_id,
			expected_turn_revision: self.readback.turn_revision,
			continuation_plan_id: self.readback.continuation_plan_id,
			fence_idempotency_key: self.readback.fence_idempotency_key,
			thread_start_request_id: self.readback.thread_start_request_id,
			thread_start_request_sha256: self.readback.thread_start_request_sha256,
			successful_response,
		}
	}
}

/// Result of the exact RuntimeSession thread-start fence command.
#[derive(Debug, Eq, PartialEq)]
pub enum FenceRuntimeSessionThreadStartOutcome {
	/// The fence committed now and authorizes exactly one provider call.
	Fresh(FreshRuntimeSessionThreadStart),
	/// The exact fence was already committed. This is readback only.
	Replayed(RuntimeSessionThreadFenceReadback),
	/// PostgreSQL committed a stable rejection without creating start authority.
	Rejected(RuntimeSessionThreadEstablishmentRejection),
}

/// Exact facts from one typed successful `thread/start` response.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SuccessfulRuntimeSessionThreadStart {
	/// Positive response identity matching the request identity.
	pub response_id: i64,
	/// Lowercase SHA-256 of the exact typed successful response bytes.
	pub response_sha256: String,
	/// Exact thread identity returned by the successful response.
	pub codex_thread_id: String,
}

/// Stable rejection shared by the two exact RuntimeSession thread commands.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeSessionThreadEstablishmentRejection {
	/// A required positive identity, revision, digest, or closed fact was invalid.
	InvalidInput,
	/// The Routing Decision/Continuation Plan, RuntimeSession, ProcessGeneration, epoch, or fence
	/// lineage was stale.
	AuthorityUnavailable,
}

/// Exact successful `thread/start` facts required to bind a fenced RuntimeSession.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BindRuntimeSessionThread {
	/// Owning ordinary Conversation.
	pub conversation_id: ConversationId,
	/// Exact current Conversation revision.
	pub expected_conversation_revision: i64,
	/// Fenced RuntimeSession identity.
	pub runtime_session_id: RuntimeSessionId,
	/// Exact fenced successor revision.
	pub expected_revision: i64,
	/// Selected active user Turn.
	pub turn_id: TurnId,
	/// Must remain the active Turn revision 1.
	pub expected_turn_revision: i64,
	/// Exact initial Continuation Plan identity.
	pub continuation_plan_id: String,
	/// Exact receipt key of the fresh fence.
	pub fence_idempotency_key: String,
	/// Positive request identity already stored by the fence.
	pub thread_start_request_id: i64,
	/// Lowercase SHA-256 already stored by the fence.
	pub thread_start_request_sha256: String,
	/// Exact typed successful response facts.
	pub successful_response: SuccessfulRuntimeSessionThreadStart,
}

/// Durable readback of one committed RuntimeSession thread binding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeSessionThreadBindingReadback {
	/// Owning ordinary Conversation.
	pub conversation_id: ConversationId,
	/// Exact Conversation revision locked by the binding.
	pub conversation_revision: i64,
	/// Bound RuntimeSession identity.
	pub runtime_session_id: RuntimeSessionId,
	/// RuntimeSession revision before binding.
	pub prior_revision: i64,
	/// RuntimeSession revision after binding.
	pub revision: i64,
	/// Exact active user Turn locked by the binding.
	pub turn_id: TurnId,
	/// Exact active Turn revision, always 1.
	pub turn_revision: i64,
	/// RuntimeSession revision before the fence.
	pub fence_prior_revision: i64,
	/// RuntimeSession revision committed by the fence.
	pub fence_revision: i64,
	/// Exact initial Continuation Plan.
	pub continuation_plan_id: String,
	/// Exact receipt key of the preceding fence.
	pub fence_idempotency_key: String,
	/// Exact receipt key of this completed binding.
	pub binding_idempotency_key: String,
	/// Exact positive `thread/start` request identity.
	pub thread_start_request_id: i64,
	/// Lowercase SHA-256 of the exact request bytes.
	pub thread_start_request_sha256: String,
	/// Exact positive successful response identity.
	pub thread_start_response_id: i64,
	/// Lowercase SHA-256 of the exact successful response bytes.
	pub thread_start_response_sha256: String,
	/// Exact bound Codex thread identity.
	pub codex_thread_id: String,
	/// Append-only RuntimeSession activity identity.
	pub activity_sequence: i64,
	/// Transactional outbox identity for that activity.
	pub outbox_id: i64,
}

/// Result of the exact RuntimeSession thread bind command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BindRuntimeSessionThreadOutcome {
	/// The binding committed now.
	Applied(RuntimeSessionThreadBindingReadback),
	/// The exact binding was already committed and was read back.
	Replayed(RuntimeSessionThreadBindingReadback),
	/// PostgreSQL committed a stable rejection without changing the RuntimeSession.
	Rejected(RuntimeSessionThreadEstablishmentRejection),
}

/// Exact positive terminal facts consumed by the RuntimeSession-owned turn acknowledgement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AcknowledgeRuntimeSessionTurn {
	/// Open ordinary Conversation owning the logical Turn.
	pub conversation_id: ConversationId,
	/// Exact current Conversation revision.
	pub expected_conversation_revision: i64,
	/// Active bound RuntimeSession that owns the durable thread.
	pub runtime_session_id: RuntimeSessionId,
	/// Exact pre-acknowledgement RuntimeSession revision.
	pub expected_runtime_session_revision: i64,
	/// Exact logical user Turn accepted by the ProviderAttempt.
	pub user_turn_id: TurnId,
	/// Exact terminal user Turn revision committed by the conversations owner.
	pub expected_user_turn_revision: i64,
	/// Optional terminal assistant Turn and revision committed by the conversations owner.
	pub assistant_turn: Option<(TurnId, i64)>,
	/// Exact terminal generic ProviderAttempt.
	pub provider_attempt_id: ProviderAttemptId,
	/// Exact terminal ProviderAttempt revision.
	pub expected_provider_attempt_revision: i64,
	/// Exact positive terminal evidence identity.
	pub provider_evidence_id: ProviderEvidenceId,
	/// Exact positive terminal outcome shared by the attempt and evidence.
	pub provider_outcome: ProviderTerminalOutcome,
	/// Exact provider thread, which must equal the bound RuntimeSession thread.
	pub provider_thread_id: String,
	/// Exact positive provider turn identity stored as `last_known_turn_id`.
	pub provider_turn_id: String,
}

/// Strict readback of one committed RuntimeSession-owned turn acknowledgement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeSessionTurnAcknowledgementReadback {
	/// Exact acknowledged RuntimeSession.
	pub runtime_session_id: RuntimeSessionId,
	/// RuntimeSession revision before acknowledgement.
	pub prior_revision: i64,
	/// RuntimeSession revision after acknowledgement.
	pub revision: i64,
	/// Exact terminal logical user Turn.
	pub user_turn_id: TurnId,
	/// Exact terminal user Turn revision observed by the RuntimeSession owner.
	pub user_turn_revision: i64,
	/// Optional terminal assistant Turn.
	pub assistant_turn_id: Option<TurnId>,
	/// Optional terminal assistant Turn revision observed by the RuntimeSession owner.
	pub assistant_turn_revision: Option<i64>,
	/// Exact terminal ProviderAttempt revision.
	pub provider_attempt_revision: i64,
	/// Exact positive evidence identity.
	pub provider_evidence_id: ProviderEvidenceId,
	/// Exact durable provider thread identity.
	pub provider_thread_id: String,
	/// Exact new `last_known_turn_id`.
	pub provider_turn_id: String,
	/// Append-only RuntimeSession activity identity.
	pub activity_sequence: i64,
	/// Transactional outbox identity.
	pub outbox_id: i64,
}

/// Result of the exact RuntimeSession-owned terminal acknowledgement command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AcknowledgeRuntimeSessionTurnOutcome {
	/// The RuntimeSession acknowledgement committed now.
	Applied(RuntimeSessionTurnAcknowledgementReadback),
	/// The same command was already committed; this is strict readback only.
	Replayed(RuntimeSessionTurnAcknowledgementReadback),
	/// Exact authority was incomplete or stale and no facts changed.
	Rejected(RuntimeSessionThreadEstablishmentRejection),
}

/// Credential-negative read model for rebuilding one ordinary active RuntimeSession owner.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OrdinaryRuntimeSessionResumeReadback {
	/// Exact owning Conversation.
	pub conversation_id: ConversationId,
	/// Current Conversation revision used by the next Routing Decision consumer.
	pub conversation_revision: i64,
	/// Exact active RuntimeSession.
	pub runtime_session_id: RuntimeSessionId,
	/// Current bound RuntimeSession revision.
	pub runtime_session_revision: i64,
	/// Durable thread fact established by the exact fence/bind pair.
	pub codex_thread_id: String,
	/// Immutable Task profile model.
	pub model: String,
	/// Immutable Task profile reasoning effort.
	pub reasoning_effort: String,
	/// Immutable Task profile instructions.
	pub instructions: String,
	/// Historical account selected for this RuntimeSession.
	pub source_account_id: AccountId,
	/// Historical account revision captured by the immutable snapshot.
	pub source_account_revision: i64,
	/// First unused ordinary Conversation Turn sequence.
	pub next_turn_sequence: i64,
	/// Exact positive thread-start request identity.
	pub thread_start_request_id: i64,
	/// Exact lowercase SHA-256 of the thread-start request.
	pub thread_start_request_sha256: String,
	/// Exact positive successful response identity.
	pub thread_start_response_id: i64,
	/// Exact lowercase SHA-256 of the successful response.
	pub thread_start_response_sha256: String,
	/// True only after the RuntimeSession owner durably acknowledged a positive terminal Turn.
	pub has_acknowledged_turn: bool,
	/// True when a prior ordinary Turn is still active.
	pub has_active_turn: bool,
	/// True when a prepared, authorized, or unknown ProviderAttempt still exists.
	pub has_unresolved_provider_attempt: bool,
}

impl PostgresStore {
	/// Create one RuntimeSession and both snapshots through the final V12 command definition.
	pub async fn create_runtime_session(
		&self,
		idempotency_key: &str,
		create: &CreateRuntimeSession,
	) -> Result<RuntimeSessionCommandOutcome<RuntimeSessionCommandEffect>, StoreError> {
		validate_exact_key(idempotency_key)?;
		validate_uuid(&create.account_snapshot.account_snapshot_id, "account snapshot identity")?;
		if create.codex_thread_id.is_some() {
			return Err(StoreError::InvalidInput(
				"RuntimeSession creation cannot bind a Codex thread",
			));
		}
		if create.initial_state != RuntimeSessionState::Starting {
			return Err(StoreError::InvalidInput(
				"RuntimeSession creation must begin unfenced and starting",
			));
		}

		let role = create.role.as_sql();
		let observed_state = account_state_sql(create.account_snapshot.observed_state);
		let initial_state = session_state_sql(create.initial_state);
		let response = self
			.execute_exact_with_retry(
				"SELECT decodex.create_runtime_session_exact(\
				 $1,$2,$3::text::uuid,$4::text::uuid,\
				 $5::text::decodex.role_profile_role,$6::text::uuid,$7::text::uuid,\
				 $8,$9::text::decodex.account_state,$10,$11::text::uuid,\
				 $12::text::decodex.runtime_session_state)",
				&[
					&EXACT_COMMAND_PROTOCOL,
					&idempotency_key,
					&create.runtime_session_id.as_str(),
					&create.conversation_id.as_str(),
					&role,
					&create.account_snapshot.account_snapshot_id,
					&create.account_snapshot.source_account_id.as_str(),
					&create.account_snapshot.display_label,
					&observed_state,
					&create.account_snapshot.source_revision,
					&create.codex_thread_id,
					&initial_state,
				],
			)
			.await?;

		parse_create_response(&response, create)
	}

	/// Transition one RuntimeSession through the final V12 command definition.
	pub async fn transition_runtime_session(
		&self,
		idempotency_key: &str,
		session_id: &RuntimeSessionId,
		expected_revision: i64,
		target_state: RuntimeSessionState,
	) -> Result<RuntimeSessionCommandOutcome<RuntimeSessionCommandEffect>, StoreError> {
		validate_exact_key(idempotency_key)?;
		if target_state == RuntimeSessionState::Active {
			return Err(StoreError::InvalidInput(
				"RuntimeSession activation requires the exact thread bind command",
			));
		}
		let target_state_sql = session_state_sql(target_state);
		let response = self
			.execute_exact_with_retry(
				"SELECT decodex.transition_runtime_session_exact(\
				 $1,$2,$3::text::uuid,$4,$5::text::decodex.runtime_session_state)",
				&[
					&EXACT_COMMAND_PROTOCOL,
					&idempotency_key,
					&session_id.as_str(),
					&expected_revision,
					&target_state_sql,
				],
			)
			.await?;

		parse_transition_response(&response, session_id, expected_revision, target_state)
	}

	/// Admit one Quick Task ProcessGeneration against exact active revision-1 Turn authority.
	pub async fn prepare_quick_task_process_generation(
		&self,
		idempotency_key: &str,
		request: &PrepareQuickTaskProcessGeneration,
	) -> Result<PrepareQuickTaskProcessGenerationOutcome, StoreError> {
		validate_exact_key(idempotency_key)?;
		validate_thread_establishment_revision(request.expected_conversation_revision)?;
		validate_thread_establishment_revision(request.expected_runtime_session_revision)?;
		validate_active_turn_revision(request.expected_turn_revision)?;
		validate_uuid(&request.continuation_plan_id, "continuation plan identity")?;
		validate_uuid(&request.routing_decision_id, "Routing Decision identity")?;

		let (response, replayed) = self
			.execute_exact_with_replay_status(
				PREPARE_QUICK_TASK_PROCESS_GENERATION_SQL,
				&[
					&EXACT_COMMAND_PROTOCOL,
					&idempotency_key,
					&request.conversation_id.as_str(),
					&request.expected_conversation_revision,
					&request.runtime_session_id.as_str(),
					&request.expected_runtime_session_revision,
					&request.turn_id.as_str(),
					&request.expected_turn_revision,
					&request.continuation_plan_id,
					&request.routing_decision_id,
					&request.selected_account_id.as_str(),
					&request.process_generation_id.as_str(),
				],
			)
			.await?;
		let (classification, readback) =
			parse_quick_task_process_generation_response(&response, request)?;
		match classification.as_str() {
			"success" if replayed =>
				Ok(PrepareQuickTaskProcessGenerationOutcome::Replayed(readback)),
			"success" => Ok(PrepareQuickTaskProcessGenerationOutcome::Fresh(
				FreshQuickTaskProcessGeneration {
					protocol_version: EXACT_COMMAND_PROTOCOL,
					idempotency_key: idempotency_key.to_owned(),
					readback,
				},
			)),
			"stable_domain_rejection" =>
				Ok(PrepareQuickTaskProcessGenerationOutcome::Rejected(readback)),
			"effect_or_persistence_unknown" =>
				Ok(PrepareQuickTaskProcessGenerationOutcome::Unknown(readback)),
			_ => Err(StoreError::Incompatible(
				"Quick Task ProcessGeneration classification is invalid".into(),
			)),
		}
	}

	/// Reconcile ProcessGeneration, thread fence, bind, and positive non-effect receipts.
	pub async fn reconcile_quick_task_thread_establishment(
		&self,
		request: &ReconcileQuickTaskThreadEstablishment,
	) -> Result<QuickTaskThreadEstablishmentReadback, StoreError> {
		validate_thread_establishment_revision(request.expected_conversation_revision)?;
		validate_thread_establishment_revision(request.expected_runtime_session_revision)?;
		validate_active_turn_revision(request.expected_turn_revision)?;
		validate_uuid(&request.continuation_plan_id, "continuation plan identity")?;

		let row = self
			.pool()
			.get()
			.await?
			.query_one(
				READ_QUICK_TASK_THREAD_ESTABLISHMENT_SQL,
				&[
					&request.conversation_id.as_str(),
					&request.expected_conversation_revision,
					&request.runtime_session_id.as_str(),
					&request.expected_runtime_session_revision,
					&request.turn_id.as_str(),
					&request.expected_turn_revision,
					&request.continuation_plan_id,
					&request.routing_decision_id,
					&request.selected_account_id.as_str(),
					&request.process_generation_id.as_str(),
				],
			)
			.await?;
		let readback: Option<Value> = row.get(0);
		match readback {
			Some(readback) => parse_quick_task_thread_establishment_readback(&readback, request),
			None => Ok(QuickTaskThreadEstablishmentReadback::Unknown),
		}
	}

	/// Fence one existing RuntimeSession before its only permitted `thread/start` call.
	pub async fn fence_runtime_session_thread_start(
		&self,
		idempotency_key: &str,
		fence: &FenceRuntimeSessionThreadStart,
	) -> Result<FenceRuntimeSessionThreadStartOutcome, StoreError> {
		validate_exact_key(idempotency_key)?;
		validate_uuid(&fence.continuation_plan_id, "continuation plan identity")?;
		validate_thread_establishment_revision(fence.expected_conversation_revision)?;
		validate_thread_establishment_revision(fence.expected_revision)?;
		validate_active_turn_revision(fence.expected_turn_revision)?;
		validate_thread_establishment_revision(fence.process_generation_revision)?;
		validate_thread_start_id(fence.thread_start_request_id)?;
		validate_sha256(&fence.thread_start_request_sha256)?;

		let (response, replayed) = self
			.execute_exact_with_replay_status(
				FENCE_RUNTIME_SESSION_THREAD_START_SQL,
				&[
					&EXACT_COMMAND_PROTOCOL,
					&idempotency_key,
					&fence.conversation_id.as_str(),
					&fence.expected_conversation_revision,
					&fence.runtime_session_id.as_str(),
					&fence.expected_revision,
					&fence.turn_id.as_str(),
					&fence.expected_turn_revision,
					&fence.continuation_plan_id,
					&fence.process_generation_id.as_str(),
					&fence.process_generation_revision,
					&fence.process_execution_epoch_id.as_str(),
					&fence.thread_start_request_id,
					&fence.thread_start_request_sha256,
				],
			)
			.await?;
		if let Some(rejection) =
			parse_thread_establishment_rejection(&response, "fence_runtime_session_thread_start")?
		{
			return Ok(FenceRuntimeSessionThreadStartOutcome::Rejected(rejection));
		}
		let readback = parse_thread_fence_response(&response, idempotency_key, fence)?;

		if replayed {
			Ok(FenceRuntimeSessionThreadStartOutcome::Replayed(readback))
		} else {
			Ok(FenceRuntimeSessionThreadStartOutcome::Fresh(FreshRuntimeSessionThreadStart {
				readback,
			}))
		}
	}

	/// Bind one typed successful `thread/start` response to its exact committed fence.
	pub async fn bind_runtime_session_thread(
		&self,
		idempotency_key: &str,
		binding: &BindRuntimeSessionThread,
	) -> Result<BindRuntimeSessionThreadOutcome, StoreError> {
		validate_exact_key(idempotency_key)?;
		validate_exact_key(&binding.fence_idempotency_key)?;
		validate_uuid(&binding.continuation_plan_id, "continuation plan identity")?;
		validate_uuid(&binding.successful_response.codex_thread_id, "Codex thread identity")?;
		validate_thread_establishment_revision(binding.expected_conversation_revision)?;
		validate_thread_establishment_revision(binding.expected_revision)?;
		validate_active_turn_revision(binding.expected_turn_revision)?;
		if binding.expected_revision <= 1 {
			return Err(StoreError::InvalidInput(
				"fenced RuntimeSession revision must have a positive predecessor",
			));
		}
		validate_thread_start_id(binding.thread_start_request_id)?;
		validate_thread_start_id(binding.successful_response.response_id)?;
		if binding.successful_response.response_id != binding.thread_start_request_id {
			return Err(StoreError::InvalidInput(
				"thread/start response identity must equal its request identity",
			));
		}
		validate_sha256(&binding.thread_start_request_sha256)?;
		validate_sha256(&binding.successful_response.response_sha256)?;
		if idempotency_key == binding.fence_idempotency_key {
			return Err(StoreError::InvalidInput(
				"RuntimeSession fence and bind receipt keys must differ",
			));
		}

		let (response, replayed) = self
			.execute_exact_with_replay_status(
				BIND_RUNTIME_SESSION_THREAD_SQL,
				&[
					&EXACT_COMMAND_PROTOCOL,
					&idempotency_key,
					&binding.conversation_id.as_str(),
					&binding.expected_conversation_revision,
					&binding.runtime_session_id.as_str(),
					&binding.expected_revision,
					&binding.turn_id.as_str(),
					&binding.expected_turn_revision,
					&binding.continuation_plan_id,
					&EXACT_COMMAND_PROTOCOL,
					&binding.fence_idempotency_key,
					&binding.thread_start_request_id,
					&binding.thread_start_request_sha256,
					&binding.successful_response.response_id,
					&binding.successful_response.response_sha256,
					&binding.successful_response.codex_thread_id,
				],
			)
			.await?;
		if let Some(rejection) =
			parse_thread_establishment_rejection(&response, "bind_runtime_session_thread")?
		{
			return Ok(BindRuntimeSessionThreadOutcome::Rejected(rejection));
		}
		let readback = parse_thread_binding_response(&response, idempotency_key, binding)?;

		if replayed {
			Ok(BindRuntimeSessionThreadOutcome::Replayed(readback))
		} else {
			Ok(BindRuntimeSessionThreadOutcome::Applied(readback))
		}
	}

	/// Acknowledge Conversation-owned terminal Turns and advance their RuntimeSession thread owner.
	pub async fn acknowledge_runtime_session_turn(
		&self,
		idempotency_key: &str,
		acknowledgement: &AcknowledgeRuntimeSessionTurn,
	) -> Result<AcknowledgeRuntimeSessionTurnOutcome, StoreError> {
		validate_exact_key(idempotency_key)?;
		validate_thread_establishment_revision(acknowledgement.expected_conversation_revision)?;
		validate_thread_establishment_revision(acknowledgement.expected_runtime_session_revision)?;
		validate_thread_establishment_revision(acknowledgement.expected_user_turn_revision)?;
		validate_thread_establishment_revision(acknowledgement.expected_provider_attempt_revision)?;
		if acknowledgement.expected_provider_attempt_revision <= 1 {
			return Err(StoreError::InvalidInput(
				"terminal ProviderAttempt revision must have a positive predecessor",
			));
		}
		if acknowledgement.provider_outcome == ProviderTerminalOutcome::NotSubmitted {
			return Err(StoreError::InvalidInput(
				"RuntimeSession turn acknowledgement requires submitted positive evidence",
			));
		}
		if let Some((_, revision)) = &acknowledgement.assistant_turn {
			validate_thread_establishment_revision(*revision)?;
		}
		validate_uuid(&acknowledgement.provider_thread_id, "provider thread identity")?;
		if acknowledgement.provider_turn_id.is_empty()
			|| acknowledgement.provider_turn_id.len() > 256
			|| acknowledgement.provider_turn_id.chars().any(char::is_control)
		{
			return Err(StoreError::InvalidInput("provider turn identity"));
		}
		crate::ensure_credential_negative_text(&acknowledgement.provider_turn_id)?;

		let assistant_turn_id =
			acknowledgement.assistant_turn.as_ref().map(|(turn_id, _)| turn_id.as_str().to_owned());
		let assistant_turn_revision =
			acknowledgement.assistant_turn.as_ref().map(|(_, revision)| *revision);
		let provider_outcome = provider_terminal_outcome_sql(acknowledgement.provider_outcome);
		let (response, replayed) = self
			.execute_exact_with_replay_status(
				ACKNOWLEDGE_RUNTIME_SESSION_TURN_SQL,
				&[
					&EXACT_COMMAND_PROTOCOL,
					&idempotency_key,
					&acknowledgement.conversation_id.as_str(),
					&acknowledgement.expected_conversation_revision,
					&acknowledgement.runtime_session_id.as_str(),
					&acknowledgement.expected_runtime_session_revision,
					&acknowledgement.user_turn_id.as_str(),
					&acknowledgement.expected_user_turn_revision,
					&assistant_turn_id,
					&assistant_turn_revision,
					&acknowledgement.provider_attempt_id.as_str(),
					&acknowledgement.expected_provider_attempt_revision,
					&acknowledgement.provider_evidence_id.as_str(),
					&provider_outcome,
					&acknowledgement.provider_thread_id,
					&acknowledgement.provider_turn_id,
				],
			)
			.await?;
		if let Some(rejection) =
			parse_thread_establishment_rejection(&response, "acknowledge_runtime_session_turn")?
		{
			return Ok(AcknowledgeRuntimeSessionTurnOutcome::Rejected(rejection));
		}
		let readback = parse_turn_acknowledgement_response(&response, acknowledgement)?;
		if replayed {
			Ok(AcknowledgeRuntimeSessionTurnOutcome::Replayed(readback))
		} else {
			Ok(AcknowledgeRuntimeSessionTurnOutcome::Applied(readback))
		}
	}
}

impl PostgresStore {
	/// Read the sole active ordinary RuntimeSession for one Conversation.
	///
	/// This read grants no process, resume, preparation, or dispatch authority. Multiple active
	/// sessions, incomplete thread facts, or malformed immutable snapshots fail closed.
	pub async fn read_ordinary_runtime_session_for_resume(
		&self,
		conversation_id: &ConversationId,
	) -> Result<Option<OrdinaryRuntimeSessionResumeReadback>, StoreError> {
		let rows = self
			.pool()
			.get()
			.await?
			.query(READ_ORDINARY_RUNTIME_SESSION_FOR_RESUME_SQL, &[&conversation_id.as_str()])
			.await?;
		if rows.is_empty() {
			return Ok(None);
		}
		if rows.len() != 1 {
			return Err(StoreError::Incompatible(
				"ordinary Conversation has multiple active RuntimeSessions".into(),
			));
		}
		let row = &rows[0];
		let conversation_revision: i64 = row.get(0);
		let runtime_session_id = RuntimeSessionId::new(row.get::<_, String>(1)).map_err(|_| {
			StoreError::Incompatible("stored RuntimeSession identity is invalid".into())
		})?;
		let runtime_session_revision: i64 = row.get(2);
		let codex_thread_id: Option<String> = row.get(3);
		let model: String = row.get(4);
		let reasoning_effort: String = row.get(5);
		let instructions: String = row.get(6);
		let source_account_id = AccountId::new(row.get::<_, String>(7)).map_err(|_| {
			StoreError::Incompatible("stored RuntimeSession account identity is invalid".into())
		})?;
		let source_account_revision: i64 = row.get(8);
		let next_turn_sequence: i64 = row.get(9);
		let thread_start_request_id: Option<i64> = row.get(10);
		let thread_start_request_sha256: Option<String> = row.get(11);
		let thread_start_response_id: Option<i64> = row.get(12);
		let thread_start_response_sha256: Option<String> = row.get(13);
		let has_acknowledged_turn: bool = row.get(14);
		let has_active_turn: bool = row.get(15);
		let has_unresolved_provider_attempt: bool = row.get(16);
		let conversation_status: String = row.get(17);
		let profile_role: String = row.get(18);
		let (
			Some(codex_thread_id),
			Some(thread_start_request_id),
			Some(thread_start_request_sha256),
			Some(thread_start_response_id),
			Some(thread_start_response_sha256),
		) = (
			codex_thread_id,
			thread_start_request_id,
			thread_start_request_sha256,
			thread_start_response_id,
			thread_start_response_sha256,
		)
		else {
			return Err(StoreError::Incompatible(
				"active RuntimeSession has incomplete thread-establishment facts".into(),
			));
		};
		if conversation_revision <= 0
			|| runtime_session_revision <= 0
			|| source_account_revision <= 0
			|| next_turn_sequence <= 0
			|| !has_acknowledged_turn
			|| conversation_status != "open"
			|| profile_role != "task"
			|| thread_start_request_id <= 0
			|| thread_start_response_id != thread_start_request_id
			|| model.is_empty()
			|| model.len() > 128
			|| reasoning_effort.is_empty()
			|| reasoning_effort.len() > 32
			|| instructions.len() > 65_536
		{
			return Err(StoreError::Incompatible(
				"ordinary RuntimeSession resume projection is invalid".into(),
			));
		}
		validate_uuid(&codex_thread_id, "Codex thread identity")?;
		validate_stored_sha256(&thread_start_request_sha256)?;
		validate_stored_sha256(&thread_start_response_sha256)?;
		crate::ensure_credential_negative_text(&model)?;
		crate::ensure_credential_negative_text(&reasoning_effort)?;
		crate::ensure_credential_negative_text(&instructions)?;

		Ok(Some(OrdinaryRuntimeSessionResumeReadback {
			conversation_id: conversation_id.clone(),
			conversation_revision,
			runtime_session_id,
			runtime_session_revision,
			codex_thread_id,
			model,
			reasoning_effort,
			instructions,
			source_account_id,
			source_account_revision,
			next_turn_sequence,
			thread_start_request_id,
			thread_start_request_sha256,
			thread_start_response_id,
			thread_start_response_sha256,
			has_acknowledged_turn,
			has_active_turn,
			has_unresolved_provider_attempt,
		}))
	}
}

fn parse_quick_task_process_generation_response(
	response: &[u8],
	request: &PrepareQuickTaskProcessGeneration,
) -> Result<(String, QuickTaskProcessGenerationReadback), StoreError> {
	let document: Value = serde_json::from_slice(response).map_err(|_| {
		StoreError::Incompatible("Quick Task ProcessGeneration response is invalid".into())
	})?;
	let classification = document
		.get("classification")
		.and_then(Value::as_str)
		.ok_or_else(|| {
			StoreError::Incompatible("Quick Task ProcessGeneration classification is absent".into())
		})?
		.to_owned();
	let effect = required_value(&document, "effect")?;
	if required_str(effect, "operation")? != "prepare_quick_task_process_generation"
		|| required_str(effect, "conversation_id")? != request.conversation_id.as_str()
		|| required_i64(effect, "conversation_revision")? != request.expected_conversation_revision
		|| required_str(effect, "runtime_session_id")? != request.runtime_session_id.as_str()
		|| required_i64(effect, "runtime_session_revision")?
			!= request.expected_runtime_session_revision
		|| required_str(effect, "turn_id")? != request.turn_id.as_str()
		|| required_i64(effect, "turn_revision")? != request.expected_turn_revision
		|| required_str(effect, "continuation_plan_id")? != request.continuation_plan_id
		|| required_str(effect, "routing_decision_id")? != request.routing_decision_id
		|| required_str(effect, "selected_account_id")? != request.selected_account_id.as_str()
		|| required_str(effect, "process_generation_id")? != request.process_generation_id.as_str()
	{
		return Err(StoreError::Incompatible(
			"Quick Task ProcessGeneration response is cross-linked".into(),
		));
	}
	let admission_revision = optional_positive_i64(effect, "admission_revision")?;
	let turn_state = required_str(effect, "turn_state")?;
	let rejection = match classification.as_str() {
		"success" if turn_state == "active" && admission_revision.is_some() => None,
		"stable_domain_rejection" if admission_revision.is_none() =>
			Some(match required_str(effect, "rejection")? {
				"missing_turn" => QuickTaskProcessGenerationRejection::MissingTurn,
				"inactive_turn" => QuickTaskProcessGenerationRejection::InactiveTurn,
				"stale_turn" => QuickTaskProcessGenerationRejection::StaleTurn,
				"authority_unavailable" =>
					QuickTaskProcessGenerationRejection::AuthorityUnavailable,
				"invalid_input" => QuickTaskProcessGenerationRejection::InvalidInput,
				_ => {
					return Err(StoreError::Incompatible(
						"Quick Task ProcessGeneration rejection is invalid".into(),
					));
				},
			}),
		"effect_or_persistence_unknown"
			if admission_revision.is_none() && optional_str(effect, "rejection")?.is_none() =>
			None,
		_ => {
			return Err(StoreError::Incompatible(
				"Quick Task ProcessGeneration response has invalid revision authority".into(),
			));
		},
	};
	if classification != "stable_domain_rejection" && rejection.is_some() {
		return Err(StoreError::Incompatible(
			"Quick Task ProcessGeneration response carries invalid rejection authority".into(),
		));
	}
	validate_exact_effect_digest(effect)?;
	Ok((
		classification,
		QuickTaskProcessGenerationReadback {
			request: request.clone(),
			admission_revision,
			rejection,
		},
	))
}

fn parse_quick_task_thread_establishment_readback(
	value: &Value,
	request: &ReconcileQuickTaskThreadEstablishment,
) -> Result<QuickTaskThreadEstablishmentReadback, StoreError> {
	let disposition = required_str(value, "disposition")?;
	let turn_state = required_str(value, "turn_state")?;
	if required_str(value, "conversation_id")? != request.conversation_id.as_str()
		|| required_i64(value, "conversation_revision")? != request.expected_conversation_revision
		|| required_str(value, "runtime_session_id")? != request.runtime_session_id.as_str()
		|| required_i64(value, "baseline_runtime_session_revision")?
			!= request.expected_runtime_session_revision
		|| required_str(value, "turn_id")? != request.turn_id.as_str()
		|| required_i64(value, "turn_revision")? != request.expected_turn_revision
		|| required_str(value, "continuation_plan_id")? != request.continuation_plan_id
		|| required_str(value, "routing_decision_id")? != request.routing_decision_id
		|| required_str(value, "selected_account_id")? != request.selected_account_id.as_str()
		|| required_str(value, "process_generation_id")? != request.process_generation_id.as_str()
	{
		return Err(StoreError::Incompatible(
			"Quick Task thread-establishment readback is cross-linked".into(),
		));
	}

	match disposition {
		"bound" if turn_state == "active" => Ok(QuickTaskThreadEstablishmentReadback::Bound(
			parse_reconciled_thread_binding(value, request)?,
		)),
		"fenced" if turn_state == "active" => Ok(QuickTaskThreadEstablishmentReadback::Fenced(
			parse_reconciled_thread_fence(value, request)?,
		)),
		"definitely_not_started" => {
			let evidence_id = required_uuid(value, "non_effect_evidence_id")?;
			let kind = match required_str(value, "non_effect_kind")? {
				"admission_rejected" => QuickTaskPreEffectEvidenceKind::AdmissionRejected,
				"spawn_not_created" => QuickTaskPreEffectEvidenceKind::SpawnNotCreated,
				"process_dead" => QuickTaskPreEffectEvidenceKind::ProcessDead,
				_ => {
					return Err(StoreError::Incompatible(
						"Quick Task pre-effect evidence kind is invalid".into(),
					));
				},
			};
			let process_generation_revision =
				optional_positive_i64(value, "process_generation_revision")?;
			if (kind == QuickTaskPreEffectEvidenceKind::AdmissionRejected)
				== process_generation_revision.is_some()
			{
				return Err(StoreError::Incompatible(
					"Quick Task pre-effect evidence revision is invalid".into(),
				));
			}
			if kind != QuickTaskPreEffectEvidenceKind::AdmissionRejected && turn_state != "active" {
				return Err(StoreError::Incompatible(
					"Quick Task process evidence lost the selected active Turn".into(),
				));
			}
			Ok(QuickTaskThreadEstablishmentReadback::DefinitelyNotStarted(
				QuickTaskThreadStartNonEffect { process_generation_revision, kind, evidence_id },
			))
		},
		"unknown" => Ok(QuickTaskThreadEstablishmentReadback::Unknown),
		_ => Err(StoreError::Incompatible(
			"Quick Task thread-establishment disposition is invalid".into(),
		)),
	}
}

fn parse_reconciled_thread_fence(
	value: &Value,
	request: &ReconcileQuickTaskThreadEstablishment,
) -> Result<RuntimeSessionThreadFenceReadback, StoreError> {
	let revision = positive_i64(value, "fence_revision")?;
	if request.expected_runtime_session_revision.checked_add(1) != Some(revision) {
		return Err(StoreError::Incompatible(
			"reconciled RuntimeSession fence revision is invalid".into(),
		));
	}
	let request_sha256 = required_str(value, "thread_start_request_sha256")?.to_owned();
	validate_stored_sha256(&request_sha256)?;
	Ok(RuntimeSessionThreadFenceReadback {
		fence_idempotency_key: required_str(value, "fence_idempotency_key")?.to_owned(),
		conversation_id: request.conversation_id.clone(),
		conversation_revision: request.expected_conversation_revision,
		runtime_session_id: request.runtime_session_id.clone(),
		prior_revision: request.expected_runtime_session_revision,
		revision,
		turn_id: request.turn_id.clone(),
		turn_revision: request.expected_turn_revision,
		continuation_plan_id: request.continuation_plan_id.clone(),
		routing_decision_id: required_uuid(value, "routing_decision_id")?,
		selected_account_id: AccountId::new(required_str(value, "selected_account_id")?)
			.map_err(|_| StoreError::Incompatible("stored selected account is invalid".into()))?,
		process_generation_id: request.process_generation_id.clone(),
		process_generation_revision: positive_i64(value, "process_generation_revision")?,
		process_execution_epoch_id: ProcessExecutionEpochId::new(required_uuid(
			value,
			"process_execution_epoch_id",
		)?)
		.map_err(|_| {
			StoreError::Incompatible("stored process execution epoch is invalid".into())
		})?,
		thread_start_request_id: positive_i64(value, "thread_start_request_id")?,
		thread_start_request_sha256: request_sha256,
		activity_sequence: positive_i64(value, "fence_activity_sequence")?,
		outbox_id: positive_i64(value, "fence_outbox_id")?,
	})
}

fn parse_reconciled_thread_binding(
	value: &Value,
	request: &ReconcileQuickTaskThreadEstablishment,
) -> Result<RuntimeSessionThreadBindingReadback, StoreError> {
	let fence = parse_reconciled_thread_fence(value, request)?;
	let revision = positive_i64(value, "runtime_session_revision")?;
	let fence_revision = positive_i64(value, "fence_revision")?;
	let response_sha256 = required_str(value, "thread_start_response_sha256")?.to_owned();
	validate_stored_sha256(&response_sha256)?;
	let response_id = positive_i64(value, "thread_start_response_id")?;
	if request.expected_runtime_session_revision.checked_add(1) != Some(fence_revision)
		|| fence_revision.checked_add(1) != Some(revision)
		|| response_id != fence.thread_start_request_id
	{
		return Err(StoreError::Incompatible(
			"reconciled RuntimeSession binding revision is invalid".into(),
		));
	}
	let codex_thread_id = required_uuid(value, "codex_thread_id")?;
	Ok(RuntimeSessionThreadBindingReadback {
		conversation_id: request.conversation_id.clone(),
		conversation_revision: request.expected_conversation_revision,
		runtime_session_id: request.runtime_session_id.clone(),
		prior_revision: fence_revision,
		revision,
		turn_id: request.turn_id.clone(),
		turn_revision: request.expected_turn_revision,
		fence_prior_revision: request.expected_runtime_session_revision,
		fence_revision,
		continuation_plan_id: request.continuation_plan_id.clone(),
		fence_idempotency_key: fence.fence_idempotency_key,
		binding_idempotency_key: required_str(value, "binding_idempotency_key")?.to_owned(),
		thread_start_request_id: fence.thread_start_request_id,
		thread_start_request_sha256: fence.thread_start_request_sha256,
		thread_start_response_id: response_id,
		thread_start_response_sha256: response_sha256,
		codex_thread_id,
		activity_sequence: positive_i64(value, "binding_activity_sequence")?,
		outbox_id: positive_i64(value, "binding_outbox_id")?,
	})
}

fn parse_turn_acknowledgement_response(
	response: &[u8],
	acknowledgement: &AcknowledgeRuntimeSessionTurn,
) -> Result<RuntimeSessionTurnAcknowledgementReadback, StoreError> {
	let document = parse_thread_establishment_success(response)?;
	let effect = required_value(&document, "effect")?;
	let prior_revision = positive_i64(effect, "prior_revision")?;
	let revision = positive_i64(effect, "revision")?;
	let user_turn_revision = positive_i64(effect, "user_turn_revision")?;
	let assistant_turn_id =
		optional_str(effect, "assistant_turn_id")?.map(TurnId::new).transpose().map_err(|_| {
			StoreError::Incompatible("stored assistant Turn identity is invalid".into())
		})?;
	let assistant_turn_revision = optional_positive_i64(effect, "assistant_turn_revision")?;
	let provider_attempt_revision = positive_i64(effect, "provider_attempt_revision")?;
	let provider_evidence_id =
		ProviderEvidenceId::new(required_str(effect, "provider_evidence_id")?).map_err(|_| {
			StoreError::Incompatible("stored provider evidence identity is invalid".into())
		})?;
	let activity_sequence = positive_i64(effect, "activity_sequence")?;
	let outbox_id = positive_i64(effect, "outbox_id")?;
	let expected_assistant =
		acknowledgement.assistant_turn.as_ref().map(|(turn_id, revision)| (turn_id, *revision));
	let actual_assistant = assistant_turn_id.as_ref().zip(assistant_turn_revision);
	let assistant_revision_shape = matches!(
		(assistant_turn_id.as_ref(), assistant_turn_revision),
		(None, None) | (Some(_), Some(_))
	);
	if required_str(effect, "operation")? != "acknowledge_runtime_session_turn"
		|| required_str(effect, "kind")? != "runtime_session_turn_acknowledged"
		|| required_str(effect, "conversation_id")? != acknowledgement.conversation_id.as_str()
		|| required_i64(effect, "conversation_revision")?
			!= acknowledgement.expected_conversation_revision
		|| required_str(effect, "runtime_session_id")?
			!= acknowledgement.runtime_session_id.as_str()
		|| required_str(effect, "user_turn_id")? != acknowledgement.user_turn_id.as_str()
		|| required_str(effect, "provider_attempt_id")?
			!= acknowledgement.provider_attempt_id.as_str()
		|| required_str(effect, "provider_evidence_id")?
			!= acknowledgement.provider_evidence_id.as_str()
		|| required_str(effect, "provider_attempt_outcome")?
			!= provider_terminal_outcome_sql(acknowledgement.provider_outcome)
		|| required_str(effect, "turn_status")?
			!= provider_terminal_turn_status_sql(acknowledgement.provider_outcome)
		|| required_str(effect, "provider_thread_id")? != acknowledgement.provider_thread_id
		|| required_str(effect, "codex_thread_id")? != acknowledgement.provider_thread_id
		|| required_str(effect, "provider_turn_id")? != acknowledgement.provider_turn_id
		|| required_str(effect, "last_known_turn_id")? != acknowledgement.provider_turn_id
		|| prior_revision != acknowledgement.expected_runtime_session_revision
		|| prior_revision.checked_add(1) != Some(revision)
		|| user_turn_revision != acknowledgement.expected_user_turn_revision
		|| provider_attempt_revision != acknowledgement.expected_provider_attempt_revision
		|| provider_evidence_id != acknowledgement.provider_evidence_id
		|| actual_assistant != expected_assistant
		|| !assistant_revision_shape
	{
		return Err(StoreError::Incompatible(
			"RuntimeSession turn acknowledgement readback does not match its request".into(),
		));
	}
	validate_exact_effect_digest(effect)?;

	Ok(RuntimeSessionTurnAcknowledgementReadback {
		runtime_session_id: acknowledgement.runtime_session_id.clone(),
		prior_revision,
		revision,
		user_turn_id: acknowledgement.user_turn_id.clone(),
		user_turn_revision,
		assistant_turn_id,
		assistant_turn_revision,
		provider_attempt_revision,
		provider_evidence_id,
		provider_thread_id: acknowledgement.provider_thread_id.clone(),
		provider_turn_id: acknowledgement.provider_turn_id.clone(),
		activity_sequence,
		outbox_id,
	})
}

fn parse_thread_fence_response(
	response: &[u8],
	idempotency_key: &str,
	fence: &FenceRuntimeSessionThreadStart,
) -> Result<RuntimeSessionThreadFenceReadback, StoreError> {
	let document = parse_thread_establishment_success(response)?;
	let effect = required_value(&document, "effect")?;
	let revision = positive_i64(effect, "revision")?;
	let prior_revision = positive_i64(effect, "prior_revision")?;
	let activity_sequence = positive_i64(effect, "activity_sequence")?;
	let outbox_id = positive_i64(effect, "outbox_id")?;
	let routing_decision_id = required_uuid(effect, "routing_decision_id")?;
	let selected_account_id = AccountId::new(required_str(effect, "selected_account_id")?)
		.map_err(|_| {
			StoreError::Incompatible("stored selected account identity is invalid".into())
		})?;

	if required_str(effect, "operation")? != "fence_runtime_session_thread_start"
		|| required_str(effect, "kind")? != "runtime_session_thread_start_fenced"
		|| required_str(effect, "prior_state")? != "starting"
		|| required_str(effect, "state")? != "starting"
		|| required_str(effect, "conversation_id")? != fence.conversation_id.as_str()
		|| required_i64(effect, "conversation_revision")? != fence.expected_conversation_revision
		|| required_str(effect, "runtime_session_id")? != fence.runtime_session_id.as_str()
		|| required_str(effect, "turn_id")? != fence.turn_id.as_str()
		|| required_i64(effect, "turn_revision")? != fence.expected_turn_revision
		|| required_str(effect, "turn_state")? != "active"
		|| required_str(effect, "continuation_plan_id")? != fence.continuation_plan_id
		|| required_str(effect, "process_generation_id")? != fence.process_generation_id.as_str()
		|| required_i64(effect, "process_generation_revision")? != fence.process_generation_revision
		|| required_str(effect, "process_execution_epoch_id")?
			!= fence.process_execution_epoch_id.as_str()
		|| required_i64(effect, "thread_start_request_id")? != fence.thread_start_request_id
		|| required_str(effect, "thread_start_request_sha256")? != fence.thread_start_request_sha256
		|| prior_revision != fence.expected_revision
		|| fence.expected_revision.checked_add(1) != Some(revision)
	{
		return Err(StoreError::Incompatible(
			"RuntimeSession thread fence readback does not match its request".into(),
		));
	}
	validate_stored_sha256(required_str(effect, "thread_start_request_sha256")?)?;
	validate_exact_effect_digest(effect)?;

	Ok(RuntimeSessionThreadFenceReadback {
		fence_idempotency_key: idempotency_key.to_owned(),
		conversation_id: fence.conversation_id.clone(),
		conversation_revision: fence.expected_conversation_revision,
		runtime_session_id: fence.runtime_session_id.clone(),
		prior_revision,
		revision,
		turn_id: fence.turn_id.clone(),
		turn_revision: fence.expected_turn_revision,
		continuation_plan_id: fence.continuation_plan_id.clone(),
		routing_decision_id,
		selected_account_id,
		process_generation_id: fence.process_generation_id.clone(),
		process_generation_revision: fence.process_generation_revision,
		process_execution_epoch_id: fence.process_execution_epoch_id.clone(),
		thread_start_request_id: fence.thread_start_request_id,
		thread_start_request_sha256: fence.thread_start_request_sha256.clone(),
		activity_sequence,
		outbox_id,
	})
}

fn parse_thread_binding_response(
	response: &[u8],
	idempotency_key: &str,
	binding: &BindRuntimeSessionThread,
) -> Result<RuntimeSessionThreadBindingReadback, StoreError> {
	let document = parse_thread_establishment_success(response)?;
	let effect = required_value(&document, "effect")?;
	let revision = positive_i64(effect, "revision")?;
	let prior_revision = positive_i64(effect, "prior_revision")?;
	let fence_prior_revision = positive_i64(effect, "fence_prior_revision")?;
	let fence_revision = positive_i64(effect, "fence_revision")?;
	let activity_sequence = positive_i64(effect, "activity_sequence")?;
	let outbox_id = positive_i64(effect, "outbox_id")?;

	if required_str(effect, "operation")? != "bind_runtime_session_thread"
		|| required_str(effect, "kind")? != "runtime_session_thread_bound"
		|| required_str(effect, "prior_state")? != "starting"
		|| required_str(effect, "state")? != "active"
		|| required_str(effect, "conversation_id")? != binding.conversation_id.as_str()
		|| required_i64(effect, "conversation_revision")? != binding.expected_conversation_revision
		|| required_str(effect, "runtime_session_id")? != binding.runtime_session_id.as_str()
		|| required_str(effect, "turn_id")? != binding.turn_id.as_str()
		|| required_i64(effect, "turn_revision")? != binding.expected_turn_revision
		|| required_str(effect, "turn_state")? != "active"
		|| required_str(effect, "continuation_plan_id")? != binding.continuation_plan_id
		|| required_str(effect, "fence_protocol")? != EXACT_COMMAND_PROTOCOL
		|| required_str(effect, "fence_idempotency_key")? != binding.fence_idempotency_key
		|| required_i64(effect, "thread_start_request_id")? != binding.thread_start_request_id
		|| required_str(effect, "thread_start_request_sha256")?
			!= binding.thread_start_request_sha256
		|| required_i64(effect, "thread_start_response_id")?
			!= binding.successful_response.response_id
		|| required_str(effect, "thread_start_response_sha256")?
			!= binding.successful_response.response_sha256
		|| required_str(effect, "codex_thread_id")? != binding.successful_response.codex_thread_id
		|| prior_revision != binding.expected_revision
		|| fence_prior_revision.checked_add(1) != Some(fence_revision)
		|| fence_revision != binding.expected_revision
		|| binding.expected_revision.checked_add(1) != Some(revision)
	{
		return Err(StoreError::Incompatible(
			"RuntimeSession thread binding readback does not match its request".into(),
		));
	}
	validate_stored_sha256(required_str(effect, "thread_start_request_sha256")?)?;
	validate_stored_sha256(required_str(effect, "thread_start_response_sha256")?)?;
	if !is_uuid(required_str(effect, "codex_thread_id")?) {
		return Err(StoreError::Incompatible(
			"stored RuntimeSession Codex thread identity is invalid".into(),
		));
	}
	validate_exact_effect_digest(effect)?;

	Ok(RuntimeSessionThreadBindingReadback {
		conversation_id: binding.conversation_id.clone(),
		conversation_revision: binding.expected_conversation_revision,
		runtime_session_id: binding.runtime_session_id.clone(),
		prior_revision,
		revision,
		turn_id: binding.turn_id.clone(),
		turn_revision: binding.expected_turn_revision,
		fence_prior_revision,
		fence_revision,
		continuation_plan_id: binding.continuation_plan_id.clone(),
		fence_idempotency_key: binding.fence_idempotency_key.clone(),
		binding_idempotency_key: idempotency_key.to_owned(),
		thread_start_request_id: binding.thread_start_request_id,
		thread_start_request_sha256: binding.thread_start_request_sha256.clone(),
		thread_start_response_id: binding.successful_response.response_id,
		thread_start_response_sha256: binding.successful_response.response_sha256.clone(),
		codex_thread_id: binding.successful_response.codex_thread_id.clone(),
		activity_sequence,
		outbox_id,
	})
}

fn parse_thread_establishment_rejection(
	response: &[u8],
	expected_operation: &str,
) -> Result<Option<RuntimeSessionThreadEstablishmentRejection>, StoreError> {
	let document: Value = serde_json::from_slice(response).map_err(|_| {
		StoreError::Incompatible("exact RuntimeSession thread response bytes are invalid".into())
	})?;
	match document.get("classification").and_then(Value::as_str) {
		Some("success") => return Ok(None),
		Some("stable_domain_rejection") => {},
		_ => {
			return Err(StoreError::Incompatible(
				"exact RuntimeSession thread response classification is invalid".into(),
			));
		},
	}
	let effect = required_value(&document, "effect")?;
	validate_exact_effect_digest(effect)?;
	let request = required_value(effect, "request")?;
	if required_str(effect, "operation")? != expected_operation
		|| effect.get("changed").and_then(Value::as_bool) != Some(false)
		|| required_str(request, "operation")? != expected_operation
		|| required_str(request, "protocol_version")? != EXACT_COMMAND_PROTOCOL
	{
		return Err(StoreError::Incompatible(
			"exact RuntimeSession thread rejection is cross-linked".into(),
		));
	}
	match required_str(effect, "rejection")? {
		"invalid_input" => Ok(Some(RuntimeSessionThreadEstablishmentRejection::InvalidInput)),
		"authority_unavailable" =>
			Ok(Some(RuntimeSessionThreadEstablishmentRejection::AuthorityUnavailable)),
		_ => Err(StoreError::Incompatible(
			"exact RuntimeSession thread rejection code is invalid".into(),
		)),
	}
}

fn parse_thread_establishment_success(response: &[u8]) -> Result<Value, StoreError> {
	let document: Value = serde_json::from_slice(response).map_err(|_| {
		StoreError::Incompatible("exact RuntimeSession thread response bytes are invalid".into())
	})?;
	if document.get("classification").and_then(Value::as_str) != Some("success")
		|| !matches!(document.get("effect"), Some(Value::Object(_)))
	{
		return Err(StoreError::Incompatible(
			"exact RuntimeSession thread response classification is invalid".into(),
		));
	}
	Ok(document)
}

fn validate_thread_establishment_revision(revision: i64) -> Result<(), StoreError> {
	if revision > 0 {
		Ok(())
	} else {
		Err(StoreError::InvalidInput("RuntimeSession thread revision must be positive"))
	}
}

fn validate_active_turn_revision(revision: i64) -> Result<(), StoreError> {
	if revision == 1 {
		Ok(())
	} else {
		Err(StoreError::InvalidInput(
			"RuntimeSession Thread Establishment requires active Turn revision 1",
		))
	}
}

fn validate_thread_start_id(id: i64) -> Result<(), StoreError> {
	if id > 0 {
		Ok(())
	} else {
		Err(StoreError::InvalidInput("thread/start identity must be positive"))
	}
}

fn validate_sha256(value: &str) -> Result<(), StoreError> {
	if is_lower_sha256(value) {
		Ok(())
	} else {
		Err(StoreError::InvalidInput("thread/start digest must be lowercase SHA-256"))
	}
}

fn validate_stored_sha256(value: &str) -> Result<(), StoreError> {
	if is_lower_sha256(value) {
		Ok(())
	} else {
		Err(StoreError::Incompatible("stored RuntimeSession thread digest is invalid".into()))
	}
}

fn is_lower_sha256(value: &str) -> bool {
	value.len() == 64
		&& value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

enum ResponseContext<'a> {
	Create(&'a CreateRuntimeSession),
	Transition {
		session_id: &'a RuntimeSessionId,
		expected_revision: i64,
		target_state: RuntimeSessionState,
	},
}

fn parse_create_response(
	response: &[u8],
	create: &CreateRuntimeSession,
) -> Result<RuntimeSessionCommandOutcome<RuntimeSessionCommandEffect>, StoreError> {
	parse_response(response, &ResponseContext::Create(create))
}

fn parse_transition_response(
	response: &[u8],
	session_id: &RuntimeSessionId,
	expected_revision: i64,
	target_state: RuntimeSessionState,
) -> Result<RuntimeSessionCommandOutcome<RuntimeSessionCommandEffect>, StoreError> {
	parse_response(
		response,
		&ResponseContext::Transition { session_id, expected_revision, target_state },
	)
}

fn parse_response(
	response: &[u8],
	context: &ResponseContext<'_>,
) -> Result<RuntimeSessionCommandOutcome<RuntimeSessionCommandEffect>, StoreError> {
	let document: Value = serde_json::from_slice(response).map_err(|_| {
		StoreError::Incompatible("exact RuntimeSession response bytes are invalid".into())
	})?;

	match document.get("classification").and_then(Value::as_str) {
		Some("stable_domain_rejection") => {
			validate_request_context(required_pointer(&document, "/effect/request")?, context)?;
			return rejection_from_document(&document).map(RuntimeSessionCommandOutcome::Rejected);
		},
		Some("success") => {},
		_ => {
			return Err(StoreError::Incompatible(
				"exact RuntimeSession response classification is invalid".into(),
			));
		},
	}

	let effect = required_pointer(&document, "/effect")?;
	validate_request_context(required_value(effect, "request")?, context)?;
	let session = required_value(effect, "runtime_session_snapshot")?;
	let profile = profile_from_value(required_value(effect, "profile_snapshot")?)?;
	let account = account_from_value(required_value(effect, "account_snapshot")?)?;
	let state = session_state_from_sql(required_str(session, "state")?)?;
	let revision = required_i64(session, "revision")?;
	let new_state = session_state_from_sql(required_str(effect, "new_state")?)?;
	let new_revision = positive_i64(effect, "new_revision")?;
	let prior_state = optional_state(effect, "prior_state")?;
	let prior_revision = optional_positive_i64(effect, "prior_revision")?;
	let profile_snapshot_id = required_str(session, "profile_snapshot_id")?;
	let account_snapshot_id = required_str(session, "account_snapshot_id")?;
	if revision < 1
		|| new_revision != revision
		|| new_state != state
		|| profile_snapshot_id != profile.profile_snapshot_id
		|| account_snapshot_id != account.account_snapshot_id
		|| match (prior_state, prior_revision, revision) {
			(None, None, 1) => false,
			(Some(_), Some(prior), current) => prior.checked_add(1) != Some(current),
			_ => true,
		} {
		return Err(StoreError::Incompatible(
			"exact RuntimeSession response effect is inconsistent".into(),
		));
	}

	let runtime_session = StoredRuntimeSession {
		runtime_session_id: RuntimeSessionId::new(required_str(session, "runtime_session_id")?)
			.map_err(|_| {
				StoreError::Incompatible("stored RuntimeSession identity is invalid".into())
			})?,
		conversation_id: ConversationId::new(required_str(session, "conversation_id")?).map_err(
			|_| StoreError::Incompatible("stored Conversation identity is invalid".into()),
		)?,
		profile_snapshot: profile,
		account_snapshot: account,
		codex_thread_id: optional_uuid(session, "codex_thread_id")?,
		last_known_turn_id: optional_str(session, "last_known_turn_id")?,
		state,
		revision,
		created_at: required_str(session, "created_at")?.to_owned(),
		updated_at: required_str(session, "updated_at")?.to_owned(),
		ended_at: optional_str(session, "ended_at")?,
	};
	validate_effect_context(&runtime_session, prior_state, prior_revision, context)?;
	let activity_sequence = positive_i64(effect, "activity_sequence")?;
	let outbox_id = positive_i64(effect, "outbox_id")?;
	let activity_payload = required_value(effect, "activity_payload")?.clone();
	let outbox_payload = required_value(effect, "outbox_payload")?.clone();
	let expected_event_kind = match context {
		ResponseContext::Create(_) => "runtime_session_created",
		ResponseContext::Transition { .. } => "runtime_session_transitioned",
	};
	let session_id = runtime_session.runtime_session_id.as_str();
	let activity_transition_matches = match context {
		ResponseContext::Create(_) => true,
		ResponseContext::Transition { expected_revision, target_state, .. } =>
			activity_payload.get("prior_state").and_then(Value::as_str)
				== prior_state.map(session_state_sql)
				&& activity_payload.get("new_state").and_then(Value::as_str)
					== Some(session_state_sql(*target_state))
				&& activity_payload.get("prior_revision").and_then(Value::as_i64)
					== Some(*expected_revision)
				&& activity_payload.get("new_revision").and_then(Value::as_i64)
					== Some(new_revision),
	};
	if !activity_transition_matches
		|| required_str(effect, "activity_aggregate_kind")? != "runtime_session"
		|| required_str(effect, "activity_aggregate_id")? != session_id
		|| required_i64(effect, "activity_revision")? != new_revision
		|| required_str(effect, "activity_event_kind")? != expected_event_kind
		|| required_str(effect, "outbox_effect_key")? != format!("activity/{activity_sequence}")
		|| required_str(effect, "outbox_aggregate_kind")? != "runtime_session"
		|| required_str(effect, "outbox_aggregate_id")? != session_id
		|| required_i64(effect, "outbox_aggregate_revision")? != new_revision
		|| activity_payload.get("runtime_session_snapshot") != Some(session)
		|| activity_payload.get("profile_snapshot") != effect.get("profile_snapshot")
		|| activity_payload.get("account_snapshot") != effect.get("account_snapshot")
		|| activity_payload.get("kind").and_then(Value::as_str) != Some("runtime_session")
		|| outbox_payload.get("activity_sequence").and_then(Value::as_i64)
			!= Some(activity_sequence)
		|| outbox_payload.get("payload") != Some(&activity_payload)
		|| outbox_payload.get("event_kind").and_then(Value::as_str) != Some(expected_event_kind)
		|| outbox_payload.get("aggregate_kind").and_then(Value::as_str) != Some("runtime_session")
		|| outbox_payload.get("aggregate_id").and_then(Value::as_str) != Some(session_id)
		|| outbox_payload.get("revision").and_then(Value::as_i64) != Some(new_revision)
	{
		return Err(StoreError::Incompatible(
			"exact RuntimeSession audit effect is inconsistent".into(),
		));
	}

	Ok(RuntimeSessionCommandOutcome::Success(RuntimeSessionCommandEffect {
		runtime_session,
		prior_state,
		new_state,
		prior_revision,
		new_revision,
		activity_sequence,
		activity_payload,
		outbox_id,
		outbox_payload,
	}))
}

fn validate_request_context(
	request: &Value,
	context: &ResponseContext<'_>,
) -> Result<(), StoreError> {
	let expected = match context {
		ResponseContext::Create(create) => serde_json::json!({
			"protocol_version": EXACT_COMMAND_PROTOCOL,
			"operation": "create_runtime_session",
			"runtime_session_id": create.runtime_session_id.as_str(),
			"conversation_id": create.conversation_id.as_str(),
			"role": create.role.as_sql(),
			"account_snapshot_id": create.account_snapshot.account_snapshot_id,
			"source_account_id": create.account_snapshot.source_account_id.as_str(),
			"display_label": create.account_snapshot.display_label,
			"observed_state": account_state_sql(create.account_snapshot.observed_state),
			"account_source_revision": create.account_snapshot.source_revision,
			"codex_thread_id": create.codex_thread_id,
			"initial_state": session_state_sql(create.initial_state),
		}),
		ResponseContext::Transition { session_id, expected_revision, target_state } =>
			serde_json::json!({
				"protocol_version": EXACT_COMMAND_PROTOCOL,
				"operation": "transition_runtime_session",
				"runtime_session_id": session_id.as_str(),
				"expected_revision": expected_revision,
				"target_state": session_state_sql(*target_state),
			}),
	};
	if request == &expected {
		Ok(())
	} else {
		Err(StoreError::Incompatible(
			"exact RuntimeSession response belongs to a different request".into(),
		))
	}
}

fn validate_effect_context(
	session: &StoredRuntimeSession,
	prior_state: Option<RuntimeSessionState>,
	prior_revision: Option<i64>,
	context: &ResponseContext<'_>,
) -> Result<(), StoreError> {
	let valid = match context {
		ResponseContext::Create(create) =>
			session.runtime_session_id == create.runtime_session_id
				&& session.conversation_id == create.conversation_id
				&& session.profile_snapshot.role == create.role
				&& session.account_snapshot.account_snapshot_id
					== create.account_snapshot.account_snapshot_id
				&& session.account_snapshot.source_account_id
					== create.account_snapshot.source_account_id
				&& session.account_snapshot.display_label == create.account_snapshot.display_label
				&& session.account_snapshot.observed_state == create.account_snapshot.observed_state
				&& session.account_snapshot.source_revision
					== create.account_snapshot.source_revision
				&& session.codex_thread_id == create.codex_thread_id
				&& session.last_known_turn_id.is_none()
				&& session.state == create.initial_state
				&& session.revision == 1
				&& prior_state.is_none()
				&& prior_revision.is_none(),
		ResponseContext::Transition { session_id, expected_revision, target_state } =>
			session.runtime_session_id.as_str() == session_id.as_str()
				&& session.state == *target_state
				&& prior_revision == Some(*expected_revision)
				&& session.revision == expected_revision.checked_add(1).unwrap_or(i64::MIN)
				&& matches!(
					(prior_state, target_state),
					(Some(RuntimeSessionState::Starting), RuntimeSessionState::Active)
						| (Some(RuntimeSessionState::Starting), RuntimeSessionState::Ended)
						| (Some(RuntimeSessionState::Starting), RuntimeSessionState::Diverged)
						| (Some(RuntimeSessionState::Active), RuntimeSessionState::Ended)
						| (Some(RuntimeSessionState::Active), RuntimeSessionState::Diverged)
				),
	};
	if valid {
		Ok(())
	} else {
		Err(StoreError::Incompatible(
			"exact RuntimeSession response does not match the command request".into(),
		))
	}
}

fn rejection_from_document(document: &Value) -> Result<RuntimeSessionRejection, StoreError> {
	let code = document.get("code").and_then(Value::as_str);
	let effect = required_pointer(document, "/effect")?;
	if effect.get("changed").and_then(Value::as_bool) != Some(false)
		|| effect.get("code").and_then(Value::as_str) != code
	{
		return Err(StoreError::Incompatible(
			"exact RuntimeSession rejection effect is inconsistent".into(),
		));
	}

	match code {
		Some("missing_target") => Ok(RuntimeSessionRejection::MissingTarget),
		Some("duplicate_target") => Ok(RuntimeSessionRejection::DuplicateTarget),
		Some("stale_revision") => Ok(RuntimeSessionRejection::StaleRevision),
		Some("illegal_transition") => Ok(RuntimeSessionRejection::IllegalTransition),
		Some("invalid_account_snapshot") => Ok(RuntimeSessionRejection::InvalidAccountSnapshot),
		Some("account_snapshot_conflict") => Ok(RuntimeSessionRejection::AccountSnapshotConflict),
		_ => Err(StoreError::Incompatible("exact RuntimeSession rejection code is invalid".into())),
	}
}

pub(crate) fn profile_from_value(
	value: &Value,
) -> Result<RuntimeSessionProfileSnapshot, StoreError> {
	let source_profile_id = required_str(value, "source_profile_id")?;
	let role = RoleProfileRole::from_sql(required_str(value, "role")?)?;
	if source_profile_id != role.as_sql() {
		return Err(StoreError::Incompatible(
			"stored RoleProfile snapshot identity is inconsistent".into(),
		));
	}

	Ok(RuntimeSessionProfileSnapshot {
		profile_snapshot_id: required_uuid(value, "profile_snapshot_id")?,
		role,
		source_revision: positive_i64(value, "source_revision")?,
		model: required_str(value, "model")?.to_owned(),
		reasoning_effort: required_str(value, "reasoning_effort")?.to_owned(),
		service_tier: required_str(value, "service_tier")?.to_owned(),
		instructions_digest: required_str(value, "instructions_digest")?.to_owned(),
		instructions: required_str(value, "instructions")?.to_owned(),
		provenance: optional_str(value, "provenance")?,
		created_at: required_str(value, "created_at")?.to_owned(),
	})
}

pub(crate) fn account_from_value(
	value: &Value,
) -> Result<RuntimeSessionAccountSnapshot, StoreError> {
	Ok(RuntimeSessionAccountSnapshot {
		account_snapshot_id: required_uuid(value, "account_snapshot_id")?,
		source_account_id: AccountId::new(required_str(value, "source_account_id")?)
			.map_err(|_| StoreError::Incompatible("stored account identity is invalid".into()))?,
		display_label: required_str(value, "display_label")?.to_owned(),
		observed_state: account_state_from_sql(required_str(value, "observed_state")?)?,
		source_revision: positive_i64(value, "source_revision")?,
		created_at: required_str(value, "created_at")?.to_owned(),
	})
}

fn required_pointer<'a>(value: &'a Value, pointer: &str) -> Result<&'a Value, StoreError> {
	value.pointer(pointer).ok_or_else(|| {
		StoreError::Incompatible("exact RuntimeSession response shape is incomplete".into())
	})
}

fn required_value<'a>(value: &'a Value, key: &str) -> Result<&'a Value, StoreError> {
	value
		.get(key)
		.ok_or_else(|| StoreError::Incompatible("exact RuntimeSession effect is incomplete".into()))
}

fn required_str<'a>(value: &'a Value, key: &str) -> Result<&'a str, StoreError> {
	value
		.get(key)
		.and_then(Value::as_str)
		.ok_or_else(|| StoreError::Incompatible("stored RuntimeSession text is invalid".into()))
}

fn required_i64(value: &Value, key: &str) -> Result<i64, StoreError> {
	value
		.get(key)
		.and_then(Value::as_i64)
		.ok_or_else(|| StoreError::Incompatible("stored RuntimeSession integer is invalid".into()))
}

fn positive_i64(value: &Value, key: &str) -> Result<i64, StoreError> {
	let result = required_i64(value, key)?;
	if result < 1 {
		return Err(StoreError::Incompatible("stored RuntimeSession revision is invalid".into()));
	}
	Ok(result)
}

fn optional_str(value: &Value, key: &str) -> Result<Option<String>, StoreError> {
	match value.get(key) {
		Some(Value::Null) => Ok(None),
		Some(Value::String(value)) => Ok(Some(value.clone())),
		_ => Err(StoreError::Incompatible("stored RuntimeSession optional text is invalid".into())),
	}
}

fn optional_uuid(value: &Value, key: &str) -> Result<Option<String>, StoreError> {
	match optional_str(value, key)? {
		Some(value) if is_uuid(&value) => Ok(Some(value)),
		Some(_) => Err(StoreError::Incompatible("stored RuntimeSession UUID is invalid".into())),
		None => Ok(None),
	}
}

fn optional_state(value: &Value, key: &str) -> Result<Option<RuntimeSessionState>, StoreError> {
	match value.get(key) {
		Some(Value::Null) => Ok(None),
		Some(Value::String(value)) => session_state_from_sql(value).map(Some),
		_ =>
			Err(StoreError::Incompatible("stored RuntimeSession optional state is invalid".into())),
	}
}

fn optional_positive_i64(value: &Value, key: &str) -> Result<Option<i64>, StoreError> {
	match value.get(key) {
		Some(Value::Null) => Ok(None),
		Some(Value::Number(value)) => {
			let value = value.as_i64().filter(|value| *value > 0).ok_or_else(|| {
				StoreError::Incompatible(
					"stored RuntimeSession optional revision is invalid".into(),
				)
			})?;
			Ok(Some(value))
		},
		_ => Err(StoreError::Incompatible(
			"stored RuntimeSession optional revision is invalid".into(),
		)),
	}
}

fn required_uuid(value: &Value, key: &str) -> Result<String, StoreError> {
	let value = required_str(value, key)?.to_owned();
	if is_uuid(&value) {
		Ok(value)
	} else {
		Err(StoreError::Incompatible("stored RuntimeSession UUID is invalid".into()))
	}
}

fn validate_uuid(value: &str, field: &'static str) -> Result<(), StoreError> {
	if is_uuid(value) { Ok(()) } else { Err(StoreError::InvalidInput(field)) }
}

fn is_uuid(value: &str) -> bool {
	value.len() == 36
		&& value.bytes().enumerate().all(|(index, byte)| match index {
			8 | 13 | 18 | 23 => byte == b'-',
			_ => byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte),
		})
}

const fn session_state_sql(value: RuntimeSessionState) -> &'static str {
	match value {
		RuntimeSessionState::Starting => "starting",
		RuntimeSessionState::Active => "active",
		RuntimeSessionState::Ended => "ended",
		RuntimeSessionState::Diverged => "diverged",
	}
}

pub(crate) fn session_state_from_sql(value: &str) -> Result<RuntimeSessionState, StoreError> {
	match value {
		"starting" => Ok(RuntimeSessionState::Starting),
		"active" => Ok(RuntimeSessionState::Active),
		"ended" => Ok(RuntimeSessionState::Ended),
		"diverged" => Ok(RuntimeSessionState::Diverged),
		_ => Err(StoreError::Incompatible("stored RuntimeSession state is invalid".into())),
	}
}

const fn provider_terminal_outcome_sql(value: ProviderTerminalOutcome) -> &'static str {
	match value {
		ProviderTerminalOutcome::Succeeded => "succeeded",
		ProviderTerminalOutcome::FailedDefinitive => "failed_definitive",
		ProviderTerminalOutcome::NotSubmitted => "not_submitted",
	}
}

const fn provider_terminal_turn_status_sql(value: ProviderTerminalOutcome) -> &'static str {
	match value {
		ProviderTerminalOutcome::Succeeded => "completed",
		ProviderTerminalOutcome::FailedDefinitive => "failed",
		ProviderTerminalOutcome::NotSubmitted => "failed",
	}
}

const fn account_state_sql(value: AccountState) -> &'static str {
	match value {
		AccountState::Unavailable => "unavailable",
		AccountState::Unknown => "unknown",
		AccountState::Available => "available",
		AccountState::Depleted => "depleted",
		AccountState::AuthFailed => "auth_failed",
		AccountState::PluginUnready => "plugin_unready",
	}
}

fn account_state_from_sql(value: &str) -> Result<AccountState, StoreError> {
	match value {
		"unavailable" => Ok(AccountState::Unavailable),
		"unknown" => Ok(AccountState::Unknown),
		"available" => Ok(AccountState::Available),
		"depleted" => Ok(AccountState::Depleted),
		"auth_failed" => Ok(AccountState::AuthFailed),
		"plugin_unready" => Ok(AccountState::PluginUnready),
		"disabled" => Err(StoreError::Incompatible(
			"administrative disabled state was not normalized by V27".into(),
		)),
		_ => Err(StoreError::Incompatible("stored account state is invalid".into())),
	}
}

#[cfg(test)]
mod tests {
	use super::{
		CreateRuntimeSession, CreateRuntimeSessionAccountSnapshot, RuntimeSessionRejection,
		parse_create_response, parse_transition_response,
	};
	use crate::{RoleProfileRole, RuntimeSessionCommandOutcome, StoreError};
	use decodex_core::{
		AccountId, AccountState, ConversationId, RuntimeSessionId, RuntimeSessionState,
	};
	use serde_json::json;

	fn create() -> CreateRuntimeSession {
		CreateRuntimeSession {
			runtime_session_id: RuntimeSessionId::new("41000000-0000-4000-8000-000000000001")
				.unwrap(),
			conversation_id: ConversationId::new("40000000-0000-4000-8000-000000000001").unwrap(),
			role: RoleProfileRole::Task,
			account_snapshot: CreateRuntimeSessionAccountSnapshot {
				account_snapshot_id: "43000000-0000-4000-8000-000000000001".into(),
				source_account_id: AccountId::new("13000000-0000-4000-8000-000000000001").unwrap(),
				display_label: "Account".into(),
				observed_state: AccountState::Unknown,
				source_revision: 1,
			},
			codex_thread_id: None,
			initial_state: RuntimeSessionState::Starting,
		}
	}

	fn create_request() -> serde_json::Value {
		json!({
			"protocol_version": "decodex/exact-command/1",
			"operation": "create_runtime_session",
			"runtime_session_id": "41000000-0000-4000-8000-000000000001",
			"conversation_id": "40000000-0000-4000-8000-000000000001",
			"role": "task",
			"account_snapshot_id": "43000000-0000-4000-8000-000000000001",
			"source_account_id": "13000000-0000-4000-8000-000000000001",
			"display_label": "Account",
			"observed_state": "unknown",
			"account_source_revision": 1,
			"codex_thread_id": null,
			"initial_state": "starting"
		})
	}

	#[test]
	fn stable_rejection_parser_is_closed() {
		let session_id = RuntimeSessionId::new("41000000-0000-4000-8000-000000000001").unwrap();
		let response = json!({
			"classification": "stable_domain_rejection",
			"code": "stale_revision",
			"effect": {
				"changed": false,
				"code": "stale_revision",
				"request": {
					"protocol_version": "decodex/exact-command/1",
					"operation": "transition_runtime_session",
					"runtime_session_id": session_id.as_str(),
					"expected_revision": 1,
					"target_state": "active"
				}
			}
		});
		assert_eq!(
			parse_transition_response(
				&serde_json::to_vec(&response).unwrap(),
				&session_id,
				1,
				RuntimeSessionState::Active,
			)
			.unwrap(),
			RuntimeSessionCommandOutcome::Rejected(RuntimeSessionRejection::StaleRevision),
		);
		assert!(
			parse_transition_response(
				&serde_json::to_vec(&response).unwrap(),
				&session_id,
				2,
				RuntimeSessionState::Active,
			)
			.is_err()
		);
	}

	#[test]
	fn success_parser_requires_complete_immutable_snapshots() {
		let incomplete =
			br#"{"classification":"success","effect":{"runtime_session_snapshot":{}}}"#;
		assert!(parse_create_response(incomplete, &create()).is_err());
	}

	#[test]
	fn success_parser_closes_snapshot_and_audit_cross_references() {
		let session = json!({
			"runtime_session_id": "41000000-0000-4000-8000-000000000001",
			"conversation_id": "40000000-0000-4000-8000-000000000001",
			"profile_snapshot_id": "42000000-0000-4000-8000-000000000001",
			"account_snapshot_id": "43000000-0000-4000-8000-000000000001",
			"codex_thread_id": null,
			"last_known_turn_id": null,
			"state": "starting",
			"revision": 1,
			"created_at": "2026-07-17T00:00:00Z",
			"updated_at": "2026-07-17T00:00:00Z",
			"ended_at": null
		});
		let profile = json!({
			"profile_snapshot_id": "42000000-0000-4000-8000-000000000001",
			"source_profile_id": "task",
			"role": "task",
			"source_revision": 1,
			"model": "gpt-5.6-sol",
			"reasoning_effort": "medium",
			"service_tier": "priority",
			"instructions_digest": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
			"instructions": "Own the task.",
			"provenance": null,
			"created_at": "2026-07-17T00:00:00Z"
		});
		let account = json!({
			"account_snapshot_id": "43000000-0000-4000-8000-000000000001",
			"source_account_id": "13000000-0000-4000-8000-000000000001",
			"display_label": "Account",
			"observed_state": "unknown",
			"source_revision": 1,
			"created_at": "2026-07-17T00:00:00Z"
		});
		let activity = json!({
			"kind": "runtime_session",
			"runtime_session_snapshot": session,
			"profile_snapshot": profile,
			"account_snapshot": account
		});
		let outbox = json!({
			"activity_sequence": 1,
			"event_kind": "runtime_session_created",
			"aggregate_kind": "runtime_session",
			"aggregate_id": "41000000-0000-4000-8000-000000000001",
			"revision": 1,
			"payload": activity
		});
		let response = json!({
			"classification": "success",
			"effect": {
				"request": create_request(),
				"runtime_session_snapshot": session,
				"profile_snapshot": profile,
				"account_snapshot": account,
				"prior_state": null,
				"new_state": "starting",
				"prior_revision": null,
				"new_revision": 1,
				"activity_sequence": 1,
				"activity_aggregate_kind": "runtime_session",
				"activity_aggregate_id": "41000000-0000-4000-8000-000000000001",
				"activity_revision": 1,
				"activity_event_kind": "runtime_session_created",
				"activity_payload": activity,
				"outbox_id": 1,
				"outbox_effect_key": "activity/1",
				"outbox_aggregate_kind": "runtime_session",
				"outbox_aggregate_id": "41000000-0000-4000-8000-000000000001",
				"outbox_aggregate_revision": 1,
				"outbox_payload": outbox
			}
		});
		let parsed =
			parse_create_response(&serde_json::to_vec(&response).unwrap(), &create()).unwrap();
		let RuntimeSessionCommandOutcome::Success(effect) = parsed else {
			panic!("golden response must parse")
		};
		assert_eq!(effect.new_state, RuntimeSessionState::Starting);
		assert_eq!(effect.activity_sequence, 1);

		let mut substituted = response;
		substituted["effect"]["runtime_session_snapshot"]["profile_snapshot_id"] =
			json!("42000000-0000-4000-8000-000000000099");
		assert!(
			parse_create_response(&serde_json::to_vec(&substituted).unwrap(), &create()).is_err()
		);
	}

	#[test]
	fn success_parser_rejects_wrong_request_and_malformed_stored_uuid_as_incompatible() {
		let incomplete = json!({
			"classification": "success",
			"effect": {"request": create_request(), "runtime_session_snapshot": {}}
		});
		let mut wrong = create();
		wrong.account_snapshot.display_label = "Other account".into();
		assert!(matches!(
			parse_create_response(&serde_json::to_vec(&incomplete).unwrap(), &wrong),
			Err(StoreError::Incompatible(_))
		));

		let malformed = json!({
			"profile_snapshot_id": "not-a-uuid",
			"source_profile_id": "task",
			"role": "task",
			"source_revision": 1,
			"model": "m",
			"reasoning_effort": "medium",
			"service_tier": "priority",
			"instructions_digest": "d",
			"instructions": "i",
			"provenance": null,
			"created_at": "t"
		});
		let malformed_response = json!({
			"classification": "success",
			"effect": {
				"request": create_request(),
				"runtime_session_snapshot": {},
				"profile_snapshot": malformed,
				"account_snapshot": {}
			}
		});
		assert!(matches!(
			parse_create_response(&serde_json::to_vec(&malformed_response).unwrap(), &create()),
			Err(StoreError::Incompatible(_))
		));
	}
}
