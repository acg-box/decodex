//! ManagedRun domain readback after the V12 turn-effect authority cutover.
//!
//! ManagedRun keeps its lifecycle and execution assignments. ProviderAttempt is the sole
//! provider-effect ledger. This module consumes all result projections without copying or
//! advancing ProviderAttempt state.

use decodex_core::{
	ExecutionAssignment, ExecutionAssignmentRole, ManagedExecutionId, ManagedRunId,
	ManagedRunLifecycle, ManagedRunPhase, ManagedRunState, ManagedRunWaitReason,
	ProcessGenerationId, ProjectId, ProviderAttemptId, ProviderAttemptState,
	ProviderAttemptUnknownReason, ProviderEvidenceId, RuntimeSessionId, RuntimeSessionState,
	WorkItemId,
};
use serde_json::Value;

use crate::{PostgresStore, StoreError};

/// One ProviderAttempt result projection consumed by the ManagedRun owner.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManagedRunProviderAttempt {
	/// Exact execution intent within the ManagedRun.
	pub execution_id: ManagedExecutionId,
	/// Original ProviderAttempt identity.
	pub attempt_id: ProviderAttemptId,
	/// Exact ProcessGeneration retained by the attempt authority.
	pub process_generation_id: ProcessGenerationId,
	/// Current positive-only ProviderAttempt state.
	pub state: ProviderAttemptState,
	/// Current positive ProviderAttempt revision.
	pub revision: i64,
	/// Exact positive terminal evidence, only for a terminal result.
	pub terminal_evidence_id: Option<ProviderEvidenceId>,
	/// Exact ambiguity cause, only while the attempt is unknown.
	pub unknown_reason: Option<ProviderAttemptUnknownReason>,
}

/// Exact revisioned readback for one ManagedRun.
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
	/// Validated ManagedRun state.
	pub state: ManagedRunState,
	/// Positive ManagedRun revision.
	pub revision: i64,
	/// Monotonic divergence marker.
	pub diverged: bool,
	/// Current ManagedRun-owned execution block.
	pub blocked: bool,
	/// PostgreSQL-authored creation timestamp.
	pub created_at: String,
	/// PostgreSQL-authored current-revision timestamp.
	pub updated_at: String,
	/// Exact-run Task and optional read-only Reviewer assignments.
	pub assignments: Vec<ExecutionAssignment>,
	/// All ProviderAttempt result projections. This is not a second effect ledger.
	pub provider_attempts: Vec<ManagedRunProviderAttempt>,
}

impl PostgresStore {
	/// Read one exact ManagedRun revision with its owner state and ProviderAttempt projection.
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
			.query_one(
				READ_MANAGED_RUN_SQL,
				&[&managed_run_id.as_str(), &project_id.as_str(), &expected_revision],
			)
			.await?;
		let document: Option<Value> = row.get(0);
		parse_readback(
			document.ok_or(StoreError::InvalidInput(
				"exact ManagedRun revision readback did not match",
			))?,
		)
	}
}

const READ_MANAGED_RUN_SQL: &str = "
SELECT decodex.read_managed_run_execution_exact(
 $1::text::uuid,$2::text::uuid,$3
)";

