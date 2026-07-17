use serde_json::Value;

use crate::{
	PostgresStore, StoreError,
	exact_commands::{EXACT_COMMAND_PROTOCOL, validate_exact_key},
};
use decodex_core::{
	EffectId, ExecutionAssignment, ExecutionAssignmentRole, ManagedRunId, ManagedRunLifecycle,
	ManagedRunPhase, ManagedRunSafetyInput, ManagedRunState, ManagedRunWaitReason, ProjectId,
	RuntimeSessionId, RuntimeSessionState, SafetyObservationId, SubmittedTurnReceiptId, TurnId,
	WorkItemId,
};

/// One inert effect-lineage record owned by a ManagedRun.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManagedRunEffectLineage {
	/// Canonical effect identity.
	pub effect_id: EffectId,
	/// Stable typed effect category.
	pub kind: ManagedRunEffectKind,
	/// Exact run-local effect key.
	pub effect_key: String,
	/// Positive deterministic run-local ordinal.
	pub ordinal: i32,
}

/// Closed effect categories; none is an execution authorization.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManagedRunEffectKind {
	/// Tool-side effect lineage.
	Tool,
	/// Repository/filesystem effect lineage.
	Repository,
	/// Git effect lineage.
	Git,
	/// Artifact effect lineage.
	Artifact,
}

/// Persisted fail-closed barrier readback.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManagedRunEffectBarrier {
	/// Guarded and closed both deny effects in this slice.
	pub state: ManagedRunEffectBarrierState,
	/// Positive monotonic barrier revision.
	pub revision: i64,
	/// Exact input that closed the barrier, when permanently closed.
	pub closure_input_id: Option<String>,
	/// PostgreSQL-authored closure timestamp.
	pub closed_at: Option<String>,
}

/// Fail-closed barrier states. There is intentionally no open state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManagedRunEffectBarrierState {
	/// Initial inert state; no effect may execute.
	Guarded,
	/// Permanently closed by one supported safety input.
	Closed,
}

/// Exact revisioned restart readback for one inert ManagedRun.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredManagedRun {
	/// Canonical ManagedRun identity.
	pub managed_run_id: ManagedRunId,
	/// Exact owning Project.
	pub project_id: ProjectId,
	/// Exact canonical WorkItem.
	pub work_item_id: WorkItemId,
	/// Exact authoritative RuntimeSession.
	pub runtime_session_id: RuntimeSessionId,
	/// Exact current RuntimeSession revision.
	pub runtime_session_revision: i64,
	/// Current RuntimeSession lifecycle read in the same query.
	pub runtime_session_state: RuntimeSessionState,
	/// Validated inert ManagedRun state.
	pub state: ManagedRunState,
	/// Positive ManagedRun revision.
	pub revision: i64,
	/// Monotonic divergence marker.
	pub diverged: bool,
	/// Always true in this slice.
	pub blocked: bool,
	/// PostgreSQL-authored creation timestamp.
	pub created_at: String,
	/// PostgreSQL-authored current-revision timestamp.
	pub updated_at: String,
	/// Exact-run Task and optional Reviewer assignments.
	pub assignments: Vec<ExecutionAssignment>,
	/// FK-backed effect lineage.
	pub effects: Vec<ManagedRunEffectLineage>,
	/// Fail-closed effect barrier.
	pub barrier: ManagedRunEffectBarrier,
}

/// Complete effect of one supported safety input.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManagedRunSafetyEffect {
	/// Canonical ManagedRun identity.
	pub managed_run_id: ManagedRunId,
	/// New positive ManagedRun revision.
	pub managed_run_revision: i64,
	/// Exact authoritative RuntimeSession.
	pub runtime_session_id: RuntimeSessionId,
	/// Current positive RuntimeSession revision.
	pub runtime_session_revision: i64,
	/// Whether this input positively established divergence.
	pub runtime_session_diverged: bool,
	/// Always true for a committed safety input.
	pub managed_run_blocked: bool,
	/// Current positive barrier revision.
	pub effect_barrier_revision: i64,
	/// Whether this input performed the single guarded-to-closed transition.
	pub effect_barrier_closed_now: bool,
	/// Whether an owned submitted-turn receipt was stale at atomic readback.
	pub stale_receipt: bool,
}

