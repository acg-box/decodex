//! SQLite owner for the bounded Adaptive Factory Spine V1 semantic cycle.

use std::{
	collections::HashSet,
	path::{Component, Path},
};

use decodex_core::{
	ConversationId, ObjectiveId, ObjectiveState, ProgramClaimId, ProgramEvidenceId,
	ProgramEvidenceKind, ProgramId, ProgramObservationId, ProgramProposalId,
	ProgramReviewClassification, ProgramReviewId, ProgramState, WorkItemId, WorkItemState,
	contains_credential_material,
};
use rusqlite::{Connection, OptionalExtension as _, Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

use crate::{CommandIdentity, SqliteStore, StoreError};

const CREATE_OPERATION: &str = "create_program_cycle";
const REVIEW_OPERATION: &str = "record_program_review";
const MAX_LIST_ITEMS: usize = 32;
const MAX_TEXT_BYTES: usize = 4_096;
const MAX_INSTRUCTION_BYTES: usize = 16_384;

/// Complete pre-execution semantic chain created atomically for V1.
#[derive(Clone, Debug)]
pub struct CreateProgramCycle {
	pub program_id: ProgramId,
	pub signal_id: ProgramObservationId,
	pub claim_id: ProgramClaimId,
	pub proposal_id: ProgramProposalId,
	pub objective_id: ObjectiveId,
	pub work_item_id: WorkItemId,
	pub name: String,
	pub purpose: String,
	pub non_goals: Vec<String>,
	pub review_policy: String,
	pub signal_source: String,
	pub signal_summary: String,
	pub signal_observed_at_micros: i64,
	pub claim_statement: String,
	pub proposal_summary: String,
	pub proposal_expected_effect: String,
	pub proposal_risk: String,
	pub proposal_evidence_need: String,
	pub objective_outcome: String,
	pub acceptance_criteria: Vec<String>,
	pub validation_criteria: Vec<String>,
	pub work_item_title: String,
	pub work_item_instructions: String,
	pub working_directory: String,
}

/// One proposed sourced Evidence record supplied to a Program Review transaction.
#[derive(Clone, Debug)]
pub struct ProgramEvidenceInput {
	pub evidence_id: ProgramEvidenceId,
	pub source: String,
	pub summary: String,
	pub observed_at_micros: i64,
}

/// Exact terminal review input for one executed WorkItem.
#[derive(Clone, Debug)]
pub struct RecordProgramReview {
	pub review_id: ProgramReviewId,
	pub program_id: ProgramId,
	pub work_item_id: WorkItemId,
	pub deterministic: ProgramEvidenceInput,
	pub external: ProgramEvidenceInput,
	pub classification: ProgramReviewClassification,
	pub rationale: String,
}

/// Bounded Program selector row.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ProgramSummaryRecord {
	pub program_id: ProgramId,
	pub name: String,
	pub purpose: String,
	pub state: ProgramState,
	pub revision: u64,
	pub updated_at_micros: i64,
}

/// Persisted Program charter.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ProgramCharterRecord {
	pub program_id: ProgramId,
	pub name: String,
	pub purpose: String,
	pub non_goals: Vec<String>,
	pub review_policy: String,
	pub state: ProgramState,
	pub revision: u64,
	pub created_at_micros: i64,
	pub updated_at_micros: i64,
}

/// Persisted sourced Signal.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ProgramSignalRecord {
	pub signal_id: ProgramObservationId,
	pub program_id: ProgramId,
	pub source: String,
	pub summary: String,
	pub observed_at_micros: i64,
	pub created_at_micros: i64,
}

/// Persisted revisable Claim.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ProgramClaimRecord {
	pub claim_id: ProgramClaimId,
	pub program_id: ProgramId,
	pub signal_id: ProgramObservationId,
	pub statement: String,
	pub revision: u64,
	pub created_at_micros: i64,
	pub updated_at_micros: i64,
}

/// Persisted non-executable Proposal.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ProgramProposalRecord {
	pub proposal_id: ProgramProposalId,
	pub program_id: ProgramId,
	pub claim_id: ProgramClaimId,
	pub summary: String,
	pub expected_effect: String,
	pub risk: String,
	pub evidence_need: String,
	pub revision: u64,
	pub created_at_micros: i64,
	pub updated_at_micros: i64,
}

/// Persisted finite Objective.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ProgramObjectiveRecord {
	pub objective_id: ObjectiveId,
	pub program_id: ProgramId,
	pub proposal_id: ProgramProposalId,
	pub outcome: String,
	pub acceptance_criteria: Vec<String>,
	pub validation_criteria: Vec<String>,
	pub state: ObjectiveState,
	pub revision: u64,
	pub created_at_micros: i64,
	pub updated_at_micros: i64,
}

/// Persisted WorkItem and its optional ordinary Quick Task binding.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ProgramWorkItemRecord {
	pub work_item_id: WorkItemId,
	pub program_id: ProgramId,
	pub objective_id: ObjectiveId,
	pub title: String,
	pub instructions: String,
	pub working_directory: String,
	pub state: WorkItemState,
	pub revision: u64,
	pub conversation_id: Option<ConversationId>,
	pub created_at_micros: i64,
	pub updated_at_micros: i64,
}

/// Persisted validation or external Evidence.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ProgramEvidenceRecord {
	pub evidence_id: ProgramEvidenceId,
	pub program_id: ProgramId,
	pub work_item_id: WorkItemId,
	pub kind: ProgramEvidenceKind,
	pub source: String,
	pub summary: String,
	pub observed_at_micros: i64,
	pub created_at_micros: i64,
}

/// Persisted evidence-backed Program Review.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ProgramReviewRecord {
	pub review_id: ProgramReviewId,
	pub program_id: ProgramId,
	pub work_item_id: WorkItemId,
	pub deterministic_evidence_id: ProgramEvidenceId,
	pub external_evidence_id: ProgramEvidenceId,
	pub classification: ProgramReviewClassification,
	pub rationale: String,
	pub created_at_micros: i64,
}

/// Complete causal projection for one Program.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ProgramCycleRecord {
	pub program: ProgramCharterRecord,
	pub signals: Vec<ProgramSignalRecord>,
	pub claims: Vec<ProgramClaimRecord>,
	pub proposals: Vec<ProgramProposalRecord>,
	pub objectives: Vec<ProgramObjectiveRecord>,
	pub work_items: Vec<ProgramWorkItemRecord>,
	pub evidence: Vec<ProgramEvidenceRecord>,
	pub reviews: Vec<ProgramReviewRecord>,
}

