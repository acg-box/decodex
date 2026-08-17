//! Bounded semantic Program-cycle contracts for the Adaptive Factory Spine.

use std::collections::HashSet;

pub use decodex_core::{
	MAX_PROGRAM_PROJECTION_NODES as MAX_PROGRAM_NODES, ProgramReviewClassification, ProgramState,
};
use serde::{Deserialize, Serialize};

use crate::{
	QuickTaskWorkingDirectory,
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
	/// One or more stable identities are invalid or duplicated.
	InvalidIdentity,
	/// A bounded text field is empty or contains invalid data.
	InvalidText,
	/// A bounded collection is empty, oversized, or duplicated.
	InvalidCollection,
	/// A required observation time is not positive.
	InvalidTime,
	/// A causal projection violates its closed graph contract.
	InvalidProjection,
}

/// One complete pre-execution semantic chain.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramCycleDraftDto {
	/// Stable identity for the new Program.
	pub program_id: EntityId,
	/// Exact built-in Domain Pack selected for the Program.
	pub domain_pack_id: WireText,
	/// Stable identity for the first Signal.
	pub signal_id: EntityId,
	/// Stable identity for the first Claim.
	pub claim_id: EntityId,
	/// Stable identity for the first Proposal.
	pub proposal_id: EntityId,
	/// Stable identity for the first Objective.
	pub objective_id: EntityId,
	/// Stable identity for the first WorkItem.
	pub work_item_id: EntityId,
	/// User-visible Program name.
	pub name: WireText,
	/// Bounded Program purpose.
	pub purpose: WireText,
	/// Explicit Program non-goals.
	pub non_goals: Vec<WireText>,
	/// Review policy for Program cycles.
	pub review_policy: WireText,
	/// Source label for the first Signal.
	pub signal_source: WireText,
	/// Bounded summary of the first Signal.
	pub signal_summary: WireText,
	/// Observation time for the first Signal, in Unix microseconds.
	pub signal_observed_at_micros: i64,
	/// Statement asserted by the first Claim.
	pub claim_statement: WireText,
	/// Summary of the first Proposal.
	pub proposal_summary: WireText,
	/// Expected effect of the first Proposal.
	pub proposal_expected_effect: WireText,
	/// Risk declared for the first Proposal.
	pub proposal_risk: WireText,
	/// Evidence required by the first Proposal.
	pub proposal_evidence_need: WireText,
	/// Intended outcome of the first Objective.
	pub objective_outcome: WireText,
	/// Acceptance criteria for the first Objective.
	pub acceptance_criteria: Vec<WireText>,
	/// Validation criteria for the first Objective.
	pub validation_criteria: Vec<WireText>,
	/// User-visible title of the first WorkItem.
	pub work_item_title: WireText,
	/// Bounded execution instructions for the first WorkItem.
	pub work_item_instructions: WireText,
	/// Server-host working directory for the first WorkItem.
	pub working_directory: QuickTaskWorkingDirectory,
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
	/// Stable identity of the existing Program.
	pub program_id: EntityId,
	/// Exact terminal Review that authorizes this continuation.
	pub predecessor_review_id: EntityId,
	/// Stable identity for the next Signal.
	pub signal_id: EntityId,
	/// Stable identity for the next Claim.
	pub claim_id: EntityId,
	/// Stable identity for the next Proposal.
	pub proposal_id: EntityId,
	/// Stable identity for the next Objective.
	pub objective_id: EntityId,
	/// Stable identity for the next WorkItem.
	pub work_item_id: EntityId,
	/// Source label for the next Signal.
	pub signal_source: WireText,
	/// Bounded summary of the next Signal.
	pub signal_summary: WireText,
	/// Observation time for the next Signal, in Unix microseconds.
	pub signal_observed_at_micros: i64,
	/// Statement asserted by the next Claim.
	pub claim_statement: WireText,
	/// Summary of the next Proposal.
	pub proposal_summary: WireText,
	/// Expected effect of the next Proposal.
	pub proposal_expected_effect: WireText,
	/// Risk declared for the next Proposal.
	pub proposal_risk: WireText,
	/// Evidence required by the next Proposal.
	pub proposal_evidence_need: WireText,
	/// Intended outcome of the next Objective.
	pub objective_outcome: WireText,
	/// Acceptance criteria for the next Objective.
	pub acceptance_criteria: Vec<WireText>,
	/// Validation criteria for the next Objective.
	pub validation_criteria: Vec<WireText>,
	/// User-visible title of the next WorkItem.
	pub work_item_title: WireText,
	/// Bounded execution instructions for the next WorkItem.
	pub work_item_instructions: WireText,
	/// Server-host working directory for the next WorkItem.
	pub working_directory: QuickTaskWorkingDirectory,
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
	/// Stable identity for the proposed Evidence.
	pub evidence_id: EntityId,
	/// Bounded Evidence source label.
	pub source: WireText,
	/// Bounded Evidence summary.
	pub summary: WireText,
	/// Evidence observation time, in Unix microseconds.
	pub observed_at_micros: i64,
}

