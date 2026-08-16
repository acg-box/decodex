//! SQLite owner for the bounded Adaptive Factory Spine V1 semantic cycle.

use std::{
	collections::HashSet,
	path::{Component, Path},
};

use decodex_core::{
	ConversationId, ObjectiveId, ObjectiveState, ProgramClaimId, ProgramEvidenceId,
	ProgramEvidenceKind, ProgramId, ProgramObservationId, ProgramProposalId,
	ProgramReviewClassification, ProgramReviewId, ProgramState, WorkItemId, WorkItemState,
	MAX_PROGRAM_PROJECTION_NODES, contains_credential_material,
};
use rusqlite::{Connection, OptionalExtension as _, Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

use crate::{CommandIdentity, SqliteStore, StoreError};

const CREATE_OPERATION: &str = "create_program_cycle";
const BIND_PACK_OPERATION: &str = "bind_program_domain_pack";
const CONTINUE_OPERATION: &str = "continue_program";
const REVIEW_OPERATION: &str = "record_program_review";
const MAX_LIST_ITEMS: usize = 32;
const MAX_TEXT_BYTES: usize = 4_096;
const MAX_INSTRUCTION_BYTES: usize = 16_384;
const COMPLETE_PROGRAM_CYCLE_NODE_COST: usize = 9;

/// Complete pre-execution semantic chain created atomically for V1.
#[derive(Clone, Debug)]
pub struct CreateProgramCycle {
	pub program_id: ProgramId,
	pub domain_pack: Option<DomainPackIdentity>,
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

/// Exact identity selected from the daemon-owned built-in Domain Pack registry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DomainPackIdentity {
	pub pack_id: String,
	pub pack_version: String,
	pub pack_digest: String,
}

/// Exact immutable identity of one built-in Domain Pack bound to a Program.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ProgramDomainPackBinding {
	pub pack_id: String,
	pub pack_version: String,
	pub pack_digest: String,
	pub bound_at_micros: i64,
}

/// One exact first binding for an existing legacy Program.
#[derive(Clone, Debug)]
pub struct BindProgramDomainPack {
	pub program_id: ProgramId,
	pub expected_revision: u64,
	pub domain_pack: DomainPackIdentity,
}

/// WorkItem ownership and optional immutable Domain Pack binding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramWorkItemDomainPack {
	pub program_id: ProgramId,
	pub domain_pack: Option<ProgramDomainPackBinding>,
}

/// One exact next semantic cycle appended to an existing reviewed Program.
#[derive(Clone, Debug)]
pub struct ContinueProgram {
	pub program_id: ProgramId,
	pub predecessor_review_id: ProgramReviewId,
	pub expected_revision: u64,
	pub signal_id: ProgramObservationId,
	pub claim_id: ProgramClaimId,
	pub proposal_id: ProgramProposalId,
	pub objective_id: ObjectiveId,
	pub work_item_id: WorkItemId,
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
	pub predecessor_review_id: Option<ProgramReviewId>,
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
	pub domain_pack: Option<ProgramDomainPackBinding>,
	pub signals: Vec<ProgramSignalRecord>,
	pub claims: Vec<ProgramClaimRecord>,
	pub proposals: Vec<ProgramProposalRecord>,
	pub objectives: Vec<ProgramObjectiveRecord>,
	pub work_items: Vec<ProgramWorkItemRecord>,
	pub evidence: Vec<ProgramEvidenceRecord>,
	pub reviews: Vec<ProgramReviewRecord>,
}