impl SqliteStore {
	/// Atomically create one complete pre-execution semantic chain.
	pub async fn create_program_cycle(
		&self,
		command: &CommandIdentity,
		create: &CreateProgramCycle,
	) -> Result<ProgramCycleRecord, StoreError> {
		validate_create(create)?;
		let command = command.clone();
		let create = create.clone();
		self.run(move |connection| {
			let transaction = connection
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(sql_error)?;
			if let Some(response) =
				read_receipt(&transaction, &command, CREATE_OPERATION, create.program_id.as_str())?
			{
				let record = serde_json::from_str(&response)
					.map_err(|_| incompatible("Program create receipt"))?;
				transaction.commit().map_err(sql_error)?;
				return Ok(record);
			}
			let exists: bool = transaction
				.query_row(
					"SELECT EXISTS (SELECT 1 FROM programs WHERE program_id = ?1)",
					params![create.program_id.as_str()],
					|row| row.get(0),
				)
				.map_err(sql_error)?;
			if exists {
				return Err(StoreError::RevisionConflict {
					entity: format!("program/{}", create.program_id),
					expected: None,
					actual: Some(1),
				});
			}
			let now = now_micros()?;
			if create.signal_observed_at_micros > now {
				return Err(StoreError::InvalidInput("Signal observation is in the future"));
			}
			let non_goals = encode_list(&create.non_goals)?;
			let acceptance = encode_list(&create.acceptance_criteria)?;
			let validation = encode_list(&create.validation_criteria)?;
			transaction
				.execute(
					"INSERT INTO programs (
				 program_id, name, purpose, non_goals_json, review_policy, state, revision,
				 created_at_micros, updated_at_micros
				 ) VALUES (?1, ?2, ?3, ?4, ?5, 'active', 1, ?6, ?6)",
					params![
						create.program_id.as_str(),
						create.name,
						create.purpose,
						non_goals,
						create.review_policy,
						now
					],
				)
				.map_err(sql_error)?;
			for (entity_id, kind) in [
				(create.program_id.as_str(), "program"),
				(create.signal_id.as_str(), "signal"),
				(create.claim_id.as_str(), "claim"),
				(create.proposal_id.as_str(), "proposal"),
				(create.objective_id.as_str(), "objective"),
				(create.work_item_id.as_str(), "work_item"),
			] {
				transaction
					.execute(
						"INSERT INTO program_entities (entity_id, program_id, kind)
					 VALUES (?1, ?2, ?3)",
						params![entity_id, create.program_id.as_str(), kind],
					)
					.map_err(sql_error)?;
			}
			transaction
				.execute(
					"INSERT INTO program_signals (
				 signal_id, program_id, source, summary, observed_at_micros, created_at_micros
				 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
					params![
						create.signal_id.as_str(),
						create.program_id.as_str(),
						create.signal_source,
						create.signal_summary,
						create.signal_observed_at_micros,
						now
					],
				)
				.map_err(sql_error)?;
			transaction
				.execute(
					"INSERT INTO program_claims (
				 claim_id, program_id, signal_id, statement, revision, created_at_micros,
				 updated_at_micros
				 ) VALUES (?1, ?2, ?3, ?4, 1, ?5, ?5)",
					params![
						create.claim_id.as_str(),
						create.program_id.as_str(),
						create.signal_id.as_str(),
						create.claim_statement,
						now
					],
				)
				.map_err(sql_error)?;
			transaction
				.execute(
					"INSERT INTO program_proposals (
				 proposal_id, program_id, claim_id, summary, expected_effect, risk, evidence_need,
				 executable, revision, created_at_micros, updated_at_micros
				 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, 1, ?8, ?8)",
					params![
						create.proposal_id.as_str(),
						create.program_id.as_str(),
						create.claim_id.as_str(),
						create.proposal_summary,
						create.proposal_expected_effect,
						create.proposal_risk,
						create.proposal_evidence_need,
						now
					],
				)
				.map_err(sql_error)?;
			transaction
				.execute(
					"INSERT INTO program_objectives (
				 objective_id, program_id, proposal_id, outcome, acceptance_criteria_json,
				 validation_criteria_json, state, revision, created_at_micros, updated_at_micros
				 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'active', 1, ?7, ?7)",
					params![
						create.objective_id.as_str(),
						create.program_id.as_str(),
						create.proposal_id.as_str(),
						create.objective_outcome,
						acceptance,
						validation,
						now
					],
				)
				.map_err(sql_error)?;
			transaction
				.execute(
					"INSERT INTO program_work_items (
				 work_item_id, program_id, objective_id, title, instructions, working_directory,
				 state, revision, created_at_micros, updated_at_micros
				 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'ready', 1, ?7, ?7)",
					params![
						create.work_item_id.as_str(),
						create.program_id.as_str(),
						create.objective_id.as_str(),
						create.work_item_title,
						create.work_item_instructions,
						create.working_directory,
						now
					],
				)
				.map_err(sql_error)?;
			let record = read_program_cycle(&transaction, &create.program_id)?
				.ok_or_else(|| incompatible("created Program cycle"))?;
			write_receipt(
				&transaction,
				&command,
				CREATE_OPERATION,
				create.program_id.as_str(),
				&serde_json::to_string(&record)
					.map_err(|_| incompatible("Program create receipt"))?,
				now,
			)?;
			transaction.commit().map_err(sql_error)?;
			Ok(record)
		})
		.await
	}

	/// Read one complete current causal Program projection.
	pub async fn program_cycle(
		&self,
		program_id: &ProgramId,
	) -> Result<Option<ProgramCycleRecord>, StoreError> {
		let program_id = program_id.clone();
		self.run(move |connection| read_program_cycle(connection, &program_id)).await
	}

	/// List a bounded most-recent-first Program selector projection.
	pub async fn list_programs(
		&self,
		limit: usize,
	) -> Result<Vec<ProgramSummaryRecord>, StoreError> {
		if limit == 0 || limit > 64 {
			return Err(StoreError::InvalidInput("Program list bound must be within 1..=64"));
		}
		self.run(move |connection| {
			let mut statement = connection
				.prepare(
					"SELECT program_id, name, purpose, state, revision, updated_at_micros
				 FROM programs ORDER BY updated_at_micros DESC, program_id DESC LIMIT ?1",
				)
				.map_err(sql_error)?;
			let rows = statement
				.query_map(params![i64::try_from(limit).unwrap_or(64)], |row| {
					Ok((
						row.get::<_, String>(0)?,
						row.get::<_, String>(1)?,
						row.get::<_, String>(2)?,
						row.get::<_, String>(3)?,
						row.get::<_, i64>(4)?,
						row.get::<_, i64>(5)?,
					))
				})
				.map_err(sql_error)?;
			rows.map(|row| {
				let (id, name, purpose, state, revision, updated) = row.map_err(sql_error)?;
				Ok(ProgramSummaryRecord {
					program_id: ProgramId::new(id).map_err(|_| incompatible("Program identity"))?,
					name,
					purpose,
					state: parse_program_state(&state)?,
					revision: positive_revision(revision)?,
					updated_at_micros: positive_time(updated)?,
				})
			})
			.collect()
		})
		.await
	}

	/// Atomically attach both required Evidence kinds and close one WorkItem review cycle.
	pub async fn record_program_review(
		&self,
		command: &CommandIdentity,
		review: &RecordProgramReview,
	) -> Result<ProgramCycleRecord, StoreError> {
		validate_review(review)?;
		let command = command.clone();
		let review = review.clone();
		self.run(move |connection| {
			let transaction = connection
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(sql_error)?;
			if let Some(response) =
				read_receipt(&transaction, &command, REVIEW_OPERATION, review.review_id.as_str())?
			{
				let record = serde_json::from_str(&response)
					.map_err(|_| incompatible("Program Review receipt"))?;
				transaction.commit().map_err(sql_error)?;
				return Ok(record);
			}
			let current = transaction
				.query_row(
					"SELECT program_id, state, revision, objective_id
				 FROM program_work_items WHERE work_item_id = ?1",
					params![review.work_item_id.as_str()],
					|row| {
						Ok((
							row.get::<_, String>(0)?,
							row.get::<_, String>(1)?,
							row.get::<_, i64>(2)?,
							row.get::<_, String>(3)?,
						))
					},
				)
				.optional()
				.map_err(sql_error)?;
			let Some((program_id, state, revision, objective_id)) = current else {
				return Err(StoreError::InvalidInput("Program WorkItem does not exist"));
			};
			if program_id != review.program_id.as_str() || state != "running" {
				return Err(StoreError::RevisionConflict {
					entity: format!("program-work-item/{}", review.work_item_id),
					expected: Some(2),
					actual: Some(revision),
				});
			}
			if !has_terminal_execution_evidence(&transaction, &review.work_item_id)? {
				return Err(StoreError::InvalidInput(
					"Program WorkItem has no settled positive provider evidence",
				));
			}
			let now = now_micros()?;
			if review.deterministic.observed_at_micros > now
				|| review.external.observed_at_micros > now
			{
				return Err(StoreError::InvalidInput("Program Evidence is in the future"));
			}
			insert_evidence(
				&transaction,
				&review,
				ProgramEvidenceKind::DeterministicValidation,
				&review.deterministic,
				now,
			)?;
			insert_evidence(
				&transaction,
				&review,
				ProgramEvidenceKind::External,
				&review.external,
				now,
			)?;
			for (entity_id, kind) in [
				(review.deterministic.evidence_id.as_str(), "evidence"),
				(review.external.evidence_id.as_str(), "evidence"),
				(review.review_id.as_str(), "review"),
			] {
				transaction
					.execute(
						"INSERT INTO program_entities (entity_id, program_id, kind)
					 VALUES (?1, ?2, ?3)",
						params![entity_id, review.program_id.as_str(), kind],
					)
					.map_err(sql_error)?;
			}
			transaction
				.execute(
					"INSERT INTO program_reviews (
				 review_id, program_id, work_item_id, deterministic_evidence_id,
				 external_evidence_id, classification, rationale, created_at_micros
				 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
					params![
						review.review_id.as_str(),
						review.program_id.as_str(),
						review.work_item_id.as_str(),
						review.deterministic.evidence_id.as_str(),
						review.external.evidence_id.as_str(),
						review.classification.as_str(),
						review.rationale,
						now
					],
				)
				.map_err(sql_error)?;
			transaction
				.execute(
					"UPDATE program_work_items SET state = 'done', revision = revision + 1,
				 updated_at_micros = ?2 WHERE work_item_id = ?1 AND state = 'running'",
					params![review.work_item_id.as_str(), now],
				)
				.map_err(sql_error)?;
			if matches!(
				review.classification,
				ProgramReviewClassification::OutcomeProgress
					| ProgramReviewClassification::CapabilityProgress
			) {
				transaction
					.execute(
						"UPDATE program_objectives SET state = 'achieved', revision = revision + 1,
					 updated_at_micros = ?2 WHERE objective_id = ?1 AND state = 'active'",
						params![objective_id, now],
					)
					.map_err(sql_error)?;
			}
			transaction
				.execute(
					"UPDATE programs SET revision = revision + 1, updated_at_micros = ?2
				 WHERE program_id = ?1",
					params![review.program_id.as_str(), now],
				)
				.map_err(sql_error)?;
			let record = read_program_cycle(&transaction, &review.program_id)?
				.ok_or_else(|| incompatible("reviewed Program cycle"))?;
			write_receipt(
				&transaction,
				&command,
				REVIEW_OPERATION,
				review.review_id.as_str(),
				&serde_json::to_string(&record)
					.map_err(|_| incompatible("Program Review receipt"))?,
				now,
			)?;
			transaction.commit().map_err(sql_error)?;
			Ok(record)
		})
		.await
	}
}

