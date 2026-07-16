//! Transactional PostgreSQL authority for open-ended Programs and finite Objectives.

use std::fmt::Display;

use deadpool_postgres::Transaction;
use serde_json::Value;
use tokio_postgres::Row;

use crate::{CommandIdentity, PostgresStore, StoreError, accounts};
use decodex_core::{
	AgentId, Objective, ObjectiveCompletionEvidence, ObjectiveEvidenceId, ObjectiveId,
	ObjectiveState, PolicyId, PolicyRevision, PolicyRevisionId, Program, ProgramCorrelationId,
	ProgramId, ProgramMetric, ProgramProvenance, ProgramSignal, ProgramState, ProgramTimestamp,
	ProjectId, ReviewCadence,
};

const PROGRAM_SELECT: &str = concat!(
	"SELECT program_id::text,project_id::text,owner_agent_id::text,name,",
	"responsibility,state::text,policy_id::text,policy_revision,review_interval_days,",
	"(EXTRACT(EPOCH FROM next_review_at)*1000000)::bigint,metrics,signals,revision,",
	"last_changed_by::text,last_correlation_id::text,last_provenance ",
	"FROM decodex.programs WHERE program_id=$1::text::uuid",
);
const OBJECTIVE_SELECT: &str = concat!(
	"SELECT objective.objective_id::text,objective.project_id::text,",
	"objective.program_id::text,objective.outcome,objective.acceptance_criteria,",
	"objective.validation_criteria,(EXTRACT(EPOCH FROM objective.target_at)*1000000)::bigint,",
	"objective.state::text,objective.revision,objective.completion_evidence_id::text,",
	"objective.last_changed_by::text,objective.last_correlation_id::text,objective.last_provenance,",
	"evidence.evidence_id::text,evidence.objective_revision,",
	"(EXTRACT(EPOCH FROM evidence.objective_updated_at)*1000000)::bigint,",
	"evidence.acceptance_result,",
	"evidence.accepted_by::text,(EXTRACT(EPOCH FROM evidence.accepted_at)*1000000)::bigint,",
	"evidence.acceptance_provenance,evidence.validation_result,evidence.validated_by::text,",
	"(EXTRACT(EPOCH FROM evidence.validated_at)*1000000)::bigint,evidence.validation_provenance,",
	"evidence.correlation_id::text,(EXTRACT(EPOCH FROM evidence.recorded_at)*1000000)::bigint ",
	"FROM decodex.objectives AS objective ",
	"LEFT JOIN decodex.objective_completion_evidence AS evidence ",
	"ON evidence.evidence_id=objective.completion_evidence_id ",
	"WHERE objective.objective_id=$1::text::uuid",
);

/// Persisted Program plus provenance for its latest committed revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramRecord {
	/// Open-ended Program authority.
	pub program: Program,
	/// Provenance for the current revision.
	pub last_change: ProgramProvenance,
}

/// Persisted Objective plus provenance for its latest committed revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectiveRecord {
	/// Finite Objective authority.
	pub objective: Objective,
	/// Provenance for the current revision.
	pub last_change: ProgramProvenance,
}

/// Optimistically guarded replacement of mutable Program review/observation context.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpdateProgramContext {
	/// Program to update.
	pub program_id: ProgramId,
	/// Canonical request-authority Project scope.
	pub project_id: ProjectId,
	/// Exact current revision.
	pub expected_revision: u64,
	/// Replacement review cadence.
	pub review_cadence: ReviewCadence,
	/// Replacement bounded metrics.
	pub metrics: Vec<ProgramMetric>,
	/// Replacement bounded signals.
	pub signals: Vec<ProgramSignal>,
}

struct ProgramCommandDescriptor {
	operation: &'static str,
	project_id: String,
	entity_id: String,
	expected_revision: Option<i64>,
}

struct ProgramCommandReservation {
	key: String,
	request_hash: String,
	claim_token: String,
}

enum ProgramCommandClaim {
	Owned(ProgramCommandReservation),
	Completed(Value),
}

enum ProgramMutation<'a> {
	Update {
		update: &'a UpdateProgramContext,
		expected_revision: i64,
		metrics: Value,
		signals: Value,
		provenance: &'a ProgramProvenance,
	},
	Transition {
		project_id: &'a ProjectId,
		program_id: &'a ProgramId,
		expected_revision: i64,
		state: ProgramState,
		provenance: &'a ProgramProvenance,
	},
}

enum ObjectiveMutation<'a> {
	Transition {
		project_id: &'a ProjectId,
		objective_id: &'a ObjectiveId,
		expected_revision: i64,
		state: ObjectiveState,
		provenance: &'a ProgramProvenance,
	},
	Achieve {
		evidence: &'a ObjectiveCompletionEvidence,
		expected_revision: i64,
	},
}

#[derive(Debug)]
enum Rejection {
	RevisionConflict { entity: String, expected: Option<i64>, actual: Option<i64> },
	NotFound,
	InvalidAuthority,
	InvalidPolicy,
	InvalidProgram,
	InvalidHorizon,
	InvalidTransition,
	InvalidEvidence,
	InvalidProject,
	ConflictingIdentity,
}