/// Stable safety-command rejection stored for exact replay.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManagedRunSafetyRejection {
	/// Required fields or shape were invalid.
	InvalidInput,
	/// ManagedRun, RuntimeSession, or barrier did not exist in exact scope.
	MissingTarget,
	/// Expected ManagedRun revision was stale.
	StaleRevision,
	/// RuntimeSession did not match the run's authoritative association.
	WrongRuntimeSession,
	/// Submitted-turn input did not reference a durable owned receipt.
	MissingSubmittedTurnReceipt,
	/// The asserted unknown turn was already owned or persisted.
	TurnAlreadyOwnedOrKnown,
	/// One durable safety input identity was reused for different facts.
	InputIdentityConflict,
}

/// Parsed exact safety command result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ManagedRunSafetyOutcome {
	/// Safety state committed atomically.
	Success(ManagedRunSafetyEffect),
	/// Stable domain rejection committed without a safety mutation.
	Rejected(ManagedRunSafetyRejection),
}

impl PostgresStore {
	/// Read one exact ManagedRun revision and all restart-critical inert state in one query.
	pub async fn read_managed_run_exact(
		&self,
		project_id: &ProjectId,
		managed_run_id: &ManagedRunId,
		expected_revision: i64,
	) -> Result<StoredManagedRun, StoreError> {
		if expected_revision <= 0 {
			return Err(StoreError::InvalidInput("ManagedRun revision must be positive"));
		}
		let client = self.pool().get().await?;
		let row = client
			.query_opt(
				READ_MANAGED_RUN_SQL,
				&[&managed_run_id.as_str(), &project_id.as_str(), &expected_revision],
			)
			.await?
			.ok_or(StoreError::InvalidInput("exact ManagedRun revision readback did not match"))?;
		parse_readback(row.get(0))
	}

	/// Apply one positive or explicitly inconclusive input through the V12 safety owner.
	pub async fn apply_managed_run_safety_input(
		&self,
		idempotency_key: &str,
		project_id: &ProjectId,
		managed_run_id: &ManagedRunId,
		expected_run_revision: i64,
		input: &ManagedRunSafetyInput,
	) -> Result<ManagedRunSafetyOutcome, StoreError> {
		validate_exact_key(idempotency_key)?;
		if expected_run_revision <= 0 {
			return Err(StoreError::InvalidInput("ManagedRun revision must be positive"));
		}
		let parts = SafetyInputParts::from(input);
		let response = self
			.execute_exact_with_retry(
				"SELECT decodex.apply_managed_run_safety_input_exact(\
			 $1,$2,$3::text::uuid,$4::text::uuid,$5,\
			 $6::text::decodex.managed_run_safety_input_kind,$7::text::uuid,\
			 $8::text::uuid,$9::text::uuid)",
				&[
					&EXACT_COMMAND_PROTOCOL,
					&idempotency_key,
					&managed_run_id.as_str(),
					&project_id.as_str(),
					&expected_run_revision,
					&parts.kind,
					&parts.input_id,
					&parts.runtime_session_id,
					&parts.turn_id,
				],
			)
			.await?;
		parse_safety_response(&response, managed_run_id, &parts)
	}
}

