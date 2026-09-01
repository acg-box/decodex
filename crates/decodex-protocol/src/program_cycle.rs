//! Bounded semantic Program-cycle contracts for the Adaptive Factory Spine.

use std::collections::HashSet;

pub use decodex_core::{
	MAX_PROGRAM_PROJECTION_NODES as MAX_PROGRAM_NODES, ProgramReviewClassification, ProgramState,
};
use serde::{Deserialize, Serialize};

use crate::{
	ConversationWorkingDirectory,
	domain_pack::{DomainPackProjectionDto, is_namespaced_symbol},
	wire::{EntityId, EntityRevision, MAX_WIRE_TEXT_BYTES, WireText},
};

/// Maximum Programs returned by one local selector query.
pub const MAX_PROGRAM_LIST_ITEMS: usize = 64;
/// Maximum causal edges in one Program projection.
pub const MAX_PROGRAM_EDGES: usize = 256;
/// Maximum bounded list items in one creation contract.
pub const MAX_PROGRAM_LIST_VALUES: usize = 32;

/// Closed Program-cycle contract refusal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgramCycleContractError {
	InvalidIdentity,
	InvalidText,
	InvalidCollection,
	InvalidTime,
	InvalidProjection,
}

/// One complete pre-execution semantic chain.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramCycleDraftDto {
	pub program_id: EntityId,
	pub domain_pack_id: WireText,
	pub signal_id: EntityId,
	pub claim_id: EntityId,
	pub proposal_id: EntityId,
	pub objective_id: EntityId,
	pub work_item_id: EntityId,
	pub name: WireText,
	pub purpose: WireText,
	pub non_goals: Vec<WireText>,
	pub review_policy: WireText,
	pub signal_source: WireText,
	pub signal_summary: WireText,
	pub signal_observed_at_micros: i64,
	pub claim_statement: WireText,
	pub proposal_summary: WireText,
	pub proposal_expected_effect: WireText,
	pub proposal_risk: WireText,
	pub proposal_evidence_need: WireText,
	pub objective_outcome: WireText,
	pub acceptance_criteria: Vec<WireText>,
	pub validation_criteria: Vec<WireText>,
	pub work_item_title: WireText,
	pub work_item_instructions: WireText,
	pub working_directory: ConversationWorkingDirectory,
}

impl ProgramCycleDraftDto {
	pub(crate) fn validate(&self) -> Result<(), ProgramCycleContractError> {
		let ids = [
			self.program_id.as_str(),
			self.signal_id.as_str(),
			self.claim_id.as_str(),
			self.proposal_id.as_str(),
			self.objective_id.as_str(),
			self.work_item_id.as_str(),
		];
		if ids.iter().copied().collect::<HashSet<_>>().len() != ids.len() {
			return Err(ProgramCycleContractError::InvalidIdentity);
		}
		if !is_namespaced_symbol(self.domain_pack_id.as_str()) {
			return Err(ProgramCycleContractError::InvalidText);
		}
		for value in [
			&self.name,
			&self.purpose,
			&self.review_policy,
			&self.signal_source,
			&self.signal_summary,
			&self.claim_statement,
			&self.proposal_summary,
			&self.proposal_expected_effect,
			&self.proposal_risk,
			&self.proposal_evidence_need,
			&self.objective_outcome,
			&self.work_item_title,
			&self.work_item_instructions,
		] {
			if value.as_str().is_empty() || value.as_str().chars().any(char::is_control) {
				return Err(ProgramCycleContractError::InvalidText);
			}
		}
		validate_list(&self.non_goals)?;
		validate_list(&self.acceptance_criteria)?;
		validate_list(&self.validation_criteria)?;
		validate_joined_field(&self.acceptance_criteria)?;
		validate_joined_field(&self.validation_criteria)?;
		if self.signal_observed_at_micros <= 0 {
			return Err(ProgramCycleContractError::InvalidTime);
		}
		Ok(())
	}
}