impl PostgresStore {
	/// Create one active Program under canonical Project Lead and exact Policy authority.
	pub async fn create_program(
		&self,
		command: &CommandIdentity,
		program: &Program,
		provenance: &ProgramProvenance,
	) -> Result<ProgramRecord, StoreError> {
		if program.state() != ProgramState::Active || program.revision() != 1 {
			return Err(StoreError::InvalidInput("new Program must be active at revision one"));
		}
		if program.owner_agent_id() != provenance.actor_id() {
			return Err(StoreError::InvalidInput(
				"Program creation provenance must be the assigned owner",
			));
		}

		let metrics = serde_json::to_value(program.metrics())
			.map_err(|_| StoreError::InvalidInput("Program metrics cannot be serialized"))?;
		let signals = serde_json::to_value(program.signals())
			.map_err(|_| StoreError::InvalidInput("Program signals cannot be serialized"))?;

		crate::ensure_credential_negative_json(&metrics)?;
		crate::ensure_credential_negative_json(&signals)?;
		crate::ensure_credential_negative_text(provenance.summary())?;

		let descriptor = ProgramCommandDescriptor {
			operation: "create_program",
			project_id: program.project_id().to_string(),
			entity_id: program.id().to_string(),
			expected_revision: None,
		};
		let mut client = crate::checkout(self.pool(), &self.connector).await?;
		let transaction = client.transaction().await?;
		let reservation = match reserve_program_command(&transaction, command, &descriptor).await? {
			ProgramCommandClaim::Completed(response) => {
				transaction.commit().await?;

				return program_result_from_response(response);
			},
			ProgramCommandClaim::Owned(reservation) => reservation,
		};
		let row = transaction
			.query_one(
				"SELECT result_code,actual_revision,changed FROM decodex.create_program(\
				 $1::pg_catalog.text::decodex.canonical_uuid_v4_text,\
				 $2::pg_catalog.text::decodex.canonical_uuid_v4_text,\
				 $3::pg_catalog.text::decodex.canonical_uuid_v4_text,$4,$5,\
				 $6::pg_catalog.text::decodex.canonical_uuid_v4_text,$7,$8,$9,$10,$11,\
				 $12::pg_catalog.text::decodex.canonical_uuid_v4_text,$13)",
				&[
					&program.id().as_str(),
					&program.project_id().as_str(),
					&program.owner_agent_id().as_str(),
					&program.name(),
					&program.responsibility(),
					&program.policy_revision_id().policy_id().as_str(),
					&to_i64(program.policy_revision_id().revision().get())?,
					&i32::from(program.review_cadence().interval_days()),
					&program.review_cadence().next_review_at().unix_microseconds(),
					&metrics,
					&signals,
					&provenance.correlation_id().as_str(),
					&provenance.summary(),
				],
			)
			.await?;
		let result = command_result(&row, &descriptor)?;
		let response = if result.is_ok() {
			let record =
				read_program(&transaction, program.id()).await?.ok_or_else(incompatible)?;

			if row.get(2) {
				append_program_activity(
					&transaction,
					"program",
					program.id().as_str(),
					record.program.revision(),
					"program_created",
					command,
					serde_json::json!({"project_id":program.project_id().as_str(),"state":"active"}),
				)
				.await?;
			}

			program_response(&record)?
		} else {
			error_response(result.expect_err("checked error"), &descriptor)
		};

		finish_program_command(&transaction, &reservation, &response).await?;

		transaction.commit().await?;

		program_result_from_response(response)
	}

	/// Replace mutable Program review/observation context under optimistic concurrency.
	pub async fn update_program_context(
		&self,
		command: &CommandIdentity,
		update: &UpdateProgramContext,
		provenance: &ProgramProvenance,
	) -> Result<ProgramRecord, StoreError> {
		let expected_revision = to_i64(update.expected_revision)?;
		let metrics = serde_json::to_value(&update.metrics)
			.map_err(|_| StoreError::InvalidInput("Program metrics cannot be serialized"))?;
		let signals = serde_json::to_value(&update.signals)
			.map_err(|_| StoreError::InvalidInput("Program signals cannot be serialized"))?;

		crate::ensure_credential_negative_json(&metrics)?;
		crate::ensure_credential_negative_json(&signals)?;
		crate::ensure_credential_negative_text(provenance.summary())?;

		let descriptor = ProgramCommandDescriptor {
			operation: "update_program_context",
			project_id: update.project_id.to_string(),
			entity_id: update.program_id.to_string(),
			expected_revision: Some(expected_revision),
		};

		self.execute_program_mutation(
			command,
			descriptor,
			ProgramMutation::Update { update, expected_revision, metrics, signals, provenance },
		)
		.await
	}

	/// Transition one Program lifecycle under optimistic concurrency.
	pub async fn transition_program(
		&self,
		command: &CommandIdentity,
		project_id: &ProjectId,
		program_id: &ProgramId,
		expected_revision: u64,
		state: ProgramState,
		provenance: &ProgramProvenance,
	) -> Result<ProgramRecord, StoreError> {
		let expected_revision = to_i64(expected_revision)?;
		let descriptor = ProgramCommandDescriptor {
			operation: "transition_program",
			project_id: project_id.to_string(),
			entity_id: program_id.to_string(),
			expected_revision: Some(expected_revision),
		};

		self.execute_program_mutation(
			command,
			descriptor,
			ProgramMutation::Transition {
				project_id,
				program_id,
				expected_revision,
				state,
				provenance,
			},
		)
		.await
	}

	/// Deterministically read one Program and its current-revision provenance.
	pub async fn program(&self, id: &ProgramId) -> Result<Option<ProgramRecord>, StoreError> {
		let client = crate::checkout(self.pool(), &self.connector).await?;
		let row = client.query_opt(PROGRAM_SELECT, &[&id.as_str()]).await?;

		row.map(program_from_row).transpose()
	}