struct SafetyInputParts<'a> {
	kind: &'static str,
	input_id: &'a str,
	runtime_session_id: &'a str,
	turn_id: Option<&'a str>,
}
impl<'a> From<&'a ManagedRunSafetyInput> for SafetyInputParts<'a> {
	fn from(input: &'a ManagedRunSafetyInput) -> Self {
		match input {
			ManagedRunSafetyInput::PositivelyObservedUnknownTurn {
				observation_id,
				runtime_session_id,
				turn_id,
			} => Self::observed(observation_id, runtime_session_id, turn_id),
			ManagedRunSafetyInput::SubmittedTurnReceipt {
				receipt_id,
				runtime_session_id,
				turn_id,
			} => Self::submitted(receipt_id, runtime_session_id, turn_id),
			ManagedRunSafetyInput::InconclusiveObservation {
				observation_id,
				runtime_session_id,
			} => Self {
				kind: "inconclusive_observation",
				input_id: observation_id.as_str(),
				runtime_session_id: runtime_session_id.as_str(),
				turn_id: None,
			},
		}
	}
}
impl<'a> SafetyInputParts<'a> {
	fn observed(
		observation_id: &'a SafetyObservationId,
		runtime_session_id: &'a RuntimeSessionId,
		turn_id: &'a TurnId,
	) -> Self {
		Self {
			kind: "positively_observed_unknown_turn",
			input_id: observation_id.as_str(),
			runtime_session_id: runtime_session_id.as_str(),
			turn_id: Some(turn_id.as_str()),
		}
	}

	fn submitted(
		receipt_id: &'a SubmittedTurnReceiptId,
		runtime_session_id: &'a RuntimeSessionId,
		turn_id: &'a TurnId,
	) -> Self {
		Self {
			kind: "submitted_turn_receipt",
			input_id: receipt_id.as_str(),
			runtime_session_id: runtime_session_id.as_str(),
			turn_id: Some(turn_id.as_str()),
		}
	}
}

const READ_MANAGED_RUN_SQL: &str = "
SELECT pg_catalog.jsonb_build_object(
 'managed_run_id',run.managed_run_id,'project_id',run.project_id,
 'work_item_id',run.work_item_id,'runtime_session_id',run.runtime_session_id,
 'runtime_session_revision',run.runtime_session_revision,
 'runtime_session_state',session.state,'lifecycle',run.lifecycle,'phase',run.phase,
 'wait_reason',run.wait_reason,'revision',run.revision,'diverged',run.diverged,
 'blocked',run.blocked,'created_at',run.created_at,'updated_at',run.updated_at,
 'barrier',pg_catalog.jsonb_build_object('state',barrier.state,'revision',barrier.revision,
   'closure_input_id',barrier.closure_input_id,'closed_at',barrier.closed_at),
 'assignments',COALESCE((SELECT pg_catalog.jsonb_agg(pg_catalog.jsonb_build_object(
   'runtime_session_id',assignment.runtime_session_id,'role',assignment.role)
   ORDER BY assignment.role) FROM decodex.managed_run_assignments assignment
   WHERE assignment.managed_run_id=run.managed_run_id),'[]'::jsonb),
 'effects',COALESCE((SELECT pg_catalog.jsonb_agg(pg_catalog.jsonb_build_object(
   'effect_id',effect.effect_id,'ordinal',effect.ordinal,'kind',effect.kind,
   'effect_key',effect.effect_key) ORDER BY effect.ordinal)
   FROM decodex.managed_run_effects effect
   WHERE effect.managed_run_id=run.managed_run_id),'[]'::jsonb)
) FROM decodex.managed_runs run
JOIN decodex.runtime_sessions session ON session.runtime_session_id=run.runtime_session_id
 AND session.revision=run.runtime_session_revision
JOIN decodex.managed_run_effect_barriers barrier USING(managed_run_id)
WHERE run.managed_run_id=$1::text::uuid AND run.project_id=$2::text::uuid AND run.revision=$3";