pub(crate) fn bind_program_work_item_execution(
	transaction: &Transaction<'_>,
	work_item_id: &WorkItemId,
	conversation_id: &ConversationId,
	bound_at_micros: i64,
) -> Result<(), StoreError> {
	let state = transaction
		.query_row(
			"SELECT state FROM program_work_items WHERE work_item_id = ?1",
			params![work_item_id.as_str()],
			|row| row.get::<_, String>(0),
		)
		.optional()
		.map_err(sql_error)?;
	if state.as_deref() != Some("ready") {
		return Err(StoreError::InvalidInput("Program WorkItem is not ready"));
	}
	transaction.execute(
		"INSERT INTO program_work_item_executions (work_item_id, conversation_id, bound_at_micros)
		 VALUES (?1, ?2, ?3)",
		params![work_item_id.as_str(), conversation_id.as_str(), bound_at_micros],
	).map_err(sql_error)?;
	let changed = transaction
		.execute(
			"UPDATE program_work_items SET state = 'running', revision = revision + 1,
		 updated_at_micros = ?2 WHERE work_item_id = ?1 AND state = 'ready'",
			params![work_item_id.as_str(), bound_at_micros],
		)
		.map_err(sql_error)?;
	if changed != 1 {
		return Err(incompatible("Program WorkItem execution binding"));
	}
	Ok(())
}