/// One terminal evidence-backed Program Review command.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramReviewDraftDto {
	/// Stable identity for the terminal Review.
	pub review_id: EntityId,
	/// Stable identity of the reviewed Program.
	pub program_id: EntityId,
	/// Stable identity of the reviewed WorkItem.
	pub work_item_id: EntityId,
	/// Required deterministic Evidence.
	pub deterministic: ProgramEvidenceDraftDto,
	/// Required external Evidence.
	pub external: ProgramEvidenceDraftDto,
	/// Closed terminal Review classification.
	pub classification: ProgramReviewClassification,
	/// Bounded rationale for the Review classification.
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
	/// Stable Program identity.
	pub program_id: EntityId,
	/// User-visible Program name.
	pub name: WireText,
	/// Bounded Program purpose.
	pub purpose: WireText,
	/// Current Program state.
	pub state: ProgramState,
	/// Current Program revision.
	pub revision: EntityRevision,
	/// Last update time, in Unix microseconds.
	pub updated_at_micros: i64,
}

/// Typed semantic node in one authoritative causal projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgramNodeKind {
	/// An observed Program input.
	Signal,
	/// A statement derived from a Signal.
	Claim,
	/// A proposed action and its expected effect.
	Proposal,
	/// An accepted outcome to pursue.
	Objective,
	/// One executable unit of Program work.
	WorkItem,
	/// Runtime execution bound to a WorkItem.
	Run,
	/// Deterministic or external validation material.
	Evidence,
	/// Terminal evaluation of one Program cycle.
	Review,
}

/// Small field retained by an inspector without creating arbitrary extension data.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramNodeFieldDto {
	/// User-visible field label.
	pub label: WireText,
	/// Bounded field value.
	pub value: WireText,
}

/// One authoritative semantic or runtime-lens node.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramNodeDto {
	/// Stable node identity.
	pub id: EntityId,
	/// Closed semantic node kind.
	pub kind: ProgramNodeKind,
	/// User-visible node title.
	pub title: WireText,
	/// Bounded node summary.
	pub summary: WireText,
	/// Current closed or domain-defined node state.
	pub state: WireText,
	/// Optional source label for the node fact.
	pub source: Option<WireText>,
	/// Optional observation time, in Unix microseconds.
	pub observed_at_micros: Option<i64>,
	/// Optional Conversation bound to this node.
	pub conversation_id: Option<EntityId>,
	/// Small inspector fields for this node.
	pub fields: Vec<ProgramNodeFieldDto>,
}

/// Closed first relation vocabulary used by the causal graph.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgramRelationKind {
	/// A Signal continues a terminal Review.
	Continues,
	/// A Claim observes a Signal.
	Observes,
	/// Evidence supports a Claim.
	Supports,
	/// A Claim justifies a Proposal.
	Justifies,
	/// A Proposal creates an Objective.
	Proposes,
	/// An Objective decomposes into a WorkItem.
	DecomposesTo,
	/// A WorkItem executes through a Run.
	Executes,
	/// A Run produces Evidence.
	Produces,
	/// Evidence validates a Review.
	Validates,
}

/// One derived causal relation between stable accepted identities.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramEdgeDto {
	/// Source Program or node identity.
	pub from: EntityId,
	/// Target Program or node identity.
	pub to: EntityId,
	/// Closed causal relation kind.
	pub kind: ProgramRelationKind,
}

/// Complete bounded Program charter and synchronized causal projection.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramCycleDto {
	/// Current Program summary.
	pub program: ProgramSummaryDto,
	/// Explicit Program non-goals.
	pub non_goals: Vec<WireText>,
	/// Review policy for Program cycles.
	pub review_policy: WireText,
	/// Optional derived projection from the bound Domain Pack.
	pub domain_pack: Option<DomainPackProjectionDto>,
	/// Bounded causal graph nodes.
	pub nodes: Vec<ProgramNodeDto>,
	/// Bounded causal graph edges.
	pub edges: Vec<ProgramEdgeDto>,
}

impl ProgramCycleDto {
	/// Validate and construct one causal Program projection.
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

	/// Validate and attach one derived Domain Pack projection.
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
	/// The bounded Program selector returned current rows.
	Available(Vec<ProgramSummaryDto>),
	/// Program selection authority is unavailable.
	Unavailable,
}

/// Exact Program causal readback outcome.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "outcome", content = "data", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProgramCycleResult {
	/// The exact Program and causal projection are available.
	Available(Box<ProgramCycleDto>),
	/// The exact Program does not exist.
	NotFound,
	/// Program read authority is unavailable.
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