/// One exact next semantic cycle for an existing reviewed Program.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramContinuationDraftDto {
	pub program_id: EntityId,
	pub predecessor_review_id: EntityId,
	pub signal_id: EntityId,
	pub claim_id: EntityId,
	pub proposal_id: EntityId,
	pub objective_id: EntityId,
	pub work_item_id: EntityId,
	pub signal_source: WireText,
	pub signal_summary: WireText,
	pub signal_observed_at_micros: i64,
	pub claim_statement: WireText,
	pub proposal_summary: WireText,
	pub proposal_expected_effect: WireText,
	pub proposal_risk: WireText,
	pub proposal_evidence_need: WireText,
	pub objective_outcome: WireText,
	pub acceptance_criteria: Vec<WireText>,
	pub validation_criteria: Vec<WireText>,
	pub work_item_title: WireText,
	pub work_item_instructions: WireText,
	pub working_directory: ConversationWorkingDirectory,
}

impl ProgramContinuationDraftDto {
	pub(crate) fn validate(&self) -> Result<(), ProgramCycleContractError> {
		let ids = [
			self.program_id.as_str(),
			self.predecessor_review_id.as_str(),
			self.signal_id.as_str(),
			self.claim_id.as_str(),
			self.proposal_id.as_str(),
			self.objective_id.as_str(),
			self.work_item_id.as_str(),
		];
		if ids.iter().copied().collect::<HashSet<_>>().len() != ids.len() {
			return Err(ProgramCycleContractError::InvalidIdentity);
		}
		for value in [
			&self.signal_source,
			&self.signal_summary,
			&self.claim_statement,
			&self.proposal_summary,
			&self.proposal_expected_effect,
			&self.proposal_risk,
			&self.proposal_evidence_need,
			&self.objective_outcome,
			&self.work_item_title,
			&self.work_item_instructions,
		] {
			if value.as_str().is_empty() || value.as_str().chars().any(char::is_control) {
				return Err(ProgramCycleContractError::InvalidText);
			}
		}
		validate_list(&self.acceptance_criteria)?;
		validate_list(&self.validation_criteria)?;
		validate_joined_field(&self.acceptance_criteria)?;
		validate_joined_field(&self.validation_criteria)?;
		if self.signal_observed_at_micros <= 0 {
			return Err(ProgramCycleContractError::InvalidTime);
		}
		Ok(())
	}
}

/// One proposed Evidence record in a terminal Program Review command.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramEvidenceDraftDto {
	pub evidence_id: EntityId,
	pub source: WireText,
	pub summary: WireText,
	pub observed_at_micros: i64,
}

/// One terminal evidence-backed Program Review command.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramReviewDraftDto {
	pub review_id: EntityId,
	pub program_id: EntityId,
	pub work_item_id: EntityId,
	pub deterministic: ProgramEvidenceDraftDto,
	pub external: ProgramEvidenceDraftDto,
	pub classification: ProgramReviewClassification,
	pub rationale: WireText,
}

impl ProgramReviewDraftDto {
	pub(crate) fn validate(&self) -> Result<(), ProgramCycleContractError> {
		if self.deterministic.evidence_id == self.external.evidence_id {
			return Err(ProgramCycleContractError::InvalidIdentity);
		}
		for evidence in [&self.deterministic, &self.external] {
			if evidence.source.as_str().is_empty()
				|| evidence.summary.as_str().is_empty()
				|| evidence.source.as_str().chars().any(char::is_control)
				|| evidence.summary.as_str().chars().any(char::is_control)
			{
				return Err(ProgramCycleContractError::InvalidText);
			}
			if evidence.observed_at_micros <= 0 {
				return Err(ProgramCycleContractError::InvalidTime);
			}
		}
		if self.rationale.as_str().is_empty()
			|| self.rationale.as_str().chars().any(char::is_control)
		{
			return Err(ProgramCycleContractError::InvalidText);
		}
		Ok(())
	}
}

/// Bounded Program selector row.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramSummaryDto {
	pub program_id: EntityId,
	pub name: WireText,
	pub purpose: WireText,
	pub state: ProgramState,
	pub revision: EntityRevision,
	pub updated_at_micros: i64,
}

/// Typed semantic node in one authoritative causal projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgramNodeKind {
	Signal,
	Claim,
	Proposal,
	Objective,
	WorkItem,
	Run,
	Evidence,
	Review,
}

/// Small field retained by an inspector without creating arbitrary extension data.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramNodeFieldDto {
	pub label: WireText,
	pub value: WireText,
}