	/// Create one finite proposed Objective under canonical active Lead authority.
	pub async fn create_objective(
		&self,
		command: &CommandIdentity,
		objective: &Objective,
		provenance: &ProgramProvenance,
	) -> Result<ObjectiveRecord, StoreError> {
		if objective.state() != ObjectiveState::Proposed || objective.revision() != 1 {
			return Err(StoreError::InvalidInput("new Objective must be proposed at revision one"));
		}

		crate::ensure_credential_negative_text(provenance.summary())?;

		let descriptor = ProgramCommandDescriptor {
			operation: "create_objective",
			project_id: objective.project_id().to_string(),
			entity_id: objective.id().to_string(),
			expected_revision: None,
		};
		let mut client = crate::checkout(self.pool(), &self.connector).await?;
		let transaction = client.transaction().await?;
		let reservation = match reserve_program_command(&transaction, command, &descriptor).await? {
			ProgramCommandClaim::Completed(response) => {
				transaction.commit().await?;

				return objective_result_from_response(response);
			},
			ProgramCommandClaim::Owned(reservation) => reservation,
		};
		let row = transaction
			.query_one(
				"SELECT result_code,actual_revision,changed FROM decodex.create_objective(\
				 $1::pg_catalog.text::decodex.canonical_uuid_v4_text,\
				 $2::pg_catalog.text::decodex.canonical_uuid_v4_text,\
				 $3::pg_catalog.text::decodex.canonical_uuid_v4_text,$4,$5,$6,$7,\
				 $8::pg_catalog.text::decodex.canonical_uuid_v4_text,\
				 $9::pg_catalog.text::decodex.canonical_uuid_v4_text,$10)",
				&[
					&objective.id().as_str(),
					&objective.project_id().as_str(),
					&objective.program_id().map(ProgramId::as_str),
					&objective.outcome(),
					&objective.acceptance_criteria(),
					&objective.validation_criteria(),
					&objective.target_at().unix_microseconds(),
					&provenance.actor_id().as_str(),
					&provenance.correlation_id().as_str(),
					&provenance.summary(),
				],
			)
			.await?;
		let result = command_result(&row, &descriptor)?;
		let response = if result.is_ok() {
			let record =
				read_objective(&transaction, objective.id()).await?.ok_or_else(incompatible)?;

			if row.get(2) {
				append_program_activity(
					&transaction,
					"objective",
					objective.id().as_str(),
					record.objective.revision(),
					"objective_created",
					command,
					serde_json::json!({"project_id":objective.project_id().as_str(),"state":"proposed"}),
				)
				.await?;
			}

			objective_response(&record)?
		} else {
			error_response(result.expect_err("checked error"), &descriptor)
		};

		finish_program_command(&transaction, &reservation, &response).await?;

		transaction.commit().await?;

		objective_result_from_response(response)
	}

	/// Transition one finite Objective without permitting bare achievement.
	pub async fn transition_objective(
		&self,
		command: &CommandIdentity,
		project_id: &ProjectId,
		objective_id: &ObjectiveId,
		expected_revision: u64,
		state: ObjectiveState,
		provenance: &ProgramProvenance,
	) -> Result<ObjectiveRecord, StoreError> {
		let expected_revision = to_i64(expected_revision)?;
		let descriptor = ProgramCommandDescriptor {
			operation: "transition_objective",
			project_id: project_id.to_string(),
			entity_id: objective_id.to_string(),
			expected_revision: Some(expected_revision),
		};

		self.execute_objective_mutation(
			command,
			descriptor,
			ObjectiveMutation::Transition {
				project_id,
				objective_id,
				expected_revision,
				state,
				provenance,
			},
		)
		.await
	}

	/// Achieve one Objective only by persisting exact immutable acceptance and validation evidence.
	pub async fn achieve_objective(
		&self,
		command: &CommandIdentity,
		evidence: &ObjectiveCompletionEvidence,
	) -> Result<ObjectiveRecord, StoreError> {
		let expected_revision = to_i64(evidence.objective_revision())?;
		let descriptor = ProgramCommandDescriptor {
			operation: "achieve_objective",
			project_id: evidence.project_id().to_string(),
			entity_id: evidence.objective_id().to_string(),
			expected_revision: Some(expected_revision),
		};

		self.execute_objective_mutation(
			command,
			descriptor,
			ObjectiveMutation::Achieve { evidence, expected_revision },
		)
		.await
	}

	/// Deterministically read one Objective, completion evidence, and current provenance.
	pub async fn objective(&self, id: &ObjectiveId) -> Result<Option<ObjectiveRecord>, StoreError> {
		let client = crate::checkout(self.pool(), &self.connector).await?;
		let row = client.query_opt(OBJECTIVE_SELECT, &[&id.as_str()]).await?;

		row.map(objective_from_row).transpose()
	}

	async fn execute_program_mutation(
		&self,
		command: &CommandIdentity,
		descriptor: ProgramCommandDescriptor,
		mutation: ProgramMutation<'_>,
	) -> Result<ProgramRecord, StoreError> {
		let mut client = crate::checkout(self.pool(), &self.connector).await?;
		let transaction = client.transaction().await?;
		let reservation = match reserve_program_command(&transaction, command, &descriptor).await? {
			ProgramCommandClaim::Completed(response) => {
				transaction.commit().await?;

				return program_result_from_response(response);
			},
			ProgramCommandClaim::Owned(reservation) => reservation,
		};
		let (row, program_id, event_kind) = match mutation {
			ProgramMutation::Update { update, expected_revision, metrics, signals, provenance } => {
				let row = transaction
					.query_one(
						"SELECT result_code,actual_revision,changed FROM decodex.update_program_context(\
						 $1::pg_catalog.text::decodex.canonical_uuid_v4_text,\
						 $2::pg_catalog.text::decodex.canonical_uuid_v4_text,$3,$4,$5,$6,$7,\
						 $8::pg_catalog.text::decodex.canonical_uuid_v4_text,\
						 $9::pg_catalog.text::decodex.canonical_uuid_v4_text,$10)",
						&[
							&update.program_id.as_str(),
							&update.project_id.as_str(),
							&expected_revision,
							&i32::from(update.review_cadence.interval_days()),
							&update.review_cadence.next_review_at().unix_microseconds(),
							&metrics,
							&signals,
							&provenance.actor_id().as_str(),
							&provenance.correlation_id().as_str(),
							&provenance.summary(),
						],
					)
					.await?;

				(row, update.program_id.clone(), "program_context_updated")
			},
			ProgramMutation::Transition {
				project_id,
				program_id,
				expected_revision,
				state,
				provenance,
			} => {
				let row = transaction
					.query_one(
						"SELECT result_code,actual_revision,changed FROM decodex.transition_program(\
						 $1::pg_catalog.text::decodex.canonical_uuid_v4_text,\
						 $2::pg_catalog.text::decodex.canonical_uuid_v4_text,$3,\
						 $4::pg_catalog.text::decodex.program_state,\
						 $5::pg_catalog.text::decodex.canonical_uuid_v4_text,\
						 $6::pg_catalog.text::decodex.canonical_uuid_v4_text,$7)",
						&[
							&program_id.as_str(),
							&project_id.as_str(),
							&expected_revision,
							&program_state_text(state),
							&provenance.actor_id().as_str(),
							&provenance.correlation_id().as_str(),
							&provenance.summary(),
						],
					)
					.await?;

				(row, program_id.clone(), "program_transitioned")
			},
		};
		let result = command_result(&row, &descriptor)?;
		let response = if result.is_ok() {
			let record = read_program(&transaction, &program_id).await?.ok_or_else(incompatible)?;

			if row.get(2) {
				append_program_activity(
					&transaction,
					"program",
					program_id.as_str(),
					record.program.revision(),
					event_kind,
					command,
					serde_json::json!({"project_id":record.program.project_id().as_str(),"state":record.program.state().as_str()}),
				)
				.await?;
			}

			program_response(&record)?
		} else {
			error_response(result.expect_err("checked error"), &descriptor)
		};

		finish_program_command(&transaction, &reservation, &response).await?;

		transaction.commit().await?;

		program_result_from_response(response)
	}