fn validate_create(create: &CreateProgramCycle) -> Result<(), StoreError> {
	validate_text(&create.name, 256)?;
	for value in [
		&create.purpose,
		&create.review_policy,
		&create.signal_source,
		&create.signal_summary,
		&create.claim_statement,
		&create.proposal_summary,
		&create.proposal_expected_effect,
		&create.proposal_risk,
		&create.proposal_evidence_need,
		&create.objective_outcome,
	] {
		validate_text(value, MAX_TEXT_BYTES)?;
	}
	validate_text(&create.work_item_title, 256)?;
	validate_text(&create.work_item_instructions, MAX_INSTRUCTION_BYTES)?;
	validate_list(&create.non_goals)?;
	validate_list(&create.acceptance_criteria)?;
	validate_list(&create.validation_criteria)?;
	if create.signal_observed_at_micros <= 0 || !valid_absolute_path(&create.working_directory) {
		return Err(StoreError::InvalidInput("Program time or working directory is invalid"));
	}
	Ok(())
}

fn validate_review(review: &RecordProgramReview) -> Result<(), StoreError> {
	if review.deterministic.evidence_id == review.external.evidence_id {
		return Err(StoreError::InvalidInput("Program Evidence identities must differ"));
	}
	for input in [&review.deterministic, &review.external] {
		validate_text(&input.source, MAX_TEXT_BYTES)?;
		validate_text(&input.summary, MAX_TEXT_BYTES)?;
		if input.observed_at_micros <= 0 {
			return Err(StoreError::InvalidInput("Program Evidence time is invalid"));
		}
	}
	validate_text(&review.rationale, MAX_TEXT_BYTES)
}

fn validate_text(value: &str, limit: usize) -> Result<(), StoreError> {
	if value.is_empty()
		|| value.len() > limit
		|| value.chars().any(char::is_control)
		|| contains_credential_material(value)
	{
		return Err(if contains_credential_material(value) {
			StoreError::CredentialRejected
		} else {
			StoreError::InvalidInput("Program text is invalid")
		});
	}
	Ok(())
}

fn validate_list(values: &[String]) -> Result<(), StoreError> {
	if values.is_empty() || values.len() > MAX_LIST_ITEMS {
		return Err(StoreError::InvalidInput("Program list is invalid"));
	}
	let mut unique = HashSet::with_capacity(values.len());
	for value in values {
		validate_text(value, MAX_TEXT_BYTES)?;
		if !unique.insert(value) {
			return Err(StoreError::InvalidInput("Program list contains duplicates"));
		}
	}
	Ok(())
}

fn valid_absolute_path(value: &str) -> bool {
	let path = Path::new(value);
	path.is_absolute()
		&& value.len() <= MAX_TEXT_BYTES
		&& !value.chars().any(char::is_control)
		&& path
			.components()
			.all(|component| matches!(component, Component::RootDir | Component::Normal(_)))
}

fn encode_list(values: &[String]) -> Result<String, StoreError> {
	serde_json::to_string(values).map_err(|_| StoreError::InvalidInput("Program list is invalid"))
}

fn insert_evidence(
	transaction: &Transaction<'_>,
	review: &RecordProgramReview,
	kind: ProgramEvidenceKind,
	input: &ProgramEvidenceInput,
	created_at_micros: i64,
) -> Result<(), StoreError> {
	transaction
		.execute(
			"INSERT INTO program_evidence (
		 evidence_id, program_id, work_item_id, kind, source, summary, observed_at_micros,
		 created_at_micros
		 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
			params![
				input.evidence_id.as_str(),
				review.program_id.as_str(),
				review.work_item_id.as_str(),
				kind.as_str(),
				input.source,
				input.summary,
				input.observed_at_micros,
				created_at_micros
			],
		)
		.map_err(sql_error)?;
	Ok(())
}

fn has_terminal_execution_evidence(
	transaction: &Transaction<'_>,
	work_item_id: &WorkItemId,
) -> Result<bool, StoreError> {
	transaction
		.query_row(
			"SELECT
		 EXISTS (
		   SELECT 1 FROM program_work_item_executions AS execution
		   JOIN provider_attempts AS attempt
		     ON attempt.conversation_id = execution.conversation_id
		   JOIN provider_attempt_positive_evidence AS evidence
		     ON evidence.attempt_id = attempt.attempt_id
		    AND evidence.evidence_id = attempt.terminal_evidence_id
		   WHERE execution.work_item_id = ?1
		     AND attempt.state IN ('succeeded', 'failed_definitive')
		 ) AND NOT EXISTS (
		   SELECT 1 FROM program_work_item_executions AS execution
		   JOIN provider_attempts AS attempt
		     ON attempt.conversation_id = execution.conversation_id
		   WHERE execution.work_item_id = ?1
		     AND attempt.state IN ('prepared', 'dispatch_authorized', 'unknown')
		 ) AND NOT EXISTS (
		   SELECT 1 FROM program_work_item_executions AS execution
		   JOIN turns AS turn ON turn.conversation_id = execution.conversation_id
		   WHERE execution.work_item_id = ?1 AND turn.status = 'active'
		 )",
			params![work_item_id.as_str()],
			|row| row.get(0),
		)
		.map_err(sql_error)
}

