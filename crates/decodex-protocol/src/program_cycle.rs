//! Bounded read-only projections for retained Program history.

use std::collections::HashSet;

pub use decodex_core::{
	MAX_PROGRAM_PROJECTION_NODES as MAX_PROGRAM_NODES, ProgramReviewClassification, ProgramState,
};
use serde::{Deserialize, Serialize};

use crate::{
	domain_pack::DomainPackProjectionDto,
	wire::{EntityId, EntityRevision, WireText},
};

/// Maximum Programs returned by one local selector query.
pub const MAX_PROGRAM_LIST_ITEMS: usize = 64;
/// Maximum causal edges in one Program projection.
pub const MAX_PROGRAM_EDGES: usize = 256;
/// Maximum bounded list items in a retained Program projection.
pub const MAX_PROGRAM_LIST_VALUES: usize = 32;

/// Closed Program-cycle contract refusal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgramCycleContractError {
	/// A bounded collection violates its size or uniqueness contract.
	InvalidCollection,
	/// The projection violates its identity or relation contract.
	InvalidProjection,
}

/// Bounded Program selector row.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramSummaryDto {
	/// Identity of the retained Program.
	pub program_id: EntityId,
	/// Human-readable display name.
	pub name: WireText,
	/// Recorded purpose of the Program.
	pub purpose: WireText,
	/// Recorded state of this entity.
	pub state: ProgramState,
	/// Persisted revision of this record.
	pub revision: EntityRevision,
	/// Last update time in Unix microseconds.
	pub updated_at_micros: i64,
}

/// Typed semantic node in one authoritative causal projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgramNodeKind {
	/// A sourced observation.
	Signal,
	/// An interpretation of an observation.
	Claim,
	/// A recorded proposed course of action.
	Proposal,
	/// A finite desired outcome.
	Objective,
	/// A recorded unit of work.
	WorkItem,
	/// An execution bound to recorded work.
	Run,
	/// A recorded validation or external observation.
	Evidence,
	/// A historical assessment of evidence.
	Review,
}

/// Small field retained by an inspector without creating arbitrary extension data.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramNodeFieldDto {
	/// Human-readable field label.
	pub label: WireText,
	/// Bounded field value.
	pub value: WireText,
}

/// One authoritative semantic or runtime-lens node.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramNodeDto {
	/// Stable identity within this projection.
	pub id: EntityId,
	/// Closed category of this record.
	pub kind: ProgramNodeKind,
	/// Human-readable title.
	pub title: WireText,
	/// Bounded summary of the source record.
	pub summary: WireText,
	/// Recorded state of this entity.
	pub state: WireText,
	/// Optional source attribution.
	pub source: Option<WireText>,
	/// Optional observation time in Unix microseconds.
	pub observed_at_micros: Option<i64>,
	/// Optional bound Conversation identity.
	pub conversation_id: Option<EntityId>,
	/// Bounded inspectable fields.
	pub fields: Vec<ProgramNodeFieldDto>,
}

/// Closed first relation vocabulary used by the causal graph.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgramRelationKind {
	/// A later cycle follows a prior review.
	Continues,
	/// An observation concerns its target.
	Observes,
	/// Evidence supports its target.
	Supports,
	/// A source justifies its target.
	Justifies,
	/// A source proposes its target.
	Proposes,
	/// A source divides into its target.
	DecomposesTo,
	/// An execution performs its target work.
	Executes,
	/// A source produces its target.
	Produces,
	/// Evidence validates its target.
	Validates,
}

/// One derived causal relation between stable accepted identities.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramEdgeDto {
	/// Source entity identity.
	pub from: EntityId,
	/// Target entity identity.
	pub to: EntityId,
	/// Closed category of this record.
	pub kind: ProgramRelationKind,
}

/// Complete bounded Program charter and synchronized causal projection.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramCycleDto {
	/// Retained Program charter summary.
	pub program: ProgramSummaryDto,
	/// Recorded exclusions from the Program scope.
	pub non_goals: Vec<WireText>,
	/// Historical review requirements.
	pub review_policy: WireText,
	/// Optional historical Domain Pack projection.
	pub domain_pack: Option<DomainPackProjectionDto>,
	/// Retained semantic and execution nodes.
	pub nodes: Vec<ProgramNodeDto>,
	/// Causal relations between projected nodes.
	pub edges: Vec<ProgramEdgeDto>,
}

impl ProgramCycleDto {
	/// Construct and validate the historical projection.
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

	/// Attach a validated historical Pack projection to this Program.
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
	/// The requested historical data is available.
	Available(Vec<ProgramSummaryDto>),
	/// The requested data or capability is unavailable.
	Unavailable,
}

/// Exact Program causal readback outcome.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "outcome", content = "data", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProgramCycleResult {
	/// The requested historical data is available.
	Available(Box<ProgramCycleDto>),
	/// The requested historical identity does not exist.
	NotFound,
	/// The requested data or capability is unavailable.
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