fn parse_readback(document: Value) -> Result<StoredManagedRun, StoreError> {
	let managed_run_id = parse_run_id(required_str(&document, "managed_run_id")?)?;
	let lifecycle = lifecycle(required_str(&document, "lifecycle")?)?;
	let phase = phase(required_str(&document, "phase")?)?;
	let wait_reason = wait_reason(required_str(&document, "wait_reason")?)?;
	let state = ManagedRunState::from_parts(lifecycle, phase, Some(wait_reason))
		.map_err(|_| incompatible("stored ManagedRun state is invalid"))?;
	let assignments =
		parse_assignments(required_array(&document, "assignments")?, &managed_run_id)?;
	let effects = parse_effects(required_array(&document, "effects")?)?;
	let barrier_value = required_value(&document, "barrier")?;
	let barrier = ManagedRunEffectBarrier {
		state: barrier_state(required_str(barrier_value, "state")?)?,
		revision: positive_i64(barrier_value, "revision")?,
		closure_input_id: optional_str(barrier_value, "closure_input_id")?,
		closed_at: optional_str(barrier_value, "closed_at")?,
	};
	validate_barrier(&barrier)?;
	let blocked = required_bool(&document, "blocked")?;
	if !blocked {
		return Err(incompatible("stored ManagedRun is not inert"));
	}
	Ok(StoredManagedRun {
		managed_run_id,
		project_id: ProjectId::new(required_str(&document, "project_id")?)
			.map_err(|_| incompatible("stored Project identity is invalid"))?,
		work_item_id: WorkItemId::new(required_str(&document, "work_item_id")?)
			.map_err(|_| incompatible("stored WorkItem identity is invalid"))?,
		runtime_session_id: parse_session_id(required_str(&document, "runtime_session_id")?)?,
		runtime_session_revision: positive_i64(&document, "runtime_session_revision")?,
		runtime_session_state: session_state(required_str(&document, "runtime_session_state")?)?,
		state,
		revision: positive_i64(&document, "revision")?,
		diverged: required_bool(&document, "diverged")?,
		blocked,
		created_at: required_str(&document, "created_at")?.to_owned(),
		updated_at: required_str(&document, "updated_at")?.to_owned(),
		assignments,
		effects,
		barrier,
	})
}

fn parse_assignments(
	values: &[Value],
	managed_run_id: &ManagedRunId,
) -> Result<Vec<ExecutionAssignment>, StoreError> {
	values
		.iter()
		.map(|value| {
			Ok(ExecutionAssignment {
				managed_run_id: managed_run_id.clone(),
				runtime_session_id: parse_session_id(required_str(value, "runtime_session_id")?)?,
				role: match required_str(value, "role")? {
					"task" => ExecutionAssignmentRole::Task,
					"reviewer" => ExecutionAssignmentRole::Reviewer,
					_ => return Err(incompatible("stored execution assignment role is invalid")),
				},
			})
		})
		.collect()
}

fn parse_effects(values: &[Value]) -> Result<Vec<ManagedRunEffectLineage>, StoreError> {
	values
		.iter()
		.map(|value| {
			Ok(ManagedRunEffectLineage {
				effect_id: EffectId::new(required_str(value, "effect_id")?)
					.map_err(|_| incompatible("stored effect identity is invalid"))?,
				kind: match required_str(value, "kind")? {
					"tool" => ManagedRunEffectKind::Tool,
					"repository" => ManagedRunEffectKind::Repository,
					"git" => ManagedRunEffectKind::Git,
					"artifact" => ManagedRunEffectKind::Artifact,
					_ => return Err(incompatible("stored effect kind is invalid")),
				},
				effect_key: required_str(value, "effect_key")?.to_owned(),
				ordinal: i32::try_from(positive_i64(value, "ordinal")?)
					.map_err(|_| incompatible("stored effect ordinal is invalid"))?,
			})
		})
		.collect()
}