fn parse_readback(document: Value) -> Result<StoredManagedRun, StoreError> {
	let managed_run_id = ManagedRunId::new(required_str(&document, "managed_run_id")?)
		.map_err(|_| incompatible("stored ManagedRun identity is invalid"))?;
	let project_id = ProjectId::new(required_str(&document, "project_id")?)
		.map_err(|_| incompatible("stored Project identity is invalid"))?;
	let work_item_id = WorkItemId::new(required_str(&document, "work_item_id")?)
		.map_err(|_| incompatible("stored WorkItem identity is invalid"))?;
	let runtime_session_id = RuntimeSessionId::new(required_str(&document, "runtime_session_id")?)
		.map_err(|_| incompatible("stored RuntimeSession identity is invalid"))?;
	let state = ManagedRunState::from_parts(
		lifecycle(required_str(&document, "lifecycle")?)?,
		phase(required_str(&document, "phase")?)?,
		optional_str(&document, "wait_reason")?.map(|value| wait_reason(&value)).transpose()?,
	)
	.map_err(|_| incompatible("stored ManagedRun state is invalid"))?;
	let assignments = required_array(&document, "assignments")?
		.iter()
		.map(|value| {
			Ok(ExecutionAssignment {
				managed_run_id: managed_run_id.clone(),
				runtime_session_id: RuntimeSessionId::new(required_str(
					value,
					"runtime_session_id",
				)?)
				.map_err(|_| incompatible("stored assignment RuntimeSession is invalid"))?,
				role: match required_str(value, "role")? {
					"task" => ExecutionAssignmentRole::Task,
					"reviewer" => ExecutionAssignmentRole::Reviewer,
					_ => return Err(incompatible("stored execution assignment role is invalid")),
				},
			})
		})
		.collect::<Result<Vec<_>, _>>()?;
	let provider_attempts = required_array(&document, "provider_attempts")?
		.iter()
		.map(|value| {
			let state = provider_state(required_str(value, "state")?)?;
			let terminal_evidence_id = optional_str(value, "terminal_evidence_id")?
				.map(ProviderEvidenceId::new)
				.transpose()
				.map_err(|_| incompatible("stored ProviderEvidence identity is invalid"))?;
			let unknown_reason = optional_str(value, "unknown_reason")?
				.map(|value| provider_unknown_reason(&value))
				.transpose()?;
			let has_positive_result = matches!(
				state,
				ProviderAttemptState::Succeeded
					| ProviderAttemptState::FailedDefinitive
					| ProviderAttemptState::NotSubmitted
			);
			if has_positive_result != terminal_evidence_id.is_some()
				|| (state == ProviderAttemptState::Unknown) != unknown_reason.is_some()
			{
				return Err(incompatible("stored ProviderAttempt result shape is invalid"));
			}
			Ok(ManagedRunProviderAttempt {
				execution_id: ManagedExecutionId::new(required_str(value, "execution_id")?)
					.map_err(|_| incompatible("stored ManagedRun execution identity is invalid"))?,
				attempt_id: ProviderAttemptId::new(required_str(value, "attempt_id")?)
					.map_err(|_| incompatible("stored ProviderAttempt identity is invalid"))?,
				process_generation_id: ProcessGenerationId::new(required_str(
					value,
					"process_generation_id",
				)?)
				.map_err(|_| incompatible("stored ProcessGeneration identity is invalid"))?,
				state,
				revision: positive_i64(value, "revision")?,
				terminal_evidence_id,
				unknown_reason,
			})
		})
		.collect::<Result<Vec<_>, _>>()?;

	Ok(StoredManagedRun {
		managed_run_id,
		project_id,
		work_item_id,
		runtime_session_id,
		runtime_session_revision: positive_i64(&document, "runtime_session_revision")?,
		runtime_session_state: session_state(required_str(&document, "runtime_session_state")?)?,
		state,
		revision: positive_i64(&document, "revision")?,
		diverged: required_bool(&document, "diverged")?,
		blocked: required_bool(&document, "blocked")?,
		created_at: required_str(&document, "created_at")?.to_owned(),
		updated_at: required_str(&document, "updated_at")?.to_owned(),
		assignments,
		provider_attempts,
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
		"reconciliation" => Ok(ManagedRunWaitReason::Reconciliation),
		"reviewer_unavailable" => Ok(ManagedRunWaitReason::ReviewerUnavailable),
		"reviewer_failed" => Ok(ManagedRunWaitReason::ReviewerFailed),
		"reviewer_ambiguous" => Ok(ManagedRunWaitReason::ReviewerAmbiguous),
		_ => Err(incompatible("stored ManagedRun wait reason is invalid")),
	}
}

fn provider_state(value: &str) -> Result<ProviderAttemptState, StoreError> {
	match value {
		"prepared" => Ok(ProviderAttemptState::Prepared),
		"canceled" => Ok(ProviderAttemptState::Canceled),
		"dispatch_authorized" => Ok(ProviderAttemptState::DispatchAuthorized),
		"succeeded" => Ok(ProviderAttemptState::Succeeded),
		"failed_definitive" => Ok(ProviderAttemptState::FailedDefinitive),
		"not_submitted" => Ok(ProviderAttemptState::NotSubmitted),
		"unknown" => Ok(ProviderAttemptState::Unknown),
		_ => Err(incompatible("stored ProviderAttempt state is invalid")),
	}
}

fn provider_unknown_reason(value: &str) -> Result<ProviderAttemptUnknownReason, StoreError> {
	match value {
		"supervision_lost" => Ok(ProviderAttemptUnknownReason::SupervisionLost),
		"dispatch_outcome_unavailable" =>
			Ok(ProviderAttemptUnknownReason::DispatchOutcomeUnavailable),
		"restore_projection" => Ok(ProviderAttemptUnknownReason::RestoreProjection),
		_ => Err(incompatible("stored ProviderAttempt unknown reason is invalid")),
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

fn required_value<'a>(value: &'a Value, key: &str) -> Result<&'a Value, StoreError> {
	value.get(key).ok_or_else(|| incompatible("ManagedRun document is missing a field"))
}

fn required_str<'a>(value: &'a Value, key: &str) -> Result<&'a str, StoreError> {
	required_value(value, key)?
		.as_str()
		.ok_or_else(|| incompatible("ManagedRun string field is invalid"))
}

fn optional_str(value: &Value, key: &str) -> Result<Option<String>, StoreError> {
	match required_value(value, key)? {
		Value::Null => Ok(None),
		Value::String(text) => Ok(Some(text.clone())),
		_ => Err(incompatible("ManagedRun optional string field is invalid")),
	}
}

fn required_bool(value: &Value, key: &str) -> Result<bool, StoreError> {
	required_value(value, key)?
		.as_bool()
		.ok_or_else(|| incompatible("ManagedRun Boolean field is invalid"))
}

fn positive_i64(value: &Value, key: &str) -> Result<i64, StoreError> {
	let number = required_value(value, key)?
		.as_i64()
		.ok_or_else(|| incompatible("ManagedRun numeric field is invalid"))?;
	if number > 0 {
		Ok(number)
	} else {
		Err(incompatible("ManagedRun numeric field is not positive"))
	}
}

fn required_array<'a>(value: &'a Value, key: &str) -> Result<&'a [Value], StoreError> {
	required_value(value, key)?
		.as_array()
		.map(Vec::as_slice)
		.ok_or_else(|| incompatible("ManagedRun array field is invalid"))
}

fn incompatible(message: &'static str) -> StoreError {
	StoreError::Incompatible(message.into())
}