/// One authoritative semantic or runtime-lens node.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramNodeDto {
	pub id: EntityId,
	pub kind: ProgramNodeKind,
	pub title: WireText,
	pub summary: WireText,
	pub state: WireText,
	pub source: Option<WireText>,
	pub observed_at_micros: Option<i64>,
	pub conversation_id: Option<EntityId>,
	pub fields: Vec<ProgramNodeFieldDto>,
}

/// Closed first relation vocabulary used by the causal graph.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgramRelationKind {
	Continues,
	Observes,
	Supports,
	Justifies,
	Proposes,
	DecomposesTo,
	Executes,
	Produces,
	Validates,
}

/// One derived causal relation between stable accepted identities.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramEdgeDto {
	pub from: EntityId,
	pub to: EntityId,
	pub kind: ProgramRelationKind,
}

/// Complete bounded Program charter and synchronized causal projection.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramCycleDto {
	pub program: ProgramSummaryDto,
	pub non_goals: Vec<WireText>,
	pub review_policy: WireText,
	pub domain_pack: Option<DomainPackProjectionDto>,
	pub nodes: Vec<ProgramNodeDto>,
	pub edges: Vec<ProgramEdgeDto>,
}

impl ProgramCycleDto {
	pub fn new(
		program: ProgramSummaryDto,
		non_goals: Vec<WireText>,
		review_policy: WireText,
		nodes: Vec<ProgramNodeDto>,
		edges: Vec<ProgramEdgeDto>,
	) -> Result<Self, ProgramCycleContractError> {
		validate_list(&non_goals)?;
		if review_policy.as_str().is_empty()
			|| nodes.is_empty()
			|| nodes.len() > MAX_PROGRAM_NODES
			|| edges.len() > MAX_PROGRAM_EDGES
		{
			return Err(ProgramCycleContractError::InvalidProjection);
		}
		let node_ids = nodes.iter().map(|node| node.id.as_str()).collect::<HashSet<_>>();
		if node_ids.len() != nodes.len()
			|| nodes.iter().any(|node| {
				node.title.as_str().is_empty()
					|| node.summary.as_str().is_empty()
					|| node.state.as_str().is_empty()
					|| node.fields.len() > 8
			}) || edges.iter().any(|edge| {
			!node_ids.contains(edge.from.as_str()) && edge.from != program.program_id
				|| !node_ids.contains(edge.to.as_str()) && edge.to != program.program_id
		}) {
			return Err(ProgramCycleContractError::InvalidProjection);
		}
		Ok(Self { program, non_goals, review_policy, domain_pack: None, nodes, edges })
	}

	pub fn with_domain_pack(
		mut self,
		domain_pack: DomainPackProjectionDto,
	) -> Result<Self, ProgramCycleContractError> {
		let domain_pack = DomainPackProjectionDto::new(
			domain_pack.descriptor,
			domain_pack.entities,
			domain_pack.relations,
			&self.program.program_id,
		)
		.map_err(|_| ProgramCycleContractError::InvalidProjection)?;
		self.domain_pack = Some(domain_pack);
		Ok(self)
	}
}

/// Bounded Program selector outcome.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "outcome", content = "data", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProgramListResult {
	Available(Vec<ProgramSummaryDto>),
	Unavailable,
}

/// Exact Program causal readback outcome.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "outcome", content = "data", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProgramCycleResult {
	Available(Box<ProgramCycleDto>),
	NotFound,
	Unavailable,
}

fn validate_list(values: &[WireText]) -> Result<(), ProgramCycleContractError> {
	if values.is_empty()
		|| values.len() > MAX_PROGRAM_LIST_VALUES
		|| values
			.iter()
			.any(|value| value.as_str().is_empty() || value.as_str().chars().any(char::is_control))
		|| values.iter().map(WireText::as_str).collect::<HashSet<_>>().len() != values.len()
	{
		return Err(ProgramCycleContractError::InvalidCollection);
	}
	Ok(())
}

fn validate_joined_field(values: &[WireText]) -> Result<(), ProgramCycleContractError> {
	let separators = values.len().saturating_sub(1).saturating_mul(" · ".len());
	let bytes = values
		.iter()
		.try_fold(separators, |total, value| total.checked_add(value.as_str().len()))
		.ok_or(ProgramCycleContractError::InvalidCollection)?;
	if bytes > MAX_WIRE_TEXT_BYTES {
		return Err(ProgramCycleContractError::InvalidCollection);
	}
	Ok(())
}