fn parse_safety_response(
	response: &[u8],
	managed_run_id: &ManagedRunId,
	parts: &SafetyInputParts<'_>,
) -> Result<ManagedRunSafetyOutcome, StoreError> {
	let document: Value = serde_json::from_slice(response)
		.map_err(|_| incompatible("exact ManagedRun safety response bytes are invalid"))?;
	let effect = required_value(&document, "effect")?;
	match required_str(&document, "classification")? {
		"stable_domain_rejection" =>
			return parse_rejection(required_str(effect, "reason")?)
				.map(ManagedRunSafetyOutcome::Rejected),
		"success" => {},
		_ => return Err(incompatible("exact ManagedRun safety classification is invalid")),
	}
	if required_str(effect, "managed_run_id")? != managed_run_id.as_str()
		|| required_str(effect, "runtime_session_id")? != parts.runtime_session_id
		|| required_str(effect, "effect_barrier_state")? != "closed"
		|| !required_bool(effect, "managed_run_blocked")?
	{
		return Err(incompatible("exact ManagedRun safety effect is inconsistent"));
	}
	Ok(ManagedRunSafetyOutcome::Success(ManagedRunSafetyEffect {
		managed_run_id: managed_run_id.clone(),
		managed_run_revision: positive_i64(effect, "managed_run_revision")?,
		runtime_session_id: parse_session_id(parts.runtime_session_id)?,
		runtime_session_revision: positive_i64(effect, "runtime_session_revision")?,
		runtime_session_diverged: required_bool(effect, "runtime_session_diverged")?,
		managed_run_blocked: true,
		effect_barrier_revision: positive_i64(effect, "effect_barrier_revision")?,
		effect_barrier_closed_now: required_bool(effect, "effect_barrier_closed_now")?,
		stale_receipt: required_bool(effect, "stale_receipt")?,
	}))
}

fn parse_rejection(value: &str) -> Result<ManagedRunSafetyRejection, StoreError> {
	Ok(match value {
		"invalid_input" => ManagedRunSafetyRejection::InvalidInput,
		"missing_target" => ManagedRunSafetyRejection::MissingTarget,
		"stale_revision" => ManagedRunSafetyRejection::StaleRevision,
		"wrong_runtime_session" => ManagedRunSafetyRejection::WrongRuntimeSession,
		"missing_submitted_turn_receipt" => ManagedRunSafetyRejection::MissingSubmittedTurnReceipt,
		"turn_already_owned_or_known" => ManagedRunSafetyRejection::TurnAlreadyOwnedOrKnown,
		"input_identity_conflict" => ManagedRunSafetyRejection::InputIdentityConflict,
		_ => return Err(incompatible("exact ManagedRun safety rejection is invalid")),
	})
}