fn read_program_cycle(
	connection: &Connection,
	program_id: &ProgramId,
) -> Result<Option<ProgramCycleRecord>, StoreError> {
	let program = connection
		.query_row(
			"SELECT name, purpose, non_goals_json, review_policy, state, revision,
		 created_at_micros, updated_at_micros FROM programs WHERE program_id = ?1",
			params![program_id.as_str()],
			|row| {
				Ok((
					row.get::<_, String>(0)?,
					row.get::<_, String>(1)?,
					row.get::<_, String>(2)?,
					row.get::<_, String>(3)?,
					row.get::<_, String>(4)?,
					row.get::<_, i64>(5)?,
					row.get::<_, i64>(6)?,
					row.get::<_, i64>(7)?,
				))
			},
		)
		.optional()
		.map_err(sql_error)?;
	let Some((name, purpose, non_goals, review_policy, state, revision, created, updated)) =
		program
	else {
		return Ok(None);
	};
	let program = ProgramCharterRecord {
		program_id: program_id.clone(),
		name,
		purpose,
		non_goals: decode_list(&non_goals)?,
		review_policy,
		state: parse_program_state(&state)?,
		revision: positive_revision(revision)?,
		created_at_micros: positive_time(created)?,
		updated_at_micros: positive_time(updated)?,
	};
	let signals = query_rows(
		connection,
		"SELECT signal_id, source, summary, observed_at_micros, created_at_micros
		 FROM program_signals WHERE program_id = ?1 ORDER BY created_at_micros, signal_id",
		program_id.as_str(),
		|row| {
			Ok(ProgramSignalRecord {
				signal_id: ProgramObservationId::new(row.get::<_, String>(0)?)
					.map_err(|_| rusqlite::Error::InvalidQuery)?,
				program_id: program_id.clone(),
				source: row.get(1)?,
				summary: row.get(2)?,
				observed_at_micros: row.get(3)?,
				created_at_micros: row.get(4)?,
			})
		},
	)?;
	let claims = query_rows(
		connection,
		"SELECT claim_id, signal_id, statement, revision, created_at_micros, updated_at_micros
		 FROM program_claims WHERE program_id = ?1 ORDER BY created_at_micros, claim_id",
		program_id.as_str(),
		|row| {
			Ok(ProgramClaimRecord {
				claim_id: ProgramClaimId::new(row.get::<_, String>(0)?)
					.map_err(|_| rusqlite::Error::InvalidQuery)?,
				program_id: program_id.clone(),
				signal_id: ProgramObservationId::new(row.get::<_, String>(1)?)
					.map_err(|_| rusqlite::Error::InvalidQuery)?,
				statement: row.get(2)?,
				revision: positive_revision_sql(row.get(3)?)?,
				created_at_micros: positive_time_sql(row.get(4)?)?,
				updated_at_micros: positive_time_sql(row.get(5)?)?,
			})
		},
	)?;
	let proposals = query_rows(
		connection,
		"SELECT proposal_id, claim_id, summary, expected_effect, risk, evidence_need,
		 revision, created_at_micros, updated_at_micros FROM program_proposals
		 WHERE program_id = ?1 ORDER BY created_at_micros, proposal_id",
		program_id.as_str(),
		|row| {
			Ok(ProgramProposalRecord {
				proposal_id: ProgramProposalId::new(row.get::<_, String>(0)?)
					.map_err(|_| rusqlite::Error::InvalidQuery)?,
				program_id: program_id.clone(),
				claim_id: ProgramClaimId::new(row.get::<_, String>(1)?)
					.map_err(|_| rusqlite::Error::InvalidQuery)?,
				summary: row.get(2)?,
				expected_effect: row.get(3)?,
				risk: row.get(4)?,
				evidence_need: row.get(5)?,
				revision: positive_revision_sql(row.get(6)?)?,
				created_at_micros: positive_time_sql(row.get(7)?)?,
				updated_at_micros: positive_time_sql(row.get(8)?)?,
			})
		},
	)?;
	let objectives = query_rows(
		connection,
		"SELECT objective_id, proposal_id, outcome, acceptance_criteria_json,
		 validation_criteria_json, state, revision, created_at_micros, updated_at_micros
		 FROM program_objectives WHERE program_id = ?1 ORDER BY created_at_micros, objective_id",
		program_id.as_str(),
		|row| {
			let state = row.get::<_, String>(5)?;
			Ok(ProgramObjectiveRecord {
				objective_id: ObjectiveId::new(row.get::<_, String>(0)?)
					.map_err(|_| rusqlite::Error::InvalidQuery)?,
				program_id: program_id.clone(),
				proposal_id: ProgramProposalId::new(row.get::<_, String>(1)?)
					.map_err(|_| rusqlite::Error::InvalidQuery)?,
				outcome: row.get(2)?,
				acceptance_criteria: decode_list_sql(&row.get::<_, String>(3)?)?,
				validation_criteria: decode_list_sql(&row.get::<_, String>(4)?)?,
				state: parse_objective_state_sql(&state)?,
				revision: positive_revision_sql(row.get(6)?)?,
				created_at_micros: positive_time_sql(row.get(7)?)?,
				updated_at_micros: positive_time_sql(row.get(8)?)?,
			})
		},
	)?;
	let work_items = query_rows(
		connection,
		"SELECT item.work_item_id, item.objective_id, item.title, item.instructions,
		 item.working_directory, item.state, item.revision, execution.conversation_id,
		 item.created_at_micros, item.updated_at_micros FROM program_work_items AS item
		 LEFT JOIN program_work_item_executions AS execution USING (work_item_id)
		 WHERE item.program_id = ?1 ORDER BY item.created_at_micros, item.work_item_id",
		program_id.as_str(),
		|row| {
			let state = row.get::<_, String>(5)?;
			Ok(ProgramWorkItemRecord {
				work_item_id: WorkItemId::new(row.get::<_, String>(0)?)
					.map_err(|_| rusqlite::Error::InvalidQuery)?,
				program_id: program_id.clone(),
				objective_id: ObjectiveId::new(row.get::<_, String>(1)?)
					.map_err(|_| rusqlite::Error::InvalidQuery)?,
				title: row.get(2)?,
				instructions: row.get(3)?,
				working_directory: row.get(4)?,
				state: parse_work_item_state_sql(&state)?,
				revision: positive_revision_sql(row.get(6)?)?,
				conversation_id: row
					.get::<_, Option<String>>(7)?
					.map(ConversationId::new)
					.transpose()
					.map_err(|_| rusqlite::Error::InvalidQuery)?,
				created_at_micros: positive_time_sql(row.get(8)?)?,
				updated_at_micros: positive_time_sql(row.get(9)?)?,
			})
		},
	)?;
	let evidence = query_rows(
		connection,
		"SELECT evidence_id, work_item_id, kind, source, summary, observed_at_micros,
		 created_at_micros FROM program_evidence WHERE program_id = ?1
		 ORDER BY created_at_micros, evidence_id",
		program_id.as_str(),
		|row| {
			let kind = row.get::<_, String>(2)?;
			Ok(ProgramEvidenceRecord {
				evidence_id: ProgramEvidenceId::new(row.get::<_, String>(0)?)
					.map_err(|_| rusqlite::Error::InvalidQuery)?,
				program_id: program_id.clone(),
				work_item_id: WorkItemId::new(row.get::<_, String>(1)?)
					.map_err(|_| rusqlite::Error::InvalidQuery)?,
				kind: parse_evidence_kind_sql(&kind)?,
				source: row.get(3)?,
				summary: row.get(4)?,
				observed_at_micros: positive_time_sql(row.get(5)?)?,
				created_at_micros: positive_time_sql(row.get(6)?)?,
			})
		},
	)?;
	let reviews = query_rows(
		connection,
		"SELECT review_id, work_item_id, deterministic_evidence_id, external_evidence_id,
		 classification, rationale, created_at_micros FROM program_reviews
		 WHERE program_id = ?1 ORDER BY created_at_micros, review_id",
		program_id.as_str(),
		|row| {
			let classification = row.get::<_, String>(4)?;
			Ok(ProgramReviewRecord {
				review_id: ProgramReviewId::new(row.get::<_, String>(0)?)
					.map_err(|_| rusqlite::Error::InvalidQuery)?,
				program_id: program_id.clone(),
				work_item_id: WorkItemId::new(row.get::<_, String>(1)?)
					.map_err(|_| rusqlite::Error::InvalidQuery)?,
				deterministic_evidence_id: ProgramEvidenceId::new(row.get::<_, String>(2)?)
					.map_err(|_| rusqlite::Error::InvalidQuery)?,
				external_evidence_id: ProgramEvidenceId::new(row.get::<_, String>(3)?)
					.map_err(|_| rusqlite::Error::InvalidQuery)?,
				classification: parse_review_classification_sql(&classification)?,
				rationale: row.get(5)?,
				created_at_micros: positive_time_sql(row.get(6)?)?,
			})
		},
	)?;
	Ok(Some(ProgramCycleRecord {
		program,
		signals,
		claims,
		proposals,
		objectives,
		work_items,
		evidence,
		reviews,
	}))
}