	async fn execute_objective_mutation(
		&self,
		command: &CommandIdentity,
		descriptor: ProgramCommandDescriptor,
		mutation: ObjectiveMutation<'_>,
	) -> Result<ObjectiveRecord, StoreError> {
		let mut client = crate::checkout(self.pool(), &self.connector).await?;
		let transaction = client.transaction().await?;
		let reservation = match reserve_program_command(&transaction, command, &descriptor).await? {
			ProgramCommandClaim::Completed(response) => {
				transaction.commit().await?;

				return objective_result_from_response(response);
			},
			ProgramCommandClaim::Owned(reservation) => reservation,
		};
		let (row, objective_id, event_kind) = match mutation {
			ObjectiveMutation::Transition {
				project_id,
				objective_id,
				expected_revision,
				state,
				provenance,
			} => {
				let row = transaction
					.query_one(
						"SELECT result_code,actual_revision,changed FROM decodex.transition_objective(\
						 $1::pg_catalog.text::decodex.canonical_uuid_v4_text,\
						 $2::pg_catalog.text::decodex.canonical_uuid_v4_text,$3,\
						 $4::pg_catalog.text::decodex.objective_state,\
						 $5::pg_catalog.text::decodex.canonical_uuid_v4_text,\
						 $6::pg_catalog.text::decodex.canonical_uuid_v4_text,$7)",
						&[
							&objective_id.as_str(),
							&project_id.as_str(),
							&expected_revision,
							&objective_state_text(state),
							&provenance.actor_id().as_str(),
							&provenance.correlation_id().as_str(),
							&provenance.summary(),
						],
					)
					.await?;

				(row, objective_id.clone(), "objective_transitioned")
			},
			ObjectiveMutation::Achieve { evidence, expected_revision } => {
				let row = transaction
					.query_one(
						"SELECT result_code,actual_revision,changed FROM decodex.achieve_objective(\
					 $1::pg_catalog.text::decodex.canonical_uuid_v4_text,\
					 $2::pg_catalog.text::decodex.canonical_uuid_v4_text,\
					 $3::pg_catalog.text::decodex.canonical_uuid_v4_text,$4,$5,\
					 $6::pg_catalog.text::decodex.canonical_uuid_v4_text,$7,$8,$9,\
					 $10::pg_catalog.text::decodex.canonical_uuid_v4_text,$11,$12,\
					 $13::pg_catalog.text::decodex.canonical_uuid_v4_text)",
						&[
							&evidence.id().as_str(),
							&evidence.objective_id().as_str(),
							&evidence.project_id().as_str(),
							&expected_revision,
							&evidence.acceptance_result(),
							&evidence.accepted_by().as_str(),
							&evidence.accepted_at().unix_microseconds(),
							&evidence.acceptance_provenance(),
							&evidence.validation_result(),
							&evidence.validated_by().as_str(),
							&evidence.validated_at().unix_microseconds(),
							&evidence.validation_provenance(),
							&evidence.correlation_id().as_str(),
						],
					)
					.await?;

				(row, evidence.objective_id().clone(), "objective_achieved")
			},
		};
		let result = command_result(&row, &descriptor)?;
		let response = if result.is_ok() {
			let record =
				read_objective(&transaction, &objective_id).await?.ok_or_else(incompatible)?;

			if row.get(2) {
				append_program_activity(
					&transaction,
					"objective",
					objective_id.as_str(),
					record.objective.revision(),
					event_kind,
					command,
					serde_json::json!({"project_id":record.objective.project_id().as_str(),"state":record.objective.state().as_str()}),
				)
				.await?;
			}

			objective_response(&record)?
		} else {
			error_response(result.expect_err("checked error"), &descriptor)
		};

		finish_program_command(&transaction, &reservation, &response).await?;

		transaction.commit().await?;

		objective_result_from_response(response)
	}
}