fn lifecycle(value: &str) -> Result<ManagedRunLifecycle, StoreError> {
	match value {
		"queued" => Ok(ManagedRunLifecycle::Queued),
		"active" => Ok(ManagedRunLifecycle::Active),
		"waiting" => Ok(ManagedRunLifecycle::Waiting),
		"terminal" => Ok(ManagedRunLifecycle::Terminal),
		_ => Err(incompatible("stored ManagedRun lifecycle is invalid")),
	}
}
fn phase(value: &str) -> Result<ManagedRunPhase, StoreError> {
	match value {
		"prepare" => Ok(ManagedRunPhase::Prepare),
		"execute" => Ok(ManagedRunPhase::Execute),
		"validate" => Ok(ManagedRunPhase::Validate),
		"review" => Ok(ManagedRunPhase::Review),
		"repair" => Ok(ManagedRunPhase::Repair),
		"land" => Ok(ManagedRunPhase::Land),
		"close" => Ok(ManagedRunPhase::Close),
		_ => Err(incompatible("stored ManagedRun phase is invalid")),
	}
}
fn wait_reason(value: &str) -> Result<ManagedRunWaitReason, StoreError> {
	match value {
		"usage" => Ok(ManagedRunWaitReason::Usage),
		"auth" => Ok(ManagedRunWaitReason::Auth),
		"plugin" => Ok(ManagedRunWaitReason::Plugin),
		"dependency" => Ok(ManagedRunWaitReason::Dependency),
		"approval" => Ok(ManagedRunWaitReason::Approval),
		"user" => Ok(ManagedRunWaitReason::User),
		"external" => Ok(ManagedRunWaitReason::External),
		"reviewer_unavailable" => Ok(ManagedRunWaitReason::ReviewerUnavailable),
		"reviewer_failed" => Ok(ManagedRunWaitReason::ReviewerFailed),
		_ => Err(incompatible("stored ManagedRun wait reason is invalid")),
	}
}
fn session_state(value: &str) -> Result<RuntimeSessionState, StoreError> {
	match value {
		"starting" => Ok(RuntimeSessionState::Starting),
		"active" => Ok(RuntimeSessionState::Active),
		"ended" => Ok(RuntimeSessionState::Ended),
		"diverged" => Ok(RuntimeSessionState::Diverged),
		_ => Err(incompatible("stored RuntimeSession state is invalid")),
	}
}
fn barrier_state(value: &str) -> Result<ManagedRunEffectBarrierState, StoreError> {
	match value {
		"guarded" => Ok(ManagedRunEffectBarrierState::Guarded),
		"closed" => Ok(ManagedRunEffectBarrierState::Closed),
		_ => Err(incompatible("stored effect barrier state is invalid")),
	}
}
fn validate_barrier(value: &ManagedRunEffectBarrier) -> Result<(), StoreError> {
	match (value.state, value.revision, value.closure_input_id.is_some(), value.closed_at.is_some())
	{
		(ManagedRunEffectBarrierState::Guarded, 1, false, false)
		| (ManagedRunEffectBarrierState::Closed, 2, true, true) => Ok(()),
		_ => Err(incompatible("stored effect barrier shape is invalid")),
	}
}
fn parse_run_id(value: &str) -> Result<ManagedRunId, StoreError> {
	ManagedRunId::new(value).map_err(|_| incompatible("stored ManagedRun identity is invalid"))
}
fn parse_session_id(value: &str) -> Result<RuntimeSessionId, StoreError> {
	RuntimeSessionId::new(value)
		.map_err(|_| incompatible("stored RuntimeSession identity is invalid"))
}
fn required_value<'a>(value: &'a Value, key: &str) -> Result<&'a Value, StoreError> {
	value.get(key).ok_or_else(|| incompatible("ManagedRun document is missing a field"))
}
fn required_str<'a>(value: &'a Value, key: &str) -> Result<&'a str, StoreError> {
	required_value(value, key)?
		.as_str()
		.ok_or_else(|| incompatible("ManagedRun string field is invalid"))
}
fn required_bool(value: &Value, key: &str) -> Result<bool, StoreError> {
	required_value(value, key)?
		.as_bool()
		.ok_or_else(|| incompatible("ManagedRun boolean field is invalid"))
}
fn positive_i64(value: &Value, key: &str) -> Result<i64, StoreError> {
	let number = required_value(value, key)?
		.as_i64()
		.ok_or_else(|| incompatible("ManagedRun numeric field is invalid"))?;
	if number <= 0 {
		Err(incompatible("ManagedRun numeric field is not positive"))
	} else {
		Ok(number)
	}
}
fn required_array<'a>(value: &'a Value, key: &str) -> Result<&'a [Value], StoreError> {
	required_value(value, key)?
		.as_array()
		.map(Vec::as_slice)
		.ok_or_else(|| incompatible("ManagedRun array field is invalid"))
}
fn optional_str(value: &Value, key: &str) -> Result<Option<String>, StoreError> {
	match required_value(value, key)? {
		Value::Null => Ok(None),
		Value::String(text) => Ok(Some(text.clone())),
		_ => Err(incompatible("ManagedRun optional string field is invalid")),
	}
}
fn incompatible(message: &'static str) -> StoreError {
	StoreError::Incompatible(message.into())
}