fn query_rows<T>(
	connection: &Connection,
	sql: &str,
	program_id: &str,
	mut map: impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
) -> Result<Vec<T>, StoreError> {
	let mut statement = connection.prepare(sql).map_err(sql_error)?;
	let rows = statement.query_map(params![program_id], |row| map(row)).map_err(sql_error)?;
	rows.collect::<Result<Vec<_>, _>>().map_err(sql_error)
}

fn decode_list(value: &str) -> Result<Vec<String>, StoreError> {
	let values: Vec<String> =
		serde_json::from_str(value).map_err(|_| incompatible("Program list"))?;
	validate_list(&values).map_err(|_| incompatible("Program list"))?;
	Ok(values)
}

fn decode_list_sql(value: &str) -> rusqlite::Result<Vec<String>> {
	decode_list(value).map_err(|_| rusqlite::Error::InvalidQuery)
}

fn parse_program_state(value: &str) -> Result<ProgramState, StoreError> {
	match value {
		"active" => Ok(ProgramState::Active),
		"paused" => Ok(ProgramState::Paused),
		"retired" => Ok(ProgramState::Retired),
		_ => Err(incompatible("Program state")),
	}
}

fn parse_objective_state_sql(value: &str) -> rusqlite::Result<ObjectiveState> {
	match value {
		"active" => Ok(ObjectiveState::Active),
		"achieved" => Ok(ObjectiveState::Achieved),
		"abandoned" => Ok(ObjectiveState::Abandoned),
		_ => Err(rusqlite::Error::InvalidQuery),
	}
}

fn parse_work_item_state_sql(value: &str) -> rusqlite::Result<WorkItemState> {
	match value {
		"ready" => Ok(WorkItemState::Ready),
		"running" => Ok(WorkItemState::Running),
		"done" => Ok(WorkItemState::Done),
		_ => Err(rusqlite::Error::InvalidQuery),
	}
}

fn parse_evidence_kind_sql(value: &str) -> rusqlite::Result<ProgramEvidenceKind> {
	match value {
		"deterministic_validation" => Ok(ProgramEvidenceKind::DeterministicValidation),
		"external" => Ok(ProgramEvidenceKind::External),
		_ => Err(rusqlite::Error::InvalidQuery),
	}
}

fn parse_review_classification_sql(value: &str) -> rusqlite::Result<ProgramReviewClassification> {
	match value {
		"outcome_progress" => Ok(ProgramReviewClassification::OutcomeProgress),
		"knowledge_progress" => Ok(ProgramReviewClassification::KnowledgeProgress),
		"capability_progress" => Ok(ProgramReviewClassification::CapabilityProgress),
		"no_material_change" => Ok(ProgramReviewClassification::NoMaterialChange),
		"regression" => Ok(ProgramReviewClassification::Regression),
		"unknown" => Ok(ProgramReviewClassification::Unknown),
		_ => Err(rusqlite::Error::InvalidQuery),
	}
}

fn positive_revision(value: i64) -> Result<u64, StoreError> {
	u64::try_from(value)
		.ok()
		.filter(|value| *value > 0)
		.ok_or_else(|| incompatible("Program revision"))
}

fn positive_revision_sql(value: i64) -> rusqlite::Result<u64> {
	positive_revision(value).map_err(|_| rusqlite::Error::InvalidQuery)
}

fn positive_time(value: i64) -> Result<i64, StoreError> {
	(value > 0).then_some(value).ok_or_else(|| incompatible("Program timestamp"))
}

fn positive_time_sql(value: i64) -> rusqlite::Result<i64> {
	positive_time(value).map_err(|_| rusqlite::Error::InvalidQuery)
}

fn now_micros() -> Result<i64, StoreError> {
	std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.ok()
		.and_then(|duration| i64::try_from(duration.as_micros()).ok())
		.filter(|value| *value > 0)
		.ok_or(StoreError::Database(crate::DatabaseError::Unavailable))
}