fn program_from_row(row: Row) -> Result<ProgramRecord, StoreError> {
	let project_id = ProjectId::new(row.get::<_, String>(1)).map_err(domain_error)?;
	let policy_revision_id = PolicyRevisionId::new(
		project_id.clone(),
		PolicyId::new(row.get::<_, String>(6)).map_err(domain_error)?,
		PolicyRevision::new(to_u64(row.get(7))?).map_err(domain_error)?,
	);
	let metrics = serde_json::from_value(row.get(10))
		.map_err(|_| StoreError::Incompatible("stored Program metrics are invalid".into()))?;
	let signals = serde_json::from_value(row.get(11))
		.map_err(|_| StoreError::Incompatible("stored Program signals are invalid".into()))?;
	let program = Program::from_stored(
		ProgramId::new(row.get::<_, String>(0)).map_err(domain_error)?,
		project_id,
		AgentId::new(row.get::<_, String>(2)).map_err(domain_error)?,
		row.get(3),
		row.get(4),
		program_state(row.get(5))?,
		policy_revision_id,
		ReviewCadence::new(
			u16::try_from(row.get::<_, i32>(8)).map_err(|_| incompatible())?,
			ProgramTimestamp::from_unix_microseconds(row.get(9)).map_err(domain_error)?,
		)
		.map_err(domain_error)?,
		metrics,
		signals,
		to_u64(row.get(12))?,
	)
	.map_err(domain_error)?;
	let last_change = provenance_from_values(row.get(13), row.get(14), row.get(15))?;

	Ok(ProgramRecord { program, last_change })
}

fn objective_from_row(row: Row) -> Result<ObjectiveRecord, StoreError> {
	let objective_id = ObjectiveId::new(row.get::<_, String>(0)).map_err(domain_error)?;
	let project_id = ProjectId::new(row.get::<_, String>(1)).map_err(domain_error)?;
	let completion = row
		.get::<_, Option<String>>(13)
		.map(|evidence_id| {
			ObjectiveCompletionEvidence::from_stored(
				ObjectiveEvidenceId::new(evidence_id).map_err(domain_error)?,
				objective_id.clone(),
				project_id.clone(),
				to_u64(row.get::<_, Option<i64>>(14).ok_or_else(incompatible)?)?,
				Some(
					ProgramTimestamp::from_unix_microseconds(
						row.get::<_, Option<i64>>(15).ok_or_else(incompatible)?,
					)
					.map_err(domain_error)?,
				),
				row.get::<_, Option<String>>(16).ok_or_else(incompatible)?,
				AgentId::new(row.get::<_, Option<String>>(17).ok_or_else(incompatible)?)
					.map_err(domain_error)?,
				ProgramTimestamp::from_unix_microseconds(
					row.get::<_, Option<i64>>(18).ok_or_else(incompatible)?,
				)
				.map_err(domain_error)?,
				row.get::<_, Option<String>>(19).ok_or_else(incompatible)?,
				row.get::<_, Option<String>>(20).ok_or_else(incompatible)?,
				AgentId::new(row.get::<_, Option<String>>(21).ok_or_else(incompatible)?)
					.map_err(domain_error)?,
				ProgramTimestamp::from_unix_microseconds(
					row.get::<_, Option<i64>>(22).ok_or_else(incompatible)?,
				)
				.map_err(domain_error)?,
				row.get::<_, Option<String>>(23).ok_or_else(incompatible)?,
				ProgramCorrelationId::new(
					row.get::<_, Option<String>>(24).ok_or_else(incompatible)?,
				)
				.map_err(domain_error)?,
				ProgramTimestamp::from_unix_microseconds(
					row.get::<_, Option<i64>>(25).ok_or_else(incompatible)?,
				)
				.map_err(domain_error)?,
			)
			.map_err(domain_error)
		})
		.transpose()?;
	let objective = Objective::from_stored(
		objective_id,
		project_id,
		row.get::<_, Option<String>>(2).map(ProgramId::new).transpose().map_err(domain_error)?,
		row.get(3),
		row.get(4),
		row.get(5),
		ProgramTimestamp::from_unix_microseconds(row.get(6)).map_err(domain_error)?,
		objective_state(row.get(7))?,
		to_u64(row.get(8))?,
		completion,
	)
	.map_err(domain_error)?;
	let last_change = provenance_from_values(row.get(10), row.get(11), row.get(12))?;

	Ok(ObjectiveRecord { objective, last_change })
}

fn provenance_from_values(
	actor_id: String,
	correlation_id: String,
	summary: String,
) -> Result<ProgramProvenance, StoreError> {
	ProgramProvenance::new(
		AgentId::new(actor_id).map_err(domain_error)?,
		ProgramCorrelationId::new(correlation_id).map_err(domain_error)?,
		summary,
	)
	.map_err(domain_error)
}

fn program_response(record: &ProgramRecord) -> Result<Value, StoreError> {
	Ok(serde_json::json!({
		"kind":"program","result":"ok","program_id":record.program.id().as_str(),
		"project_id":record.program.project_id().as_str(),
		"owner_agent_id":record.program.owner_agent_id().as_str(),"name":record.program.name(),
		"responsibility":record.program.responsibility(),"state":record.program.state().as_str(),
		"policy_id":record.program.policy_revision_id().policy_id().as_str(),
		"policy_revision":record.program.policy_revision_id().revision().get(),
		"review_interval_days":record.program.review_cadence().interval_days(),
		"next_review_at_microseconds":record.program.review_cadence().next_review_at().unix_microseconds(),
		"metrics":serde_json::to_value(record.program.metrics()).map_err(|_| incompatible())?,
		"signals":serde_json::to_value(record.program.signals()).map_err(|_| incompatible())?,
		"revision":record.program.revision(),"last_changed_by":record.last_change.actor_id().as_str(),
		"last_correlation_id":record.last_change.correlation_id().as_str(),
		"last_provenance":record.last_change.summary()
	}))
}