struct ProgramStep<'a> {
	program_id: &'a ProgramId,
	predecessor_review_id: Option<&'a ProgramReviewId>,
	signal_id: &'a ProgramObservationId,
	claim_id: &'a ProgramClaimId,
	proposal_id: &'a ProgramProposalId,
	objective_id: &'a ObjectiveId,
	work_item_id: &'a WorkItemId,
	signal_source: &'a str,
	signal_summary: &'a str,
	signal_observed_at_micros: i64,
	claim_statement: &'a str,
	proposal_summary: &'a str,
	proposal_expected_effect: &'a str,
	proposal_risk: &'a str,
	proposal_evidence_need: &'a str,
	objective_outcome: &'a str,
	acceptance_criteria: &'a [String],
	validation_criteria: &'a [String],
	work_item_title: &'a str,
	work_item_instructions: &'a str,
	working_directory: &'a str,
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
			if let Some(domain_pack) = &create.domain_pack {
				insert_domain_pack_binding(
					&transaction,
					&create.program_id,
					domain_pack,
					now,
				)?;
			}
				transaction
					.execute(
						"INSERT INTO program_entities (entity_id, program_id, kind)
						 VALUES (?1, ?1, 'program')",
						params![create.program_id.as_str()],
					)
					.map_err(sql_error)?;
				insert_program_step(
					&transaction,
					&ProgramStep {
						program_id: &create.program_id,
						predecessor_review_id: None,
						signal_id: &create.signal_id,
						claim_id: &create.claim_id,
						proposal_id: &create.proposal_id,
						objective_id: &create.objective_id,
						work_item_id: &create.work_item_id,
						signal_source: &create.signal_source,
						signal_summary: &create.signal_summary,
						signal_observed_at_micros: create.signal_observed_at_micros,
						claim_statement: &create.claim_statement,
						proposal_summary: &create.proposal_summary,
						proposal_expected_effect: &create.proposal_expected_effect,
						proposal_risk: &create.proposal_risk,
						proposal_evidence_need: &create.proposal_evidence_need,
						objective_outcome: &create.objective_outcome,
						acceptance_criteria: &create.acceptance_criteria,
						validation_criteria: &create.validation_criteria,
						work_item_title: &create.work_item_title,
						work_item_instructions: &create.work_item_instructions,
						working_directory: &create.working_directory,
					},
					now,
				)?;
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

	/// Bind one built-in Domain Pack to an existing Program exactly once.
	pub async fn bind_program_domain_pack(
		&self,
		command: &CommandIdentity,
		binding: &BindProgramDomainPack,
	) -> Result<ProgramCycleRecord, StoreError> {
		validate_pack_identity(&binding.domain_pack)?;
		if binding.expected_revision == 0 {
			return Err(StoreError::InvalidInput("Program revision is invalid"));
		}
		let command = command.clone();
		let binding = binding.clone();
		self.run(move |connection| {
			let transaction = connection
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(sql_error)?;
			if let Some(response) = read_receipt(
				&transaction,
				&command,
				BIND_PACK_OPERATION,
				binding.program_id.as_str(),
			)? {
				let record = serde_json::from_str(&response)
					.map_err(|_| incompatible("Program Domain Pack binding receipt"))?;
				transaction.commit().map_err(sql_error)?;
				return Ok(record);
			}
			let expected_revision = i64::try_from(binding.expected_revision)
				.map_err(|_| StoreError::InvalidInput("Program revision is invalid"))?;
			let current = transaction
				.query_row(
					"SELECT revision FROM programs WHERE program_id = ?1",
					params![binding.program_id.as_str()],
					|row| row.get::<_, i64>(0),
				)
				.optional()
				.map_err(sql_error)?;
			let Some(actual_revision) = current else {
				return Err(StoreError::InvalidInput("Program does not exist"));
			};
			if actual_revision != expected_revision {
				return Err(StoreError::RevisionConflict {
					entity: format!("program/{}", binding.program_id),
					expected: Some(expected_revision),
					actual: Some(actual_revision),
				});
			}
			let already_bound: bool = transaction
				.query_row(
					"SELECT EXISTS (
					 SELECT 1 FROM program_domain_pack_bindings WHERE program_id = ?1
					)",
					params![binding.program_id.as_str()],
					|row| row.get(0),
				)
				.map_err(sql_error)?;
			if already_bound {
				return Err(StoreError::InvalidInput("Program Domain Pack is already bound"));
			}
			let now = now_micros()?;
			insert_domain_pack_binding(
				&transaction,
				&binding.program_id,
				&binding.domain_pack,
				now,
			)?;
			let changed = transaction
				.execute(
					"UPDATE programs SET revision = revision + 1, updated_at_micros = ?3
					 WHERE program_id = ?1 AND revision = ?2",
					params![binding.program_id.as_str(), expected_revision, now],
				)
				.map_err(sql_error)?;
			if changed != 1 {
				return Err(StoreError::RevisionConflict {
					entity: format!("program/{}", binding.program_id),
					expected: Some(expected_revision),
					actual: None,
				});
			}
			let record = read_program_cycle(&transaction, &binding.program_id)?
				.ok_or_else(|| incompatible("bound Program Domain Pack"))?;
			write_receipt(
				&transaction,
				&command,
				BIND_PACK_OPERATION,
				binding.program_id.as_str(),
				&serde_json::to_string(&record)
					.map_err(|_| incompatible("Program Domain Pack binding receipt"))?,
				now,
			)?;
			transaction.commit().map_err(sql_error)?;
			Ok(record)
		})
		.await
	}

	/// Atomically append one exact next cycle to a reviewed active Program.
	pub async fn continue_program(
		&self,
		command: &CommandIdentity,
		continuation: &ContinueProgram,
	) -> Result<ProgramCycleRecord, StoreError> {
		validate_continuation(continuation)?;
		let command = command.clone();
		let continuation = continuation.clone();
		self.run(move |connection| {
			let transaction = connection
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(sql_error)?;
			if let Some(response) = read_receipt(
				&transaction,
				&command,
				CONTINUE_OPERATION,
				continuation.signal_id.as_str(),
			)? {
				let record = serde_json::from_str(&response)
					.map_err(|_| incompatible("Program continuation receipt"))?;
				transaction.commit().map_err(sql_error)?;
				return Ok(record);
			}

			let expected_revision = i64::try_from(continuation.expected_revision)
				.map_err(|_| StoreError::InvalidInput("Program revision is invalid"))?;
			let current = transaction
				.query_row(
					"SELECT state, revision FROM programs WHERE program_id = ?1",
					params![continuation.program_id.as_str()],
					|row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
				)
				.optional()
				.map_err(sql_error)?;
			let Some((state, actual_revision)) = current else {
				return Err(StoreError::InvalidInput("Program does not exist"));
			};
			if actual_revision != expected_revision {
				return Err(StoreError::RevisionConflict {
					entity: format!("program/{}", continuation.program_id),
					expected: Some(expected_revision),
					actual: Some(actual_revision),
				});
			}
			if state != "active" {
				return Err(StoreError::InvalidInput("Program is not active"));
			}
			let projection_nodes: i64 = transaction
				.query_row(
					"SELECT
					   (SELECT COUNT(*) FROM program_entities
					    WHERE program_id = ?1 AND kind != 'program')
					 + (SELECT COUNT(*) FROM program_work_item_executions AS execution
					    JOIN program_work_items AS item USING (work_item_id)
					    WHERE item.program_id = ?1)",
					params![continuation.program_id.as_str()],
					|row| row.get(0),
				)
				.map_err(sql_error)?;
			let projection_nodes = usize::try_from(projection_nodes)
				.map_err(|_| incompatible("Program projection node count"))?;
			if projection_nodes.saturating_add(COMPLETE_PROGRAM_CYCLE_NODE_COST)
				> MAX_PROGRAM_PROJECTION_NODES
			{
				return Err(StoreError::CapacityExhausted("Program projection"));
			}

			let unreviewed: i64 = transaction
				.query_row(
					"SELECT COUNT(*) FROM program_work_items AS item
					 LEFT JOIN program_reviews AS review USING (work_item_id)
					 WHERE item.program_id = ?1 AND review.review_id IS NULL",
					params![continuation.program_id.as_str()],
					|row| row.get(0),
				)
				.map_err(sql_error)?;
			if unreviewed != 0 {
				return Err(StoreError::InvalidInput(
					"Program already has an unreviewed cycle",
				));
			}
			let terminal_reviews: i64 = transaction
				.query_row(
					"SELECT COUNT(*) FROM program_reviews AS review
					 LEFT JOIN program_signals AS signal
					   ON signal.predecessor_review_id = review.review_id
					 WHERE review.program_id = ?1 AND signal.signal_id IS NULL",
					params![continuation.program_id.as_str()],
					|row| row.get(0),
				)
				.map_err(sql_error)?;
			if terminal_reviews != 1 {
				return Err(incompatible("Program terminal Review lineage"));
			}
			let predecessor_objective = transaction
				.query_row(
					"SELECT item.objective_id FROM program_reviews AS review
					 JOIN program_work_items AS item USING (work_item_id)
					 WHERE review.review_id = ?1 AND review.program_id = ?2
					   AND NOT EXISTS (
					     SELECT 1 FROM program_signals AS signal
					     WHERE signal.predecessor_review_id = review.review_id
					   )",
					params![
						continuation.predecessor_review_id.as_str(),
						continuation.program_id.as_str()
					],
					|row| row.get::<_, String>(0),
				)
				.optional()
				.map_err(sql_error)?
				.ok_or(StoreError::InvalidInput(
					"Program predecessor is not the terminal Review",
				))?;

			let now = now_micros()?;
			if continuation.signal_observed_at_micros > now {
				return Err(StoreError::InvalidInput("Signal observation is in the future"));
			}
			transaction
				.execute(
					"UPDATE program_objectives SET state = 'abandoned', revision = revision + 1,
					 updated_at_micros = ?2 WHERE objective_id = ?1 AND state = 'active'",
					params![predecessor_objective, now],
				)
				.map_err(sql_error)?;
			insert_program_step(
				&transaction,
				&ProgramStep {
					program_id: &continuation.program_id,
					predecessor_review_id: Some(&continuation.predecessor_review_id),
					signal_id: &continuation.signal_id,
					claim_id: &continuation.claim_id,
					proposal_id: &continuation.proposal_id,
					objective_id: &continuation.objective_id,
					work_item_id: &continuation.work_item_id,
					signal_source: &continuation.signal_source,
					signal_summary: &continuation.signal_summary,
					signal_observed_at_micros: continuation.signal_observed_at_micros,
					claim_statement: &continuation.claim_statement,
					proposal_summary: &continuation.proposal_summary,
					proposal_expected_effect: &continuation.proposal_expected_effect,
					proposal_risk: &continuation.proposal_risk,
					proposal_evidence_need: &continuation.proposal_evidence_need,
					objective_outcome: &continuation.objective_outcome,
					acceptance_criteria: &continuation.acceptance_criteria,
					validation_criteria: &continuation.validation_criteria,
					work_item_title: &continuation.work_item_title,
					work_item_instructions: &continuation.work_item_instructions,
					working_directory: &continuation.working_directory,
				},
				now,
			)?;
			let changed = transaction
				.execute(
					"UPDATE programs SET revision = revision + 1, updated_at_micros = ?3
					 WHERE program_id = ?1 AND revision = ?2 AND state = 'active'",
					params![continuation.program_id.as_str(), expected_revision, now],
				)
				.map_err(sql_error)?;
			if changed != 1 {
				return Err(StoreError::RevisionConflict {
					entity: format!("program/{}", continuation.program_id),
					expected: Some(expected_revision),
					actual: None,
				});
			}
			let record = read_program_cycle(&transaction, &continuation.program_id)?
				.ok_or_else(|| incompatible("continued Program"))?;
			write_receipt(
				&transaction,
				&command,
				CONTINUE_OPERATION,
				continuation.signal_id.as_str(),
				&serde_json::to_string(&record)
					.map_err(|_| incompatible("Program continuation receipt"))?,
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

	/// Read the Program owner and immutable Pack binding for one WorkItem.
	pub async fn program_domain_pack_for_work_item(
		&self,
		work_item_id: &WorkItemId,
	) -> Result<Option<ProgramWorkItemDomainPack>, StoreError> {
		let work_item_id = work_item_id.clone();
		self.run(move |connection| {
			let row = connection
				.query_row(
					"SELECT item.program_id, binding.pack_id, binding.pack_version,
					 binding.pack_digest, binding.bound_at_micros
					 FROM program_work_items AS item
					 LEFT JOIN program_domain_pack_bindings AS binding USING (program_id)
					 WHERE item.work_item_id = ?1",
					params![work_item_id.as_str()],
					|row| {
						Ok((
							row.get::<_, String>(0)?,
							row.get::<_, Option<String>>(1)?,
							row.get::<_, Option<String>>(2)?,
							row.get::<_, Option<String>>(3)?,
							row.get::<_, Option<i64>>(4)?,
						))
					},
				)
				.optional()
				.map_err(sql_error)?;
			let Some((program_id, pack_id, pack_version, pack_digest, bound_at_micros)) = row
			else {
				return Ok(None);
			};
			let program_id =
				ProgramId::new(program_id).map_err(|_| incompatible("Program identity"))?;
			let domain_pack = optional_pack_binding(
				pack_id,
				pack_version,
				pack_digest,
				bound_at_micros,
			)?;
			Ok(Some(ProgramWorkItemDomainPack { program_id, domain_pack }))
		})
		.await
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

fn insert_domain_pack_binding(
	transaction: &Transaction<'_>,
	program_id: &ProgramId,
	identity: &DomainPackIdentity,
	bound_at_micros: i64,
) -> Result<(), StoreError> {
	validate_pack_identity(identity)?;
	transaction
		.execute(
			"INSERT INTO program_domain_pack_bindings (
			 program_id, pack_id, pack_version, pack_digest, bound_at_micros
			 ) VALUES (?1, ?2, ?3, ?4, ?5)",
			params![
				program_id.as_str(),
				identity.pack_id,
				identity.pack_version,
				identity.pack_digest,
				bound_at_micros
			],
		)
		.map_err(sql_error)?;
	Ok(())
}

fn insert_program_step(
	transaction: &Transaction<'_>,
	step: &ProgramStep<'_>,
	created_at_micros: i64,
) -> Result<(), StoreError> {
	let acceptance_criteria_json = encode_list(step.acceptance_criteria)?;
	let validation_criteria_json = encode_list(step.validation_criteria)?;
	for (entity_id, kind) in [
		(step.signal_id.as_str(), "signal"),
		(step.claim_id.as_str(), "claim"),
		(step.proposal_id.as_str(), "proposal"),
		(step.objective_id.as_str(), "objective"),
		(step.work_item_id.as_str(), "work_item"),
	] {
		transaction
			.execute(
				"INSERT INTO program_entities (entity_id, program_id, kind)
				 VALUES (?1, ?2, ?3)",
				params![entity_id, step.program_id.as_str(), kind],
			)
			.map_err(sql_error)?;
	}
	transaction
		.execute(
			"INSERT INTO program_signals (
			 signal_id, program_id, predecessor_review_id, source, summary,
			 observed_at_micros, created_at_micros
			 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
			params![
				step.signal_id.as_str(),
				step.program_id.as_str(),
				step.predecessor_review_id.map(ProgramReviewId::as_str),
				step.signal_source,
				step.signal_summary,
				step.signal_observed_at_micros,
				created_at_micros
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
				step.claim_id.as_str(),
				step.program_id.as_str(),
				step.signal_id.as_str(),
				step.claim_statement,
				created_at_micros
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
				step.proposal_id.as_str(),
				step.program_id.as_str(),
				step.claim_id.as_str(),
				step.proposal_summary,
				step.proposal_expected_effect,
				step.proposal_risk,
				step.proposal_evidence_need,
				created_at_micros
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
				step.objective_id.as_str(),
				step.program_id.as_str(),
				step.proposal_id.as_str(),
				step.objective_outcome,
				acceptance_criteria_json,
				validation_criteria_json,
				created_at_micros
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
				step.work_item_id.as_str(),
				step.program_id.as_str(),
				step.objective_id.as_str(),
				step.work_item_title,
				step.work_item_instructions,
				step.working_directory,
				created_at_micros
			],
		)
		.map_err(sql_error)?;
	Ok(())
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
	if let Some(domain_pack) = &create.domain_pack {
		validate_pack_identity(domain_pack)?;
	}
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

fn validate_pack_identity(identity: &DomainPackIdentity) -> Result<(), StoreError> {
	let valid_symbol = |value: &str| {
		value.len() >= 3
			&& value.len() <= 128
			&& value.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
			&& value.as_bytes().last().is_some_and(u8::is_ascii_alphanumeric)
			&& value
				.bytes()
				.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b".-".contains(&byte))
			&& value.contains('.')
			&& !value.contains("..")
	};
	let version_parts = identity.pack_version.split('.').collect::<Vec<_>>();
	let valid_version = version_parts.len() == 3
		&& version_parts.iter().all(|part| {
			!part.is_empty()
				&& part.bytes().all(|byte| byte.is_ascii_digit())
				&& (part == &"0" || !part.starts_with('0'))
		});
	let valid_digest = identity.pack_digest.len() == 64
		&& identity
			.pack_digest
			.bytes()
			.all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'));
	if !valid_symbol(&identity.pack_id) || !valid_version || !valid_digest {
		return Err(StoreError::InvalidInput("Domain Pack identity is invalid"));
	}
	Ok(())
}

fn validate_continuation(continuation: &ContinueProgram) -> Result<(), StoreError> {
	if continuation.expected_revision == 0 {
		return Err(StoreError::InvalidInput("Program revision is invalid"));
	}
	for value in [
		&continuation.signal_source,
		&continuation.signal_summary,
		&continuation.claim_statement,
		&continuation.proposal_summary,
		&continuation.proposal_expected_effect,
		&continuation.proposal_risk,
		&continuation.proposal_evidence_need,
		&continuation.objective_outcome,
	] {
		validate_text(value, MAX_TEXT_BYTES)?;
	}
	validate_text(&continuation.work_item_title, 256)?;
	validate_text(&continuation.work_item_instructions, MAX_INSTRUCTION_BYTES)?;
	validate_list(&continuation.acceptance_criteria)?;
	validate_list(&continuation.validation_criteria)?;
	if continuation.signal_observed_at_micros <= 0
		|| !valid_absolute_path(&continuation.working_directory)
	{
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
	let domain_pack = connection
		.query_row(
			"SELECT pack_id, pack_version, pack_digest, bound_at_micros
			 FROM program_domain_pack_bindings WHERE program_id = ?1",
			params![program_id.as_str()],
			|row| {
				Ok(ProgramDomainPackBinding {
					pack_id: row.get(0)?,
					pack_version: row.get(1)?,
					pack_digest: row.get(2)?,
					bound_at_micros: row.get(3)?,
				})
			},
		)
		.optional()
		.map_err(sql_error)?;
	if let Some(binding) = &domain_pack {
		validate_persisted_pack_binding(binding)?;
	}
	let signals = query_rows(
		connection,
		"SELECT signal_id, predecessor_review_id, source, summary, observed_at_micros,
		 created_at_micros
		 FROM program_signals WHERE program_id = ?1 ORDER BY created_at_micros, signal_id",
		program_id.as_str(),
		|row| {
			Ok(ProgramSignalRecord {
				signal_id: ProgramObservationId::new(row.get::<_, String>(0)?)
					.map_err(|_| rusqlite::Error::InvalidQuery)?,
				program_id: program_id.clone(),
				predecessor_review_id: row
					.get::<_, Option<String>>(1)?
					.map(ProgramReviewId::new)
					.transpose()
					.map_err(|_| rusqlite::Error::InvalidQuery)?,
				source: row.get(2)?,
				summary: row.get(3)?,
				observed_at_micros: positive_time_sql(row.get(4)?)?,
				created_at_micros: positive_time_sql(row.get(5)?)?,
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
		domain_pack,
		signals,
		claims,
		proposals,
		objectives,
		work_items,
		evidence,
		reviews,
	}))
}

fn optional_pack_binding(
	pack_id: Option<String>,
	pack_version: Option<String>,
	pack_digest: Option<String>,
	bound_at_micros: Option<i64>,
) -> Result<Option<ProgramDomainPackBinding>, StoreError> {
	match (pack_id, pack_version, pack_digest, bound_at_micros) {
		(None, None, None, None) => Ok(None),
		(Some(pack_id), Some(pack_version), Some(pack_digest), Some(bound_at_micros)) => {
			let binding = ProgramDomainPackBinding {
				pack_id,
				pack_version,
				pack_digest,
				bound_at_micros,
			};
			validate_persisted_pack_binding(&binding)?;
			Ok(Some(binding))
		},
		_ => Err(incompatible("Program Domain Pack binding")),
	}
}

fn validate_persisted_pack_binding(
	binding: &ProgramDomainPackBinding,
) -> Result<(), StoreError> {
	validate_pack_identity(&DomainPackIdentity {
		pack_id: binding.pack_id.clone(),
		pack_version: binding.pack_version.clone(),
		pack_digest: binding.pack_digest.clone(),
	})
	.map_err(|_| incompatible("Program Domain Pack identity"))?;
	positive_time(binding.bound_at_micros)?;
	Ok(())
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
			domain_pack: Some(DomainPackIdentity {
				pack_id: "decodex.dev".to_owned(),
				pack_version: "1.0.0".to_owned(),
				pack_digest:
					"1111111111111111111111111111111111111111111111111111111111111111"
						.to_owned(),
			}),
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

	fn continuation_fixture(
		create: &CreateProgramCycle,
		predecessor_review_id: ProgramReviewId,
		expected_revision: u64,
	) -> ContinueProgram {
		ContinueProgram {
			program_id: create.program_id.clone(),
			predecessor_review_id,
			expected_revision,
			signal_id: id("51000000-0000-4000-8000-000000000001", ProgramObservationId::new),
			claim_id: id("52000000-0000-4000-8000-000000000001", ProgramClaimId::new),
			proposal_id: id("53000000-0000-4000-8000-000000000001", ProgramProposalId::new),
			objective_id: id("54000000-0000-4000-8000-000000000001", ObjectiveId::new),
			work_item_id: WorkItemId::new("55000000-0000-4000-8000-000000000001")
				.expect("WorkItem identity"),
			signal_source: "first cycle Review".to_owned(),
			signal_summary: "The first cycle exposed the next bounded gap.".to_owned(),
			signal_observed_at_micros: 1,
			claim_statement: "A second finite cycle can close that gap.".to_owned(),
			proposal_summary: "Append one exact next semantic chain.".to_owned(),
			proposal_expected_effect: "The Program keeps one identity across cycles.".to_owned(),
			proposal_risk: "A stale client could branch the history.".to_owned(),
			proposal_evidence_need: "Restart and idempotency evidence.".to_owned(),
			objective_outcome: "Two ordered cycles survive restart.".to_owned(),
			acceptance_criteria: vec!["The first cycle remains queryable.".to_owned()],
			validation_criteria: vec!["A replay creates no second Signal.".to_owned()],
			work_item_title: "Prove repeatable Program continuation".to_owned(),
			work_item_instructions: "Implement and verify one exact next cycle.".to_owned(),
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
		assert_eq!(
			created.domain_pack.as_ref().map(|binding| binding.pack_id.as_str()),
			Some("decodex.dev")
		);
		assert_eq!(
			store
				.program_domain_pack_for_work_item(&create.work_item_id)
				.await
				.expect("read WorkItem Pack")
				.expect("WorkItem exists")
				.domain_pack
				.as_ref()
				.map(|binding| binding.pack_digest.as_str()),
			create.domain_pack.as_ref().map(|identity| identity.pack_digest.as_str())
		);
		assert_eq!(store.create_program_cycle(&command, &create).await.expect("replay"), created);
		drop(store);
		let reopened = SqliteStore::open_test(&path).expect("reopen store");
		assert_eq!(reopened.program_cycle(&create.program_id).await.expect("read"), Some(created));
	}

	#[tokio::test]
	async fn legacy_program_accepts_one_immutable_pack_binding() {
		let directory = tempdir().expect("temporary directory");
		let path = directory.path().join("decodex.sqlite3");
		let store = SqliteStore::open_test(&path).expect("initialize store");
		let mut create = create_fixture();
		create.domain_pack = None;
		store
			.create_program_cycle(
				&CommandIdentity::new("program-create-legacy", b"legacy create")
					.expect("command identity"),
				&create,
			)
			.await
			.expect("create legacy Program");
		let identity = DomainPackIdentity {
			pack_id: "decodex.dev".to_owned(),
			pack_version: "1.0.0".to_owned(),
			pack_digest:
				"2222222222222222222222222222222222222222222222222222222222222222"
					.to_owned(),
		};
		let binding = BindProgramDomainPack {
			program_id: create.program_id.clone(),
			expected_revision: 1,
			domain_pack: identity.clone(),
		};
		let command =
			CommandIdentity::new("program-bind-pack", b"bind pack").expect("command identity");
		let bound = store
			.bind_program_domain_pack(&command, &binding)
			.await
			.expect("bind Pack");
		assert_eq!(bound.program.revision, 2);
		assert_eq!(bound.domain_pack.as_ref().map(|pack| pack.pack_id.as_str()), Some("decodex.dev"));
		assert_eq!(
			store.bind_program_domain_pack(&command, &binding).await.expect("replay"),
			bound
		);
		let second = BindProgramDomainPack {
			program_id: create.program_id.clone(),
			expected_revision: 2,
			domain_pack: identity,
		};
		assert!(matches!(
			store
				.bind_program_domain_pack(
					&CommandIdentity::new("program-bind-pack-again", b"bind again")
						.expect("command identity"),
					&second,
				)
				.await,
			Err(StoreError::InvalidInput("Program Domain Pack is already bound"))
		));
		store
			.with_connection(|connection| {
				assert!(
					connection
						.execute(
							"UPDATE program_domain_pack_bindings SET pack_digest = ?2
							 WHERE program_id = ?1",
							params![
								create.program_id.as_str(),
								"3333333333333333333333333333333333333333333333333333333333333333"
							],
						)
						.is_err()
				);
				Ok(())
			})
			.expect("verify immutable trigger");
		drop(store);
		let reopened = SqliteStore::open_test(&path).expect("reopen store");
		assert_eq!(
			reopened
				.program_cycle(&create.program_id)
				.await
				.expect("read")
				.expect("Program")
				.domain_pack
				.as_ref()
				.map(|pack| pack.pack_digest.as_str()),
			Some("2222222222222222222222222222222222222222222222222222222222222222")
		);
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
	async fn reviewed_program_continues_once_and_reopens_without_replay() {
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
			classification: ProgramReviewClassification::KnowledgeProgress,
			rationale: "The first cycle found the next bounded gap.".to_owned(),
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
		assert_eq!(reviewed.objectives[0].state, ObjectiveState::Active);

		let continuation = continuation_fixture(
			&create,
			review.review_id.clone(),
			reviewed.program.revision,
		);
		let continue_command = CommandIdentity::new("program-continue-1", b"program continue")
			.expect("continuation command");
		let continued = store
			.continue_program(&continue_command, &continuation)
			.await
			.expect("continue reviewed Program");
		assert_eq!(continued.program.revision, 3);
		assert_eq!(continued.signals.len(), 2);
		assert_eq!(continued.work_items.len(), 2);
		assert_eq!(continued.objectives[0].state, ObjectiveState::Abandoned);
		assert_eq!(continued.objectives[1].state, ObjectiveState::Active);
		assert_eq!(
			continued.signals[1].predecessor_review_id.as_ref(),
			Some(&review.review_id)
		);
		assert_eq!(
			store
				.continue_program(&continue_command, &continuation)
				.await
				.expect("continuation replay"),
			continued
		);

		let mut stale = continuation.clone();
		stale.signal_id = id("61000000-0000-4000-8000-000000000001", ProgramObservationId::new);
		assert!(matches!(
			store
				.continue_program(
					&CommandIdentity::new("program-continue-stale", b"stale continue")
						.expect("stale command"),
					&stale,
				)
				.await,
			Err(StoreError::RevisionConflict { expected: Some(2), actual: Some(3), .. })
		));
		let mut parallel = stale;
		parallel.expected_revision = 3;
		assert!(matches!(
			store
				.continue_program(
					&CommandIdentity::new("program-continue-parallel", b"parallel continue")
						.expect("parallel command"),
					&parallel,
				)
				.await,
			Err(StoreError::InvalidInput("Program already has an unreviewed cycle"))
		));
		store
			.with_connection(|connection| {
				let mut insert = connection
					.prepare(
						"INSERT INTO program_entities (entity_id, program_id, kind)
						 VALUES (?1, ?2, 'evidence')",
					)
					.map_err(|_| crate::DatabaseError::Unavailable)?;
				for index in 0..106 {
					insert
						.execute(params![
							format!("90000000-0000-4000-8000-{index:012}"),
							create.program_id.as_str()
						])
						.map_err(|_| crate::DatabaseError::Unavailable)?;
				}
				Ok(())
			})
			.expect("fill the bounded projection fixture");
		assert_eq!(
			store
				.continue_program(&continue_command, &continuation)
				.await
				.expect("accepted continuation replay bypasses capacity preflight"),
			continued
		);
		assert!(matches!(
			store
				.continue_program(
					&CommandIdentity::new("program-continue-capacity", b"capacity continue")
						.expect("capacity command"),
					&parallel,
				)
				.await,
			Err(StoreError::CapacityExhausted("Program projection"))
		));
		store
			.with_connection(|connection| {
				connection
					.execute_batch(
						"DELETE FROM program_entities
						 WHERE entity_id LIKE '90000000-0000-4000-8000-%';
						 DELETE FROM provider_attempt_positive_evidence
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
			reopened.program_cycle(&create.program_id).await.expect("read continued Program"),
			Some(continued.clone())
		);
		assert_eq!(
			reopened
				.continue_program(&continue_command, &continuation)
				.await
				.expect("continuation replay after restart"),
			continued
		);
	}

	#[test]
	fn review_vocabulary_keeps_unknown_distinct() {
		assert_eq!(ProgramReviewClassification::Unknown.as_str(), "unknown");
	}
}