fn read_receipt(
	transaction: &Transaction<'_>,
	command: &CommandIdentity,
	operation: &str,
	entity_id: &str,
) -> Result<Option<String>, StoreError> {
	let row = transaction
		.query_row(
			"SELECT request_sha256, operation, entity_id, response_json
		 FROM runtime_command_receipts WHERE idempotency_key = ?1",
			params![command.key,],
			|row| {
				Ok((
					row.get::<_, String>(0)?,
					row.get::<_, String>(1)?,
					row.get::<_, String>(2)?,
					row.get::<_, String>(3)?,
				))
			},
		)
		.optional()
		.map_err(sql_error)?;
	let Some((request_sha, stored_operation, stored_entity, response)) = row else {
		return Ok(None);
	};
	if request_sha != command.request_hash
		|| stored_operation != operation
		|| stored_entity != entity_id
	{
		return Err(StoreError::IdempotencyConflict);
	}
	Ok(Some(response))
}

fn write_receipt(
	transaction: &Transaction<'_>,
	command: &CommandIdentity,
	operation: &str,
	entity_id: &str,
	response_json: &str,
	completed_at_micros: i64,
) -> Result<(), StoreError> {
	transaction
		.execute(
			"INSERT INTO runtime_command_receipts (
		 idempotency_key, request_sha256, operation, entity_id, response_json,
		 completed_at_micros
		 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
			params![
				command.key,
				command.request_hash,
				operation,
				entity_id,
				response_json,
				completed_at_micros
			],
		)
		.map_err(sql_error)?;
	Ok(())
}

fn sql_error(_error: rusqlite::Error) -> StoreError {
	StoreError::Database(crate::DatabaseError::Unavailable)
}

fn incompatible(subject: &str) -> StoreError {
	StoreError::Incompatible(subject.to_owned())
}

#[cfg(test)]
mod tests {
	use decodex_core::{ProgramReviewClassification, WorkItemState};
	use tempfile::tempdir;

	use super::*;
	use crate::CreateQuickTaskConversation;

	fn id<T>(
		value: &str,
		parse: impl FnOnce(String) -> Result<T, decodex_core::ProgramError>,
	) -> T {
		parse(value.to_owned()).expect("fixture identity")
	}

	fn create_fixture() -> CreateProgramCycle {
		CreateProgramCycle {
			program_id: id("30000000-0000-4000-8000-000000000001", ProgramId::new),
			signal_id: id("31000000-0000-4000-8000-000000000001", ProgramObservationId::new),
			claim_id: id("32000000-0000-4000-8000-000000000001", ProgramClaimId::new),
			proposal_id: id("33000000-0000-4000-8000-000000000001", ProgramProposalId::new),
			objective_id: id("34000000-0000-4000-8000-000000000001", ObjectiveId::new),
			work_item_id: WorkItemId::new("35000000-0000-4000-8000-000000000001")
				.expect("WorkItem identity"),
			name: "Adaptive Factory Spine".to_owned(),
			purpose: "Prove one causal Program cycle.".to_owned(),
			non_goals: vec!["Do not add a plugin host.".to_owned()],
			review_policy: "Review after one settled Codex execution.".to_owned(),
			signal_source: "repository inspection".to_owned(),
			signal_summary: "Managed Factory commands are unavailable.".to_owned(),
			signal_observed_at_micros: 1,
			claim_statement: "The semantic spine is the next missing capability.".to_owned(),
			proposal_summary: "Implement one bounded semantic chain.".to_owned(),
			proposal_expected_effect: "A user can inspect why one Codex task exists.".to_owned(),
			proposal_risk: "A general workflow engine would over-expand the slice.".to_owned(),
			proposal_evidence_need: "SQLite restart and protocol round-trip tests.".to_owned(),
			objective_outcome: "One persisted Program cycle is queryable.".to_owned(),
			acceptance_criteria: vec!["The causal identities survive restart.".to_owned()],
			validation_criteria: vec!["The database test reopens the store.".to_owned()],
			work_item_title: "Implement the semantic spine".to_owned(),
			work_item_instructions: "Implement and verify the bounded SQLite owner.".to_owned(),
			working_directory: "/tmp/decodex".to_owned(),
		}
	}

	#[tokio::test]
	async fn create_cycle_is_atomic_replayable_and_restart_safe() {
		let directory = tempdir().expect("temporary directory");
		let path = directory.path().join("decodex.sqlite3");
		let store = SqliteStore::open_test(&path).expect("initialize store");
		let create = create_fixture();
		let command =
			CommandIdentity::new("program-create-1", b"program create").expect("command identity");
		let created = store.create_program_cycle(&command, &create).await.expect("create cycle");
		assert_eq!(created.work_items[0].state, WorkItemState::Ready);
		assert_eq!(created.signals.len(), 1);
		assert_eq!(store.create_program_cycle(&command, &create).await.expect("replay"), created);
		drop(store);
		let reopened = SqliteStore::open_test(&path).expect("reopen store");
		assert_eq!(reopened.program_cycle(&create.program_id).await.expect("read"), Some(created));
	}

	#[tokio::test]
	async fn create_cycle_rejects_idempotency_drift() {
		let directory = tempdir().expect("temporary directory");
		let path = directory.path().join("decodex.sqlite3");
		let store = SqliteStore::open_test(&path).expect("initialize store");
		let create = create_fixture();
		let command =
			CommandIdentity::new("program-create-1", b"program create").expect("command identity");
		store.create_program_cycle(&command, &create).await.expect("create cycle");
		let conflict = CommandIdentity::new("program-create-1", b"different request")
			.expect("conflicting identity");
		assert!(matches!(
			store.create_program_cycle(&conflict, &create).await,
			Err(StoreError::IdempotencyConflict)
		));
	}

	#[tokio::test]
	async fn quick_task_creation_binds_one_work_item_in_the_same_transaction() {
		let directory = tempdir().expect("temporary directory");
		let path = directory.path().join("decodex.sqlite3");
		let store = SqliteStore::open_test(&path).expect("initialize store");
		let create = create_fixture();
		store
			.create_program_cycle(
				&CommandIdentity::new("program-create-1", b"program create")
					.expect("command identity"),
				&create,
			)
			.await
			.expect("create cycle");
		let conversation_id = ConversationId::new("36000000-0000-4000-8000-000000000001")
			.expect("Conversation identity");
		store
			.create_quick_task_conversation(
				&CommandIdentity::new("program-quick-task-1", b"program Quick Task")
					.expect("command identity"),
				&CreateQuickTaskConversation {
					conversation_id: conversation_id.clone(),
					work_item_id: Some(create.work_item_id.clone()),
					title: "Program work".to_owned(),
					message: create.work_item_instructions.clone(),
					working_directory: create.working_directory.clone(),
					model: "gpt-5.6-sol".to_owned(),
					reasoning_effort: "high".to_owned(),
					fast: false,
				},
			)
			.await
			.expect("create bound Quick Task");
		let record = store
			.program_cycle(&create.program_id)
			.await
			.expect("read Program")
			.expect("Program exists");
		assert_eq!(record.work_items[0].state, WorkItemState::Running);
		assert_eq!(record.work_items[0].conversation_id, Some(conversation_id));
	}