fn objective_response(record: &ObjectiveRecord) -> Result<Value, StoreError> {
	let completion = record.objective.completion().map(|evidence| {
		let objective_updated_at = evidence.objective_updated_at().ok_or_else(incompatible)?;

		Ok::<_, StoreError>(serde_json::json!({
			"evidence_id":evidence.id().as_str(),"objective_revision":evidence.objective_revision(),
			"objective_updated_at_microseconds":objective_updated_at.unix_microseconds(),
			"acceptance_result":evidence.acceptance_result(),"accepted_by":evidence.accepted_by().as_str(),
			"accepted_at_microseconds":evidence.accepted_at().unix_microseconds(),
			"acceptance_provenance":evidence.acceptance_provenance(),
			"validation_result":evidence.validation_result(),"validated_by":evidence.validated_by().as_str(),
			"validated_at_microseconds":evidence.validated_at().unix_microseconds(),
			"validation_provenance":evidence.validation_provenance(),
			"correlation_id":evidence.correlation_id().as_str(),
			"recorded_at_microseconds":evidence.recorded_at().unix_microseconds()
		}))
	}).transpose()?;

	Ok(serde_json::json!({
		"kind":"objective","result":"ok","objective_id":record.objective.id().as_str(),
		"project_id":record.objective.project_id().as_str(),
		"program_id":record.objective.program_id().map(ProgramId::as_str),
		"outcome":record.objective.outcome(),"acceptance_criteria":record.objective.acceptance_criteria(),
		"validation_criteria":record.objective.validation_criteria(),
		"target_at_microseconds":record.objective.target_at().unix_microseconds(),
		"state":record.objective.state().as_str(),"revision":record.objective.revision(),
		"completion":completion,"last_changed_by":record.last_change.actor_id().as_str(),
		"last_correlation_id":record.last_change.correlation_id().as_str(),
		"last_provenance":record.last_change.summary()
	}))
}

fn program_result_from_response(response: Value) -> Result<ProgramRecord, StoreError> {
	if let Some(error) = rejection_from_response(&response)? {
		return Err(error);
	}

	if required_str(&response, "kind")? != "program" {
		return Err(StoreError::IdempotencyConflict);
	}

	let project_id =
		ProjectId::new(required_str(&response, "project_id")?).map_err(domain_error)?;
	let policy_revision_id = PolicyRevisionId::new(
		project_id.clone(),
		PolicyId::new(required_str(&response, "policy_id")?).map_err(domain_error)?,
		PolicyRevision::new(required_u64(&response, "policy_revision")?).map_err(domain_error)?,
	);
	let program = Program::from_stored(
		ProgramId::new(required_str(&response, "program_id")?).map_err(domain_error)?,
		project_id,
		AgentId::new(required_str(&response, "owner_agent_id")?).map_err(domain_error)?,
		required_str(&response, "name")?.into(),
		required_str(&response, "responsibility")?.into(),
		program_state(required_str(&response, "state")?)?,
		policy_revision_id,
		ReviewCadence::new(
			u16::try_from(required_u64(&response, "review_interval_days")?)
				.map_err(|_| incompatible())?,
			ProgramTimestamp::from_unix_microseconds(required_i64(
				&response,
				"next_review_at_microseconds",
			)?)
			.map_err(domain_error)?,
		)
		.map_err(domain_error)?,
		serde_json::from_value(required(&response, "metrics")?.clone())
			.map_err(|_| incompatible())?,
		serde_json::from_value(required(&response, "signals")?.clone())
			.map_err(|_| incompatible())?,
		required_u64(&response, "revision")?,
	)
	.map_err(domain_error)?;
	let last_change = provenance_from_values(
		required_str(&response, "last_changed_by")?.into(),
		required_str(&response, "last_correlation_id")?.into(),
		required_str(&response, "last_provenance")?.into(),
	)?;

	Ok(ProgramRecord { program, last_change })
}

fn objective_result_from_response(response: Value) -> Result<ObjectiveRecord, StoreError> {
	if let Some(error) = rejection_from_response(&response)? {
		return Err(error);
	}

	if required_str(&response, "kind")? != "objective" {
		return Err(StoreError::IdempotencyConflict);
	}

	let objective_id =
		ObjectiveId::new(required_str(&response, "objective_id")?).map_err(domain_error)?;
	let project_id =
		ProjectId::new(required_str(&response, "project_id")?).map_err(domain_error)?;
	let completion = response
		.get("completion")
		.filter(|value| !value.is_null())
		.map(|value| {
			ObjectiveCompletionEvidence::from_stored(
				ObjectiveEvidenceId::new(required_str(value, "evidence_id")?)
					.map_err(domain_error)?,
				objective_id.clone(),
				project_id.clone(),
				required_u64(value, "objective_revision")?,
				Some(
					ProgramTimestamp::from_unix_microseconds(required_i64(
						value,
						"objective_updated_at_microseconds",
					)?)
					.map_err(domain_error)?,
				),
				required_str(value, "acceptance_result")?.into(),
				AgentId::new(required_str(value, "accepted_by")?).map_err(domain_error)?,
				ProgramTimestamp::from_unix_microseconds(required_i64(
					value,
					"accepted_at_microseconds",
				)?)
				.map_err(domain_error)?,
				required_str(value, "acceptance_provenance")?.into(),
				required_str(value, "validation_result")?.into(),
				AgentId::new(required_str(value, "validated_by")?).map_err(domain_error)?,
				ProgramTimestamp::from_unix_microseconds(required_i64(
					value,
					"validated_at_microseconds",
				)?)
				.map_err(domain_error)?,
				required_str(value, "validation_provenance")?.into(),
				ProgramCorrelationId::new(required_str(value, "correlation_id")?)
					.map_err(domain_error)?,
				ProgramTimestamp::from_unix_microseconds(required_i64(
					value,
					"recorded_at_microseconds",
				)?)
				.map_err(domain_error)?,
			)
			.map_err(domain_error)
		})
		.transpose()?;
	let objective = Objective::from_stored(
		objective_id,
		project_id,
		response
			.get("program_id")
			.and_then(Value::as_str)
			.map(ProgramId::new)
			.transpose()
			.map_err(domain_error)?,
		required_str(&response, "outcome")?.into(),
		serde_json::from_value(required(&response, "acceptance_criteria")?.clone())
			.map_err(|_| incompatible())?,
		serde_json::from_value(required(&response, "validation_criteria")?.clone())
			.map_err(|_| incompatible())?,
		ProgramTimestamp::from_unix_microseconds(required_i64(
			&response,
			"target_at_microseconds",
		)?)
		.map_err(domain_error)?,
		objective_state(required_str(&response, "state")?)?,
		required_u64(&response, "revision")?,
		completion,
	)
	.map_err(domain_error)?;
	let last_change = provenance_from_values(
		required_str(&response, "last_changed_by")?.into(),
		required_str(&response, "last_correlation_id")?.into(),
		required_str(&response, "last_provenance")?.into(),
	)?;

	Ok(ObjectiveRecord { objective, last_change })
}

fn command_result(
	row: &Row,
	descriptor: &ProgramCommandDescriptor,
) -> Result<Result<(), Rejection>, StoreError> {
	let code = row.get::<_, &str>(0);
	let actual = row.get::<_, Option<i64>>(1);

	Ok(match code {
		"ok" => Ok(()),
		"revision_conflict" => Err(Rejection::RevisionConflict {
			entity: descriptor.entity_id.clone(),
			expected: descriptor.expected_revision,
			actual,
		}),
		"not_found" => Err(Rejection::NotFound),
		"invalid_authority" => Err(Rejection::InvalidAuthority),
		"invalid_policy" => Err(Rejection::InvalidPolicy),
		"invalid_program" => Err(Rejection::InvalidProgram),
		"invalid_horizon" => Err(Rejection::InvalidHorizon),
		"invalid_transition" => Err(Rejection::InvalidTransition),
		"invalid_evidence" => Err(Rejection::InvalidEvidence),
		"invalid_project" => Err(Rejection::InvalidProject),
		"conflicting_identity" => Err(Rejection::ConflictingIdentity),
		_ => return Err(incompatible()),
	})
}

fn error_response(error: Rejection, descriptor: &ProgramCommandDescriptor) -> Value {
	match error {
		Rejection::RevisionConflict { entity, expected, actual } => serde_json::json!({
			"kind":"program_error","code":"revision_conflict","entity":entity,
			"expected":expected,"actual":actual
		}),
		other => serde_json::json!({
			"kind":"program_error","code":match other {
				Rejection::NotFound => "not_found",
				Rejection::InvalidAuthority => "invalid_authority",
				Rejection::InvalidPolicy => "invalid_policy",
				Rejection::InvalidProgram => "invalid_program",
				Rejection::InvalidHorizon => "invalid_horizon",
				Rejection::InvalidTransition => "invalid_transition",
				Rejection::InvalidEvidence => "invalid_evidence",
				Rejection::InvalidProject => "invalid_project",
				Rejection::ConflictingIdentity => "conflicting_identity",
				Rejection::RevisionConflict { .. } => unreachable!(),
			},
			"entity":descriptor.entity_id
		}),
	}
}

fn rejection_from_response(value: &Value) -> Result<Option<StoreError>, StoreError> {
	if value.get("kind").and_then(Value::as_str) != Some("program_error") {
		return Ok(None);
	}

	let error = match required_str(value, "code")? {
		"revision_conflict" => StoreError::RevisionConflict {
			entity: required_str(value, "entity")?.into(),
			expected: optional_i64(value, "expected")?,
			actual: optional_i64(value, "actual")?,
		},
		"not_found" => StoreError::RevisionConflict {
			entity: required_str(value, "entity")?.into(),
			expected: None,
			actual: None,
		},
		"invalid_authority" => StoreError::InvalidInput(
			"Program/Objective mutation requires active same-Project Lead authority",
		),
		"invalid_policy" => StoreError::InvalidInput(
			"Program requires an exact accepted same-Project Policy revision",
		),
		"invalid_program" =>
			StoreError::InvalidInput("Objective Program link is not same-Project authority"),
		"invalid_horizon" =>
			StoreError::InvalidInput("Objective target must be a future finite horizon"),
		"invalid_transition" =>
			StoreError::InvalidInput("Program/Objective lifecycle transition is invalid"),
		"invalid_evidence" => StoreError::InvalidInput(
			"Objective achievement requires chronological acceptance and validation evidence",
		),
		"invalid_project" => StoreError::InvalidInput(
			"Program/Objective request Project does not match stored authority",
		),
		"conflicting_identity" => StoreError::IdempotencyConflict,
		_ => return Err(incompatible()),
	};

	Ok(Some(error))
}

fn program_state(value: &str) -> Result<ProgramState, StoreError> {
	match value {
		"active" => Ok(ProgramState::Active),
		"needs_attention" => Ok(ProgramState::NeedsAttention),
		"blocked" => Ok(ProgramState::Blocked),
		"paused" => Ok(ProgramState::Paused),
		"retired" => Ok(ProgramState::Retired),
		_ => Err(incompatible()),
	}
}

const fn program_state_text(value: ProgramState) -> &'static str {
	value.as_str()
}

fn objective_state(value: &str) -> Result<ObjectiveState, StoreError> {
	match value {
		"proposed" => Ok(ObjectiveState::Proposed),
		"active" => Ok(ObjectiveState::Active),
		"blocked" => Ok(ObjectiveState::Blocked),
		"achieved" => Ok(ObjectiveState::Achieved),
		"abandoned" => Ok(ObjectiveState::Abandoned),
		_ => Err(incompatible()),
	}
}

const fn objective_state_text(value: ObjectiveState) -> &'static str {
	value.as_str()
}

fn required<'a>(value: &'a Value, key: &str) -> Result<&'a Value, StoreError> {
	value.get(key).ok_or_else(incompatible)
}

fn required_str<'a>(value: &'a Value, key: &str) -> Result<&'a str, StoreError> {
	required(value, key)?.as_str().ok_or_else(incompatible)
}

fn required_i64(value: &Value, key: &str) -> Result<i64, StoreError> {
	required(value, key)?.as_i64().ok_or_else(incompatible)
}

fn required_u64(value: &Value, key: &str) -> Result<u64, StoreError> {
	required(value, key)?.as_u64().ok_or_else(incompatible)
}

fn optional_i64(value: &Value, key: &str) -> Result<Option<i64>, StoreError> {
	match required(value, key)? {
		Value::Null => Ok(None),
		value => value.as_i64().map(Some).ok_or_else(incompatible),
	}
}