	#[tokio::test]
	async fn evidence_backed_review_closes_the_cycle_and_reopens_without_replay() {
		let directory = tempdir().expect("temporary directory");
		let path = directory.path().join("decodex.sqlite3");
		let store = SqliteStore::open_test(&path).expect("initialize store");
		let create = create_fixture();
		store
			.create_program_cycle(
				&CommandIdentity::new("program-create-1", b"program create")
					.expect("command identity"),
				&create,
			)
			.await
			.expect("create cycle");
		let conversation_id = ConversationId::new("36000000-0000-4000-8000-000000000001")
			.expect("Conversation identity");
		store
			.create_quick_task_conversation(
				&CommandIdentity::new("program-quick-task-1", b"program Quick Task")
					.expect("command identity"),
				&CreateQuickTaskConversation {
					conversation_id: conversation_id.clone(),
					work_item_id: Some(create.work_item_id.clone()),
					title: "Program work".to_owned(),
					message: create.work_item_instructions.clone(),
					working_directory: create.working_directory.clone(),
					model: "gpt-5.6-sol".to_owned(),
					reasoning_effort: "high".to_owned(),
					fast: false,
				},
			)
			.await
			.expect("create bound Quick Task");

		// This unit fixture isolates the review predicate. The provider owner has its own
		// complete foreign-key and mutation tests; here we provide its exact settled public facts.
		store
			.with_connection(|connection| {
				connection
					.execute_batch(
						"PRAGMA foreign_keys = OFF;
						 INSERT INTO provider_attempts (
						 attempt_id, conversation_id, turn_id, continuation_plan_id,
						 routing_decision_id, runtime_session_id, runtime_session_revision,
						 account_id, process_generation_id, process_generation_revision,
						 execution_epoch_id, request_id, request_sha256,
						 provider_correlation_key, state, terminal_evidence_id, revision,
						 created_at_micros, updated_at_micros
						 ) VALUES (
						 '37000000-0000-4000-8000-000000000001',
						 '36000000-0000-4000-8000-000000000001',
						 '38000000-0000-4000-8000-000000000001',
						 '39000000-0000-4000-8000-000000000001',
						 '3a000000-0000-4000-8000-000000000001',
						 '3b000000-0000-4000-8000-000000000001', 1,
						 '3c000000-0000-4000-8000-000000000001',
						 '3d000000-0000-4000-8000-000000000001', 1,
						 '3e000000-0000-4000-8000-000000000001',
						 '3f000000-0000-4000-8000-000000000001',
						 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
						 'app-server:program-review-fixture', 'succeeded',
						 '40000000-0000-4000-8000-000000000001', 3, 1, 1
						 );
						 INSERT INTO provider_attempt_positive_evidence (
						 evidence_id, attempt_id, request_id, source, outcome, provider_key,
						 provider_thread_id, provider_turn_id, witness_sha256, observed_at_micros
						 ) VALUES (
						 '40000000-0000-4000-8000-000000000001',
						 '37000000-0000-4000-8000-000000000001',
						 '3f000000-0000-4000-8000-000000000001',
						 'exact_turn_readback', 'succeeded', 'app-server:program-review-fixture',
						 'fixture-thread', 'fixture-turn',
						 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb', 1
						 );
						 PRAGMA foreign_keys = ON;",
					)
					.map_err(|_| crate::DatabaseError::Unavailable)
			})
			.expect("seed settled provider facts");

		let review = RecordProgramReview {
			review_id: id("41000000-0000-4000-8000-000000000001", ProgramReviewId::new),
			program_id: create.program_id.clone(),
			work_item_id: create.work_item_id.clone(),
			deterministic: ProgramEvidenceInput {
				evidence_id: id("42000000-0000-4000-8000-000000000001", ProgramEvidenceId::new),
				source: "cargo test".to_owned(),
				summary: "Focused deterministic checks passed.".to_owned(),
				observed_at_micros: 1,
			},
			external: ProgramEvidenceInput {
				evidence_id: id("43000000-0000-4000-8000-000000000001", ProgramEvidenceId::new),
				source: "Codex app-server exact turn readback".to_owned(),
				summary: "The bound provider turn settled successfully.".to_owned(),
				observed_at_micros: 1,
			},
			classification: ProgramReviewClassification::CapabilityProgress,
			rationale: "The closed loop is now a reusable product capability.".to_owned(),
		};
		let command =
			CommandIdentity::new("program-review-1", b"program review").expect("review command");
		let reviewed = store.record_program_review(&command, &review).await.expect("record review");
		assert_eq!(reviewed.evidence.len(), 2);
		assert_eq!(reviewed.reviews.len(), 1);
		assert_eq!(reviewed.work_items[0].state, WorkItemState::Done);
		assert_eq!(
			store.record_program_review(&command, &review).await.expect("review replay"),
			reviewed
		);
		store
			.with_connection(|connection| {
				connection
					.execute_batch(
						"DELETE FROM provider_attempt_positive_evidence
						 WHERE attempt_id = '37000000-0000-4000-8000-000000000001';
						 DELETE FROM provider_attempts
						 WHERE attempt_id = '37000000-0000-4000-8000-000000000001';",
					)
					.map_err(|_| crate::DatabaseError::Unavailable)
			})
			.expect("remove isolated provider-owner fixture rows");
		drop(store);
		let reopened = SqliteStore::open_test(&path).expect("reopen store");
		assert_eq!(
			reopened.program_cycle(&create.program_id).await.expect("read reviewed cycle"),
			Some(reviewed)
		);
	}

	#[test]
	fn review_vocabulary_keeps_unknown_distinct() {
		assert_eq!(ProgramReviewClassification::Unknown.as_str(), "unknown");
	}
}