fn to_i64(value: u64) -> Result<i64, StoreError> {
	i64::try_from(value).map_err(|_| {
		StoreError::InvalidInput("Program/Objective revision exceeds PostgreSQL bigint")
	})
}

fn to_u64(value: i64) -> Result<u64, StoreError> {
	u64::try_from(value).map_err(|_| incompatible())
}

fn domain_error(error: impl Display) -> StoreError {
	StoreError::Incompatible(format!("invalid stored Program/Objective authority: {error}"))
}

fn incompatible() -> StoreError {
	StoreError::Incompatible("invalid stored Program/Objective authority".into())
}

async fn reserve_program_command(
	transaction: &Transaction<'_>,
	command: &CommandIdentity,
	descriptor: &ProgramCommandDescriptor,
) -> Result<ProgramCommandClaim, StoreError> {
	let inserted = transaction
		.query_opt(
			"INSERT INTO decodex.command_receipts\
			 (idempotency_key,request_hash,protocol_version,operation,project_scope,scope_id,\
			 entity_id,expected_revision,receipt_state,claim_token,claim_expires_at)\
			 VALUES ($1,$2,'decodex/store-command/1',$3,'project',$4,$5,$6,'pending',\
			 pg_catalog.gen_random_uuid(),pg_catalog.clock_timestamp()+interval '5 minutes')\
			 ON CONFLICT DO NOTHING RETURNING claim_token::text",
			&[
				&command.key,
				&command.request_hash,
				&descriptor.operation,
				&descriptor.project_id,
				&descriptor.entity_id,
				&descriptor.expected_revision,
			],
		)
		.await?;

	if let Some(row) = inserted {
		return Ok(ProgramCommandClaim::Owned(ProgramCommandReservation {
			key: command.key.clone(),
			request_hash: command.request_hash.clone(),
			claim_token: row.get(0),
		}));
	}

	let row = transaction
		.query_one(
			"SELECT request_hash,operation,project_scope,scope_id,entity_id,expected_revision,\
			 receipt_state::text,response,response_bytes FROM decodex.command_receipts \
			 WHERE idempotency_key=$1 FOR UPDATE",
			&[&command.key],
		)
		.await?;

	if row.get::<_, String>(0) != command.request_hash
		|| row.get::<_, String>(1) != descriptor.operation
		|| row.get::<_, String>(2) != "project"
		|| row.get::<_, String>(3) != descriptor.project_id
		|| row.get::<_, String>(4) != descriptor.entity_id
		|| row.get::<_, Option<i64>>(5) != descriptor.expected_revision
	{
		return Err(StoreError::IdempotencyConflict);
	}
	if row.get::<_, &str>(6) != "completed" {
		return Err(StoreError::Incompatible(
			"Program/Objective command exposed a committed pending receipt".into(),
		));
	}

	let response: Value = row.get(7);
	let response_bytes = row.get::<_, Option<Vec<u8>>>(8).ok_or_else(incompatible)?;
	let decoded: Value = serde_json::from_slice(&response_bytes).map_err(|_| incompatible())?;

	if response != decoded {
		return Err(StoreError::Incompatible(
			"Program/Objective receipt bytes differ from its response".into(),
		));
	}

	Ok(ProgramCommandClaim::Completed(response))
}

async fn finish_program_command(
	transaction: &Transaction<'_>,
	reservation: &ProgramCommandReservation,
	response: &Value,
) -> Result<(), StoreError> {
	let response_bytes = serde_json::to_vec(response)
		.map_err(|_| StoreError::InvalidInput("Program command response cannot be serialized"))?;
	let updated = transaction
		.execute(
			"UPDATE decodex.command_receipts SET response=$2,response_bytes=$3,\
			 receipt_state='completed',completed_at=pg_catalog.clock_timestamp(),\
			 completion_claim_token=$5::text::uuid,claim_token=NULL,claim_expires_at=NULL \
			 WHERE idempotency_key=$1 AND request_hash=$4 AND receipt_state='pending' \
			 AND claim_token=$5::text::uuid AND claim_expires_at>pg_catalog.clock_timestamp()",
			&[
				&reservation.key,
				response,
				&response_bytes,
				&reservation.request_hash,
				&reservation.claim_token,
			],
		)
		.await?;

	if updated == 1 {
		Ok(())
	} else {
		Err(StoreError::OwnershipLost("Program/Objective command receipt"))
	}
}

async fn append_program_activity(
	transaction: &Transaction<'_>,
	aggregate_kind: &str,
	aggregate_id: &str,
	revision: u64,
	event_kind: &str,
	command: &CommandIdentity,
	payload: Value,
) -> Result<(), StoreError> {
	accounts::append_activity_and_outbox(
		transaction,
		aggregate_kind,
		aggregate_id,
		to_i64(revision)?,
		event_kind,
		&command.key,
		&payload,
	)
	.await
}

async fn read_program(
	transaction: &Transaction<'_>,
	id: &ProgramId,
) -> Result<Option<ProgramRecord>, StoreError> {
	transaction.query_opt(PROGRAM_SELECT, &[&id.as_str()]).await?.map(program_from_row).transpose()
}

async fn read_objective(
	transaction: &Transaction<'_>,
	id: &ObjectiveId,
) -> Result<Option<ObjectiveRecord>, StoreError> {
	transaction
		.query_opt(OBJECTIVE_SELECT, &[&id.as_str()])
		.await?
		.map(objective_from_row)
		.transpose()
}

#[cfg(test)]
mod tests {
	use decodex_core::{ObjectiveState, ProgramState};

	#[test]
	fn closed_state_decoders_refuse_unknown_values() {
		assert_eq!(super::program_state("needs_attention").unwrap(), ProgramState::NeedsAttention);
		assert_eq!(super::objective_state("achieved").unwrap(), ObjectiveState::Achieved);
		assert!(super::program_state("complete").is_err());
		assert!(super::objective_state("perpetual").is_err());
	}
}
