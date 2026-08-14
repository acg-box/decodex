//! Canonical structural WorkItem values and non-authoritative readiness assessment.

use std::{
	collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque},
	error::Error,
	fmt::{Display, Formatter},
};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

use crate::{AgentId, ObjectiveId, ObjectiveState, ProgramId, ProgramState, ProjectId};

macro_rules! stable_id {
	($name:ident, $error:ident, $label:literal) => {
		#[doc = concat!("Stable canonical ", $label, " identity.")]
		#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
		pub struct $name(String);
		impl $name {
			#[doc = concat!("Parse one canonical lowercase RFC 9562 UUID-v4 ", $label, " identity.")]
			pub fn new(value: impl Into<String>) -> Result<Self, WorkItemError> {
				let value = value.into();
				if !is_canonical_uuid_v4(&value) {
					return Err(WorkItemError::$error);
				}
				Ok(Self(value))
			}

			#[doc = concat!("Borrow the canonical ", $label, " identity.")]
			pub fn as_str(&self) -> &str {
				&self.0
			}
		}
		impl Display for $name {
			fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
				formatter.write_str(&self.0)
			}
		}
		impl Serialize for $name {
			fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
			where
				S: Serializer,
			{
				serializer.serialize_str(&self.0)
			}
		}
		impl<'de> Deserialize<'de> for $name {
			fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
			where
				D: Deserializer<'de>,
			{
				Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
			}
		}
	};
}

stable_id!(WorkItemId, InvalidWorkItemId, "WorkItem");

stable_id!(WorkItemCorrelationId, InvalidCorrelationId, "WorkItem correlation");

/// Maximum UTF-8 bytes in a WorkItem title.
pub const MAX_WORK_ITEM_TITLE_BYTES: usize = 256;
/// Maximum UTF-8 bytes in an ordinary WorkItem text value.
pub const MAX_WORK_ITEM_TEXT_BYTES: usize = 4_096;
/// Maximum acceptance or validation criteria on one WorkItem.
pub const MAX_WORK_ITEM_CRITERIA: usize = 32;
/// Maximum Objective references on one WorkItem.
pub const MAX_WORK_ITEM_OBJECTIVES: usize = 32;
/// Maximum nodes accepted by one closed graph validation.
pub const MAX_WORK_ITEM_GRAPH_NODES: usize = 4_096;
/// Maximum edges accepted by one closed graph validation.
pub const MAX_WORK_ITEM_GRAPH_EDGES: usize = 16_384;
/// Maximum related WorkItem edges or observations accepted by one readiness assessment.
pub const MAX_WORK_ITEM_READINESS_RELATIONS: usize = 256;
/// Combined maximum Program plus Objective observations accepted by one readiness assessment.
pub const MAX_WORK_ITEM_READINESS_CONTEXT: usize = 256;
/// Latest finite timestamp representable by durable-store and RFC 3339, in Unix microseconds.
pub const MAX_WORK_ITEM_TIMESTAMP_MICROSECONDS: i64 = 253_402_300_799_999_999;

/// Closed structural WorkItem refusal without caller-controlled text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkItemError {
	/// WorkItem identity was not canonical UUID-v4 text.
	InvalidWorkItemId,
	/// Mutation correlation identity was not canonical UUID-v4 text.
	InvalidCorrelationId,
	/// Bounded ordinary text was empty, oversized, or contained controls.
	InvalidText,
	/// A bounded collection was empty, oversized, or contained duplicates.
	InvalidCollection,
	/// Ordinary domain data contained concrete credential material.
	CredentialRejected,
	/// A persisted or incremented optimistic revision was invalid.
	InvalidRevision,
	/// Expected revision did not match the current WorkItem revision.
	RevisionConflict,
	/// A timestamp was outside the finite domain or chronology was invalid.
	InvalidChronology,
	/// A Program or Objective reference declared another Project.
	CrossProjectReference,
	/// Requested ordinary structural transition was not legal.
	InvalidLifecycle,
	/// A graph or readiness input exceeded its explicit cardinality bound.
	InputLimitExceeded,
	/// A dependency or blocker edge referred to itself.
	SelfEdge,
	/// A dependency or blocker edge was repeated.
	DuplicateEdge,
	/// A dependency or blocker edge crossed Projects.
	CrossProjectEdge,
	/// An edge endpoint was outside the supplied closed graph.
	UnknownNode,
	/// A closed graph repeated a WorkItem node identity.
	DuplicateNode,
	/// A closed graph was empty or did not prove exactly one Project.
	NonSingleProjectGraph,
	/// The dependency and blocker graph contained a cycle.
	DependencyCycle,
	/// A supplied readiness observation did not structurally bind its declared subject.
	InvalidObservation,
}
impl Error for WorkItemError {}

impl Display for WorkItemError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(match self {
			Self::InvalidWorkItemId => "invalid WorkItem identity",
			Self::InvalidCorrelationId => "invalid WorkItem correlation identity",
			Self::InvalidText => "invalid bounded WorkItem text",
			Self::InvalidCollection => "invalid bounded WorkItem collection",
			Self::CredentialRejected => "credential-bearing WorkItem data rejected",
			Self::InvalidRevision => "invalid WorkItem revision",
			Self::RevisionConflict => "WorkItem revision conflict",
			Self::InvalidChronology => "invalid WorkItem chronology",
			Self::CrossProjectReference => "cross-Project WorkItem reference rejected",
			Self::InvalidLifecycle => "invalid ordinary WorkItem lifecycle transition",
			Self::InputLimitExceeded => "WorkItem input cardinality limit exceeded",
			Self::SelfEdge => "self WorkItem edge rejected",
			Self::DuplicateEdge => "duplicate WorkItem edge rejected",
			Self::CrossProjectEdge => "cross-Project WorkItem edge rejected",
			Self::UnknownNode => "WorkItem edge endpoint is outside the closed graph",
			Self::DuplicateNode => "duplicate WorkItem graph node rejected",
			Self::NonSingleProjectGraph => "closed WorkItem graph does not prove one Project",
			Self::DependencyCycle => "WorkItem dependency cycle rejected",
			Self::InvalidObservation => "invalid structural WorkItem readiness observation",
		})
	}
}

/// Closed WorkItem priority ordered from highest to lowest urgency.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkItemPriority {
	/// Immediate attention is required.
	Urgent,
	/// High-priority planned work.
	High,
	/// Normal-priority planned work.
	Medium,
	/// Low-priority planned work.
	Low,
	/// Explicitly unprioritized intake.
	None,
}
impl WorkItemPriority {
	/// Canonical persistence spelling.
	pub const fn as_str(self) -> &'static str {
		match self {
			Self::Urgent => "urgent",
			Self::High => "high",
			Self::Medium => "medium",
			Self::Low => "low",
			Self::None => "none",
		}
	}
}

/// Complete WorkItem lifecycle vocabulary.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkItemState {
	/// Untriaged Lead intake.
	Inbox,
	/// Triaged work not yet recorded as ready by the authoritative owner.
	Planned,
	/// Authoritative storage recorded readiness.
	Ready,
	/// Managed execution is active.
	Running,
	/// Output is awaiting or undergoing review.
	Review,
	/// Progress is explicitly prevented.
	Blocked,
	/// Authoritative acceptance and validation recorded success.
	Done,
	/// Work ended without success.
	Canceled,
}
impl WorkItemState {
	/// Canonical persistence spelling.
	pub const fn as_str(self) -> &'static str {
		match self {
			Self::Inbox => "inbox",
			Self::Planned => "planned",
			Self::Ready => "ready",
			Self::Running => "running",
			Self::Review => "review",
			Self::Blocked => "blocked",
			Self::Done => "done",
			Self::Canceled => "canceled",
		}
	}

	/// Whether a non-authoritative ordinary structural transition is legal.
	///
	/// Entering `ready`, `running`, or `done` belongs to authoritative downstream owners.
	/// Blocked work must return through `planned`; it cannot resume into execution or review.
	pub const fn can_transition_to(self, next: Self) -> bool {
		matches!(
			(self, next),
			(Self::Inbox, Self::Planned | Self::Canceled)
				| (Self::Planned, Self::Inbox | Self::Blocked | Self::Canceled)
				| (Self::Ready, Self::Planned | Self::Blocked | Self::Canceled)
				| (Self::Running, Self::Review | Self::Blocked | Self::Canceled)
				| (Self::Review, Self::Blocked | Self::Canceled)
				| (Self::Blocked, Self::Planned | Self::Canceled)
		)
	}
}

/// Closed dependency relation kind.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkItemEdgeKind {
	/// The source requires the target to be done.
	DependsOn,
	/// The target prevents the source until the target is terminal.
	BlockedBy,
}

/// Deterministically ordered explanation that prevents a structurally clear assessment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReadinessReason {
	/// Only planned or blocked projections are meaningful readiness subjects.
	Lifecycle(WorkItemState),
	/// Referenced Program observation was absent.
	ProgramObservationMissing(ProgramId),
	/// Referenced Program observation was not active.
	ProgramInactive(ProgramId, ProgramState),
	/// Program observations were duplicate, cross-Project, or unrelated.
	InvalidProgramObservation(ProgramId),
	/// Referenced Objective observation was absent.
	ObjectiveObservationMissing(ObjectiveId),
	/// Referenced Objective observation was not active.
	ObjectiveInactive(ObjectiveId, ObjectiveState),
	/// Objective observations were duplicate, cross-Project, or unrelated.
	InvalidObjectiveObservation(ObjectiveId),
	/// An edge did not uniquely constrain the assessed WorkItem.
	InvalidEdge(WorkItemId),
	/// An edge had no matching related WorkItem observation.
	RelatedObservationMissing(WorkItemEdgeKind, WorkItemId),
	/// Related observations were duplicate or did not match an edge.
	InvalidRelatedObservation(WorkItemId),
	/// A supplied dependency state was not done.
	DependencyIncomplete(WorkItemId, WorkItemState),
	/// A supplied blocker state was not terminal.
	BlockerActive(WorkItemId, WorkItemState),
}

/// Database-compatible finite time represented as Unix epoch microseconds.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct WorkItemTimestamp(i64);
impl WorkItemTimestamp {
	/// Validate one finite non-negative timestamp.
	pub const fn from_unix_microseconds(value: i64) -> Result<Self, WorkItemError> {
		if value < 0 || value > MAX_WORK_ITEM_TIMESTAMP_MICROSECONDS {
			Err(WorkItemError::InvalidChronology)
		} else {
			Ok(Self(value))
		}
	}

	/// Read Unix epoch microseconds without rounding or truncation.
	pub const fn unix_microseconds(self) -> i64 {
		self.0
	}
}

/// Bounded provenance attached to one structural WorkItem revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkItemProvenance {
	actor_id: AgentId,
	correlation_id: WorkItemCorrelationId,
	summary: String,
}
impl WorkItemProvenance {
	/// Construct credential-negative provenance.
	///
	/// `actor_id` is a declared identity only; this value grants no Agent authority.
	pub fn new(
		actor_id: AgentId,
		correlation_id: WorkItemCorrelationId,
		summary: impl Into<String>,
	) -> Result<Self, WorkItemError> {
		let summary = summary.into();

		validate_text(&summary, MAX_WORK_ITEM_TEXT_BYTES)?;

		Ok(Self { actor_id, correlation_id, summary })
	}

	/// Declared actor identity for this provenance record.
	pub const fn actor_id(&self) -> &AgentId {
		&self.actor_id
	}

	/// Stable mutation correlation identity.
	pub const fn correlation_id(&self) -> &WorkItemCorrelationId {
		&self.correlation_id
	}

	/// Bounded inspectable summary.
	pub fn summary(&self) -> &str {
		&self.summary
	}
}

/// Optional explicitly Project-scoped Program reference.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkItemProgramRef {
	program_id: ProgramId,
	project_id: ProjectId,
}
impl WorkItemProgramRef {
	/// Construct one structural Program reference.
	pub const fn new(program_id: ProgramId, project_id: ProjectId) -> Self {
		Self { program_id, project_id }
	}

	/// Stable Program identity.
	pub const fn program_id(&self) -> &ProgramId {
		&self.program_id
	}

	/// Declared owning Project identity.
	pub const fn project_id(&self) -> &ProjectId {
		&self.project_id
	}
}

/// Normalized explicitly Project-scoped Objective reference.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct WorkItemObjectiveRef {
	objective_id: ObjectiveId,
	project_id: ProjectId,
}
impl WorkItemObjectiveRef {
	/// Construct one structural Objective reference.
	pub const fn new(objective_id: ObjectiveId, project_id: ProjectId) -> Self {
		Self { objective_id, project_id }
	}

	/// Stable Objective identity.
	pub const fn objective_id(&self) -> &ObjectiveId {
		&self.objective_id
	}

	/// Declared owning Project identity.
	pub const fn project_id(&self) -> &ProjectId {
		&self.project_id
	}
}

/// Canonical structural WorkItem aggregate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkItem {
	id: WorkItemId,
	project_id: ProjectId,
	declared_lead_id: AgentId,
	program: Option<WorkItemProgramRef>,
	objectives: Vec<WorkItemObjectiveRef>,
	title: String,
	description: String,
	priority: WorkItemPriority,
	acceptance_criteria: Vec<String>,
	validation_criteria: Vec<String>,
	state: WorkItemState,
	revision: u64,
	created_at: WorkItemTimestamp,
	updated_at: WorkItemTimestamp,
	provenance: WorkItemProvenance,
}
impl WorkItem {
	/// Create revision one of an inbox WorkItem using declared structural identities.
	#[allow(clippy::too_many_arguments)]
	pub fn new(
		id: WorkItemId,
		project_id: ProjectId,
		declared_lead_id: AgentId,
		program: Option<WorkItemProgramRef>,
		objectives: Vec<WorkItemObjectiveRef>,
		title: impl Into<String>,
		description: impl Into<String>,
		priority: WorkItemPriority,
		acceptance_criteria: Vec<String>,
		validation_criteria: Vec<String>,
		created_at: WorkItemTimestamp,
		provenance: WorkItemProvenance,
	) -> Result<Self, WorkItemError> {
		Self::from_stored(
			id,
			project_id,
			declared_lead_id,
			program,
			objectives,
			title.into(),
			description.into(),
			priority,
			acceptance_criteria,
			validation_criteria,
			WorkItemState::Inbox,
			1,
			created_at,
			created_at,
			provenance,
		)
	}

	/// Validate one deterministic persistence projection without granting mutation authority.
	#[allow(clippy::too_many_arguments)]
	pub fn from_stored(
		id: WorkItemId,
		project_id: ProjectId,
		declared_lead_id: AgentId,
		program: Option<WorkItemProgramRef>,
		mut objectives: Vec<WorkItemObjectiveRef>,
		title: String,
		description: String,
		priority: WorkItemPriority,
		acceptance_criteria: Vec<String>,
		validation_criteria: Vec<String>,
		state: WorkItemState,
		revision: u64,
		created_at: WorkItemTimestamp,
		updated_at: WorkItemTimestamp,
		provenance: WorkItemProvenance,
	) -> Result<Self, WorkItemError> {
		validate_text(&title, MAX_WORK_ITEM_TITLE_BYTES)?;
		validate_text(&description, MAX_WORK_ITEM_TEXT_BYTES)?;
		validate_criteria(&acceptance_criteria)?;
		validate_criteria(&validation_criteria)?;

		if revision == 0 {
			return Err(WorkItemError::InvalidRevision);
		}
		if created_at > updated_at {
			return Err(WorkItemError::InvalidChronology);
		}
		if program.as_ref().is_some_and(|value| value.project_id() != &project_id)
			|| objectives.iter().any(|value| value.project_id() != &project_id)
		{
			return Err(WorkItemError::CrossProjectReference);
		}
		if objectives.len() > MAX_WORK_ITEM_OBJECTIVES {
			return Err(WorkItemError::InvalidCollection);
		}

		objectives.sort();

		if objectives.windows(2).any(|pair| pair[0] == pair[1]) {
			return Err(WorkItemError::InvalidCollection);
		}

		Ok(Self {
			id,
			project_id,
			declared_lead_id,
			program,
			objectives,
			title,
			description,
			priority,
			acceptance_criteria,
			validation_criteria,
			state,
			revision,
			created_at,
			updated_at,
			provenance,
		})
	}

	/// Apply one ordinary structural transition with optimistic revision checking.
	///
	/// This method cannot enter `ready`, `running`, or `done` and consumes no readiness
	/// assessment. Authoritative downstream commands own those transitions.
	pub fn transition(
		&mut self,
		expected_revision: u64,
		state: WorkItemState,
		updated_at: WorkItemTimestamp,
		provenance: WorkItemProvenance,
	) -> Result<(), WorkItemError> {
		if expected_revision == 0 || expected_revision != self.revision {
			return Err(WorkItemError::RevisionConflict);
		}
		if updated_at < self.updated_at {
			return Err(WorkItemError::InvalidChronology);
		}
		if !self.state.can_transition_to(state) {
			return Err(WorkItemError::InvalidLifecycle);
		}

		self.revision = self.revision.checked_add(1).ok_or(WorkItemError::InvalidRevision)?;
		self.state = state;
		self.updated_at = updated_at;
		self.provenance = provenance;

		Ok(())
	}

	/// Stable WorkItem identity.
	pub const fn id(&self) -> &WorkItemId {
		&self.id
	}

	/// Declared owning Project identity.
	pub const fn project_id(&self) -> &ProjectId {
		&self.project_id
	}

	/// Declared owner Lead identity; storage must resolve its factual authority.
	pub const fn declared_lead_id(&self) -> &AgentId {
		&self.declared_lead_id
	}

	/// Optional same-Project Program reference.
	pub const fn program(&self) -> Option<&WorkItemProgramRef> {
		self.program.as_ref()
	}

	/// Sorted normalized same-Project Objective references.
	pub fn objectives(&self) -> &[WorkItemObjectiveRef] {
		&self.objectives
	}

	/// Bounded display title.
	pub fn title(&self) -> &str {
		&self.title
	}

	/// Bounded concrete execution request.
	pub fn description(&self) -> &str {
		&self.description
	}

	/// Closed priority.
	pub const fn priority(&self) -> WorkItemPriority {
		self.priority
	}

	/// Explicit bounded acceptance criteria.
	pub fn acceptance_criteria(&self) -> &[String] {
		&self.acceptance_criteria
	}

	/// Explicit bounded validation criteria, without validator authority.
	pub fn validation_criteria(&self) -> &[String] {
		&self.validation_criteria
	}

	/// Current lifecycle projection.
	pub const fn state(&self) -> WorkItemState {
		self.state
	}

	/// Positive optimistic revision.
	pub const fn revision(&self) -> u64 {
		self.revision
	}

	/// Finite creation timestamp.
	pub const fn created_at(&self) -> WorkItemTimestamp {
		self.created_at
	}

	/// Finite timestamp of the current revision.
	pub const fn updated_at(&self) -> WorkItemTimestamp {
		self.updated_at
	}

	/// Bounded provenance for the current revision.
	pub const fn provenance(&self) -> &WorkItemProvenance {
		&self.provenance
	}
}

/// One typed normalized structural dependency or blocker edge.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct WorkItemEdge {
	kind: WorkItemEdgeKind,
	project_id: ProjectId,
	work_item_id: WorkItemId,
	related_work_item_id: WorkItemId,
}
impl WorkItemEdge {
	/// Construct an edge while refusing self and declared cross-Project relationships.
	pub fn new(
		kind: WorkItemEdgeKind,
		work_item_id: WorkItemId,
		work_item_project_id: ProjectId,
		related_work_item_id: WorkItemId,
		related_project_id: ProjectId,
	) -> Result<Self, WorkItemError> {
		if work_item_id == related_work_item_id {
			return Err(WorkItemError::SelfEdge);
		}
		if work_item_project_id != related_project_id {
			return Err(WorkItemError::CrossProjectEdge);
		}

		Ok(Self { kind, project_id: work_item_project_id, work_item_id, related_work_item_id })
	}

	/// Closed edge kind.
	pub const fn kind(&self) -> WorkItemEdgeKind {
		self.kind
	}

	/// Declared shared Project identity.
	pub const fn project_id(&self) -> &ProjectId {
		&self.project_id
	}

	/// Source WorkItem identity.
	pub const fn work_item_id(&self) -> &WorkItemId {
		&self.work_item_id
	}

	/// Related dependency or blocker identity.
	pub const fn related_work_item_id(&self) -> &WorkItemId {
		&self.related_work_item_id
	}
}

/// One Project-scoped node in a closed graph observation.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct WorkItemNode {
	id: WorkItemId,
	project_id: ProjectId,
}
impl WorkItemNode {
	/// Construct one structural graph node.
	pub const fn new(id: WorkItemId, project_id: ProjectId) -> Self {
		Self { id, project_id }
	}

	/// Stable WorkItem identity.
	pub const fn id(&self) -> &WorkItemId {
		&self.id
	}

	/// Declared owning Project identity.
	pub const fn project_id(&self) -> &ProjectId {
		&self.project_id
	}
}

/// Supplied structural observation of one related WorkItem.
///
/// This projection carries no currentness or authorization claim.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct RelatedWorkItemObservation {
	edge: WorkItemEdge,
	revision: u64,
	updated_at: WorkItemTimestamp,
	state: WorkItemState,
}
impl RelatedWorkItemObservation {
	/// Bind an observation to the related endpoint declared by an edge.
	pub fn new(
		edge: WorkItemEdge,
		observed_work_item_id: WorkItemId,
		observed_project_id: ProjectId,
		revision: u64,
		updated_at: WorkItemTimestamp,
		state: WorkItemState,
	) -> Result<Self, WorkItemError> {
		if edge.related_work_item_id() != &observed_work_item_id
			|| edge.project_id() != &observed_project_id
		{
			return Err(WorkItemError::InvalidObservation);
		}
		if revision == 0 {
			return Err(WorkItemError::InvalidRevision);
		}

		Ok(Self { edge, revision, updated_at, state })
	}

	/// Edge whose related endpoint was observed.
	pub const fn edge(&self) -> &WorkItemEdge {
		&self.edge
	}

	/// Supplied positive revision, without a freshness claim.
	pub const fn revision(&self) -> u64 {
		self.revision
	}

	/// Supplied finite update timestamp, without a freshness claim.
	pub const fn updated_at(&self) -> WorkItemTimestamp {
		self.updated_at
	}

	/// Supplied lifecycle state.
	pub const fn state(&self) -> WorkItemState {
		self.state
	}
}

/// Supplied structural Program observation used only for readiness explanation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkItemProgramObservation {
	id: ProgramId,
	project_id: ProjectId,
	revision: u64,
	state: ProgramState,
}
impl WorkItemProgramObservation {
	/// Construct one positive-revision observation without a currentness claim.
	pub fn new(
		id: ProgramId,
		project_id: ProjectId,
		revision: u64,
		state: ProgramState,
	) -> Result<Self, WorkItemError> {
		if revision == 0 {
			return Err(WorkItemError::InvalidRevision);
		}

		Ok(Self { id, project_id, revision, state })
	}

	/// Observed Program identity.
	pub const fn id(&self) -> &ProgramId {
		&self.id
	}

	/// Declared owning Project identity.
	pub const fn project_id(&self) -> &ProjectId {
		&self.project_id
	}

	/// Supplied positive revision, without a freshness claim.
	pub const fn revision(&self) -> u64 {
		self.revision
	}

	/// Supplied lifecycle state.
	pub const fn state(&self) -> ProgramState {
		self.state
	}
}

/// Supplied structural Objective observation used only for readiness explanation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkItemObjectiveObservation {
	id: ObjectiveId,
	project_id: ProjectId,
	revision: u64,
	state: ObjectiveState,
}
impl WorkItemObjectiveObservation {
	/// Construct one positive-revision observation without a currentness claim.
	pub fn new(
		id: ObjectiveId,
		project_id: ProjectId,
		revision: u64,
		state: ObjectiveState,
	) -> Result<Self, WorkItemError> {
		if revision == 0 {
			return Err(WorkItemError::InvalidRevision);
		}

		Ok(Self { id, project_id, revision, state })
	}

	/// Observed Objective identity.
	pub const fn id(&self) -> &ObjectiveId {
		&self.id
	}

	/// Declared owning Project identity.
	pub const fn project_id(&self) -> &ProjectId {
		&self.project_id
	}

	/// Supplied positive revision, without a freshness claim.
	pub const fn revision(&self) -> u64 {
		self.revision
	}

	/// Supplied lifecycle state.
	pub const fn state(&self) -> ObjectiveState {
		self.state
	}
}

/// Bounded supplied observations for one non-authoritative readiness assessment.
pub struct ReadinessObservations<'a> {
	related_work_items: &'a [RelatedWorkItemObservation],
	programs: &'a [WorkItemProgramObservation],
	objectives: &'a [WorkItemObjectiveObservation],
}
impl<'a> ReadinessObservations<'a> {
	/// Assemble bounded observations without asserting that they are current or authoritative.
	pub fn new(
		related_work_items: &'a [RelatedWorkItemObservation],
		programs: &'a [WorkItemProgramObservation],
		objectives: &'a [WorkItemObjectiveObservation],
	) -> Result<Self, WorkItemError> {
		let context_count = programs
			.len()
			.checked_add(objectives.len())
			.ok_or(WorkItemError::InputLimitExceeded)?;

		if related_work_items.len() > MAX_WORK_ITEM_READINESS_RELATIONS
			|| context_count > MAX_WORK_ITEM_READINESS_CONTEXT
		{
			return Err(WorkItemError::InputLimitExceeded);
		}

		Ok(Self { related_work_items, programs, objectives })
	}
}

/// Immutable non-authoritative explanation over one supplied set of observations.
///
/// No WorkItem mutation consumes this value. An empty reason set is not a readiness permit and
/// does not establish that any observation is current.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReadinessAssessment {
	work_item_id: WorkItemId,
	work_item_revision: u64,
	reasons: Vec<ReadinessReason>,
}
impl ReadinessAssessment {
	/// Exact structural WorkItem identity assessed.
	pub const fn work_item_id(&self) -> &WorkItemId {
		&self.work_item_id
	}

	/// Exact structural WorkItem revision assessed.
	pub const fn work_item_revision(&self) -> u64 {
		self.work_item_revision
	}

	/// Deterministically ordered structural reasons.
	pub fn reasons(&self) -> &[ReadinessReason] {
		&self.reasons
	}
}

/// Validate a bounded closed graph with deterministic refusal precedence.
///
/// Hash-indexed node, duplicate, degree, and adjacency tables keep the traversal in expected
/// O(V+E) time. Input order fixes error precedence; hash iteration order is never observable.
pub fn validate_work_item_graph(
	nodes: &[WorkItemNode],
	edges: &[WorkItemEdge],
) -> Result<(), WorkItemError> {
	if nodes.len() > MAX_WORK_ITEM_GRAPH_NODES || edges.len() > MAX_WORK_ITEM_GRAPH_EDGES {
		return Err(WorkItemError::InputLimitExceeded);
	}

	let Some(project_id) = nodes.first().map(WorkItemNode::project_id) else {
		return Err(WorkItemError::NonSingleProjectGraph);
	};
	let mut node_index = HashMap::with_capacity(nodes.len());

	for (index, node) in nodes.iter().enumerate() {
		if node_index.insert(node.id(), index).is_some() {
			return Err(WorkItemError::DuplicateNode);
		}
		if node.project_id() != project_id {
			return Err(WorkItemError::NonSingleProjectGraph);
		}
	}

	let mut pairs = HashSet::with_capacity(edges.len());
	let mut incoming = vec![0_usize; nodes.len()];
	let mut outgoing = vec![Vec::new(); nodes.len()];

	for edge in edges {
		if !pairs.insert((edge.work_item_id(), edge.related_work_item_id())) {
			return Err(WorkItemError::DuplicateEdge);
		}

		let Some(&source) = node_index.get(edge.work_item_id()) else {
			return Err(WorkItemError::UnknownNode);
		};
		let Some(&target) = node_index.get(edge.related_work_item_id()) else {
			return Err(WorkItemError::UnknownNode);
		};

		if edge.project_id() != project_id {
			return Err(WorkItemError::CrossProjectEdge);
		}

		incoming[target] = incoming[target].checked_add(1).ok_or(WorkItemError::DependencyCycle)?;

		outgoing[source].push(target);
	}

	let mut queue = incoming
		.iter()
		.enumerate()
		.filter_map(|(index, count)| (*count == 0).then_some(index))
		.collect::<VecDeque<_>>();
	let mut visited = 0_usize;

	while let Some(source) = queue.pop_front() {
		visited += 1;

		for &target in &outgoing[source] {
			incoming[target] -= 1;

			if incoming[target] == 0 {
				queue.push_back(target);
			}
		}
	}

	if visited == nodes.len() { Ok(()) } else { Err(WorkItemError::DependencyCycle) }
}

/// Assess supplied structural readiness observations without mutation, reads, or authority claims.
pub fn assess_work_item_readiness(
	work_item: &WorkItem,
	edges: &[WorkItemEdge],
	observations: &ReadinessObservations<'_>,
) -> Result<ReadinessAssessment, WorkItemError> {
	if edges.len() > MAX_WORK_ITEM_READINESS_RELATIONS {
		return Err(WorkItemError::InputLimitExceeded);
	}

	let mut reasons = Vec::new();

	if !matches!(work_item.state(), WorkItemState::Planned | WorkItemState::Blocked) {
		reasons.push(ReadinessReason::Lifecycle(work_item.state()));
	}

	assess_programs(work_item, observations.programs, &mut reasons);
	assess_objectives(work_item, observations.objectives, &mut reasons);
	assess_related(work_item, edges, observations.related_work_items, &mut reasons);

	reasons.sort_by(|left, right| readiness_reason_key(left).cmp(&readiness_reason_key(right)));
	reasons.dedup();

	Ok(ReadinessAssessment {
		work_item_id: work_item.id().clone(),
		work_item_revision: work_item.revision(),
		reasons,
	})
}

fn assess_programs(
	work_item: &WorkItem,
	observations: &[WorkItemProgramObservation],
	reasons: &mut Vec<ReadinessReason>,
) {
	let referenced = work_item.program().map(WorkItemProgramRef::program_id);
	let mut index = BTreeMap::new();
	let mut invalid = BTreeSet::new();

	for observation in observations {
		if index.insert(observation.id(), observation).is_some()
			|| observation.project_id() != work_item.project_id()
			|| referenced != Some(observation.id())
		{
			invalid.insert(observation.id().clone());
		}
	}
	for id in &invalid {
		reasons.push(ReadinessReason::InvalidProgramObservation(id.clone()));
	}

	if let Some(id) = referenced {
		if invalid.contains(id) {
			return;
		}

		match index.get(id) {
			None => reasons.push(ReadinessReason::ProgramObservationMissing(id.clone())),
			Some(observation) if observation.state() == ProgramState::Active => {},
			Some(observation) =>
				reasons.push(ReadinessReason::ProgramInactive(id.clone(), observation.state())),
		}
	}
}

fn assess_objectives(
	work_item: &WorkItem,
	observations: &[WorkItemObjectiveObservation],
	reasons: &mut Vec<ReadinessReason>,
) {
	let mut index = BTreeMap::new();
	let mut invalid = BTreeSet::new();

	for observation in observations {
		let referenced = work_item
			.objectives()
			.binary_search_by(|value| value.objective_id().cmp(observation.id()))
			.is_ok();

		if index.insert(observation.id(), observation).is_some()
			|| observation.project_id() != work_item.project_id()
			|| !referenced
		{
			invalid.insert(observation.id().clone());
		}
	}
	for id in &invalid {
		reasons.push(ReadinessReason::InvalidObjectiveObservation(id.clone()));
	}
	for reference in work_item.objectives() {
		let id = reference.objective_id();

		if invalid.contains(id) {
			continue;
		}

		match index.get(id) {
			None => reasons.push(ReadinessReason::ObjectiveObservationMissing(id.clone())),
			Some(observation) if observation.state() == ObjectiveState::Active => {},
			Some(observation) =>
				reasons.push(ReadinessReason::ObjectiveInactive(id.clone(), observation.state())),
		}
	}
}

fn assess_related(
	work_item: &WorkItem,
	edges: &[WorkItemEdge],
	observations: &[RelatedWorkItemObservation],
	reasons: &mut Vec<ReadinessReason>,
) {
	let mut edge_index = BTreeMap::new();
	let mut invalid_edges = BTreeSet::new();

	for edge in edges {
		let id = edge.related_work_item_id();

		if edge_index.insert(id, edge).is_some()
			|| edge.work_item_id() != work_item.id()
			|| edge.project_id() != work_item.project_id()
		{
			invalid_edges.insert(id.clone());
		}
	}
	for id in &invalid_edges {
		reasons.push(ReadinessReason::InvalidEdge(id.clone()));
	}

	let mut observation_index = BTreeMap::new();
	let mut invalid_observations = BTreeSet::new();

	for observation in observations {
		let id = observation.edge().related_work_item_id();

		if observation_index.insert(observation.edge(), observation).is_some()
			|| edge_index.get(id).is_none_or(|edge| *edge != observation.edge())
		{
			invalid_observations.insert(id.clone());
		}
	}
	for id in &invalid_observations {
		reasons.push(ReadinessReason::InvalidRelatedObservation(id.clone()));
	}
	for (id, edge) in edge_index {
		if invalid_edges.contains(id) || invalid_observations.contains(id) {
			continue;
		}

		match observation_index.get(edge) {
			None =>
				reasons.push(ReadinessReason::RelatedObservationMissing(edge.kind(), id.clone())),
			Some(observation) => match (edge.kind(), observation.state()) {
				(WorkItemEdgeKind::DependsOn, WorkItemState::Done)
				| (WorkItemEdgeKind::BlockedBy, WorkItemState::Done | WorkItemState::Canceled) => {},
				(WorkItemEdgeKind::DependsOn, state) =>
					reasons.push(ReadinessReason::DependencyIncomplete(id.clone(), state)),
				(WorkItemEdgeKind::BlockedBy, state) =>
					reasons.push(ReadinessReason::BlockerActive(id.clone(), state)),
			},
		}
	}
}

fn readiness_reason_key(reason: &ReadinessReason) -> (u8, &str, u8) {
	match reason {
		ReadinessReason::Lifecycle(state) => (0, "", state_rank(*state)),
		ReadinessReason::ProgramObservationMissing(id) => (1, id.as_str(), 0),
		ReadinessReason::ProgramInactive(id, state) => (2, id.as_str(), program_state_rank(*state)),
		ReadinessReason::InvalidProgramObservation(id) => (3, id.as_str(), 0),
		ReadinessReason::ObjectiveObservationMissing(id) => (4, id.as_str(), 0),
		ReadinessReason::ObjectiveInactive(id, state) =>
			(5, id.as_str(), objective_state_rank(*state)),
		ReadinessReason::InvalidObjectiveObservation(id) => (6, id.as_str(), 0),
		ReadinessReason::InvalidEdge(id) => (7, id.as_str(), 0),
		ReadinessReason::RelatedObservationMissing(kind, id) =>
			(8, id.as_str(), edge_kind_rank(*kind)),
		ReadinessReason::InvalidRelatedObservation(id) => (9, id.as_str(), 0),
		ReadinessReason::DependencyIncomplete(id, state) => (10, id.as_str(), state_rank(*state)),
		ReadinessReason::BlockerActive(id, state) => (11, id.as_str(), state_rank(*state)),
	}
}

const fn edge_kind_rank(kind: WorkItemEdgeKind) -> u8 {
	match kind {
		WorkItemEdgeKind::DependsOn => 0,
		WorkItemEdgeKind::BlockedBy => 1,
	}
}

const fn state_rank(state: WorkItemState) -> u8 {
	match state {
		WorkItemState::Inbox => 0,
		WorkItemState::Planned => 1,
		WorkItemState::Ready => 2,
		WorkItemState::Running => 3,
		WorkItemState::Review => 4,
		WorkItemState::Blocked => 5,
		WorkItemState::Done => 6,
		WorkItemState::Canceled => 7,
	}
}

const fn program_state_rank(state: ProgramState) -> u8 {
	match state {
		ProgramState::Active => 0,
		ProgramState::NeedsAttention => 1,
		ProgramState::Blocked => 2,
		ProgramState::Paused => 3,
		ProgramState::Retired => 4,
	}
}

const fn objective_state_rank(state: ObjectiveState) -> u8 {
	match state {
		ObjectiveState::Proposed => 0,
		ObjectiveState::Active => 1,
		ObjectiveState::Blocked => 2,
		ObjectiveState::Achieved => 3,
		ObjectiveState::Abandoned => 4,
	}
}

fn validate_criteria(values: &[String]) -> Result<(), WorkItemError> {
	if values.is_empty() || values.len() > MAX_WORK_ITEM_CRITERIA {
		return Err(WorkItemError::InvalidCollection);
	}

	let mut unique = BTreeSet::new();

	for value in values {
		validate_text(value, MAX_WORK_ITEM_TEXT_BYTES)?;

		if !unique.insert(value) {
			return Err(WorkItemError::InvalidCollection);
		}
	}

	Ok(())
}

fn validate_text(value: &str, maximum: usize) -> Result<(), WorkItemError> {
	if value.is_empty() || value.len() > maximum || value.chars().any(char::is_control) {
		return Err(WorkItemError::InvalidText);
	}
	if crate::contains_credential_material(value) {
		return Err(WorkItemError::CredentialRejected);
	}

	Ok(())
}

fn is_canonical_uuid_v4(value: &str) -> bool {
	let bytes = value.as_bytes();

	bytes.len() == 36
		&& bytes[8] == b'-'
		&& bytes[13] == b'-'
		&& bytes[18] == b'-'
		&& bytes[23] == b'-'
		&& bytes[14] == b'4'
		&& matches!(bytes[19], b'8' | b'9' | b'a' | b'b')
		&& bytes.iter().enumerate().all(|(index, byte)| {
			matches!(index, 8 | 13 | 18 | 23)
				|| byte.is_ascii_digit()
				|| matches!(byte, b'a'..=b'f')
		})
}

#[cfg(test)]
mod tests {
	use crate::{
		MAX_WORK_ITEM_TIMESTAMP_MICROSECONDS, ReadinessReason, WorkItemCorrelationId,
		WorkItemError, WorkItemId,
		work_item::{
			self, AgentId, MAX_WORK_ITEM_CRITERIA, MAX_WORK_ITEM_GRAPH_EDGES,
			MAX_WORK_ITEM_GRAPH_NODES, MAX_WORK_ITEM_OBJECTIVES, MAX_WORK_ITEM_READINESS_RELATIONS,
			MAX_WORK_ITEM_TEXT_BYTES, MAX_WORK_ITEM_TITLE_BYTES, ObjectiveId, ObjectiveState,
			ProgramId, ProgramState, ProjectId, ReadinessObservations, RelatedWorkItemObservation,
			WorkItem, WorkItemEdge, WorkItemEdgeKind, WorkItemNode, WorkItemObjectiveObservation,
			WorkItemObjectiveRef, WorkItemPriority, WorkItemProgramObservation, WorkItemProgramRef,
			WorkItemProvenance, WorkItemState, WorkItemTimestamp,
		},
	};

	fn uuid(prefix: u8, value: usize) -> String {
		format!("{prefix:02x}000000-0000-4000-8000-{value:012x}")
	}

	fn work_item_id(value: usize) -> WorkItemId {
		WorkItemId::new(uuid(0x10, value)).unwrap()
	}

	fn project_id(value: usize) -> ProjectId {
		ProjectId::new(uuid(0x20, value)).unwrap()
	}

	fn program_id(value: usize) -> ProgramId {
		ProgramId::new(uuid(0x30, value)).unwrap()
	}

	fn objective_id(value: usize) -> ObjectiveId {
		ObjectiveId::new(uuid(0x40, value)).unwrap()
	}

	fn agent_id(value: usize) -> AgentId {
		AgentId::new(uuid(0x50, value)).unwrap()
	}

	fn timestamp(value: i64) -> WorkItemTimestamp {
		WorkItemTimestamp::from_unix_microseconds(value).unwrap()
	}

	fn provenance(value: usize) -> WorkItemProvenance {
		WorkItemProvenance::new(
			agent_id(value),
			WorkItemCorrelationId::new(uuid(0x60, value)).unwrap(),
			format!("structural update {value}"),
		)
		.unwrap()
	}

	fn item_with(
		value: usize,
		state: WorkItemState,
		program: Option<WorkItemProgramRef>,
		objectives: Vec<WorkItemObjectiveRef>,
	) -> WorkItem {
		WorkItem::from_stored(
			work_item_id(value),
			project_id(1),
			agent_id(1),
			program,
			objectives,
			format!("Work item {value}"),
			"bounded implementation request".into(),
			WorkItemPriority::Medium,
			vec!["accepted result".into()],
			vec!["focused tests pass".into()],
			state,
			1,
			timestamp(1),
			timestamp(1),
			provenance(value),
		)
		.unwrap()
	}

	fn edge(kind: WorkItemEdgeKind, source: usize, target: usize) -> WorkItemEdge {
		WorkItemEdge::new(
			kind,
			work_item_id(source),
			project_id(1),
			work_item_id(target),
			project_id(1),
		)
		.unwrap()
	}

	fn related(edge: WorkItemEdge, state: WorkItemState) -> RelatedWorkItemObservation {
		RelatedWorkItemObservation::new(
			edge.clone(),
			edge.related_work_item_id().clone(),
			edge.project_id().clone(),
			1,
			timestamp(1),
			state,
		)
		.unwrap()
	}

	#[test]
	fn canonical_ids_and_finite_timestamps_are_exact() {
		assert!(WorkItemId::new(uuid(0x10, 1)).is_ok());
		assert_eq!(
			WorkItemId::new("10000000-0000-5000-8000-000000000001"),
			Err(WorkItemError::InvalidWorkItemId)
		);
		assert_eq!(
			WorkItemId::new("10000000-0000-4000-7000-000000000001"),
			Err(WorkItemError::InvalidWorkItemId)
		);
		assert_eq!(
			WorkItemCorrelationId::new("UPPERCASE"),
			Err(WorkItemError::InvalidCorrelationId)
		);
		assert_eq!(
			WorkItemTimestamp::from_unix_microseconds(-1),
			Err(WorkItemError::InvalidChronology)
		);
		assert!(
			WorkItemTimestamp::from_unix_microseconds(MAX_WORK_ITEM_TIMESTAMP_MICROSECONDS).is_ok()
		);
		assert_eq!(
			WorkItemTimestamp::from_unix_microseconds(MAX_WORK_ITEM_TIMESTAMP_MICROSECONDS + 1),
			Err(WorkItemError::InvalidChronology)
		);
	}

	#[test]
	fn provenance_rejects_empty_oversized_control_and_credential_text() {
		let correlation = || WorkItemCorrelationId::new(uuid(0x60, 1)).unwrap();

		assert!(
			WorkItemProvenance::new(
				agent_id(1),
				correlation(),
				"p".repeat(MAX_WORK_ITEM_TEXT_BYTES),
			)
			.is_ok()
		);

		for summary in
			[String::new(), "p".repeat(MAX_WORK_ITEM_TEXT_BYTES + 1), "line\nbreak".into()]
		{
			assert_eq!(
				WorkItemProvenance::new(agent_id(1), correlation(), summary),
				Err(WorkItemError::InvalidText)
			);
		}

		assert_eq!(
			WorkItemProvenance::new(agent_id(1), correlation(), "secret=abcd"),
			Err(WorkItemError::CredentialRejected)
		);
	}

	#[test]
	fn work_item_fields_enforce_exact_boundaries_and_credentials() {
		let exact_title = "t".repeat(MAX_WORK_ITEM_TITLE_BYTES);
		let exact_text = "d".repeat(MAX_WORK_ITEM_TEXT_BYTES);
		let exact_criteria = (0..MAX_WORK_ITEM_CRITERIA)
			.map(|value| format!("criterion {value}"))
			.collect::<Vec<_>>();

		assert!(
			WorkItem::new(
				work_item_id(1),
				project_id(1),
				agent_id(1),
				None,
				vec![],
				exact_title,
				exact_text,
				WorkItemPriority::Urgent,
				exact_criteria.clone(),
				exact_criteria,
				timestamp(0),
				provenance(1),
			)
			.is_ok()
		);

		let oversized_title = "t".repeat(MAX_WORK_ITEM_TITLE_BYTES + 1);

		assert_eq!(
			WorkItem::new(
				work_item_id(1),
				project_id(1),
				agent_id(1),
				None,
				vec![],
				oversized_title,
				"description",
				WorkItemPriority::Low,
				vec!["accept".into()],
				vec!["validate".into()],
				timestamp(0),
				provenance(1),
			),
			Err(WorkItemError::InvalidText)
		);
		assert_eq!(
			WorkItem::new(
				work_item_id(1),
				project_id(1),
				agent_id(1),
				None,
				vec![],
				"title",
				"secret=abcd",
				WorkItemPriority::Low,
				vec!["accept".into()],
				vec!["validate".into()],
				timestamp(0),
				provenance(1),
			),
			Err(WorkItemError::CredentialRejected)
		);

		let too_many =
			(0..=MAX_WORK_ITEM_CRITERIA).map(|value| format!("criterion {value}")).collect();

		assert_eq!(
			WorkItem::new(
				work_item_id(1),
				project_id(1),
				agent_id(1),
				None,
				vec![],
				"title",
				"description",
				WorkItemPriority::None,
				too_many,
				vec!["validate".into()],
				timestamp(0),
				provenance(1),
			),
			Err(WorkItemError::InvalidCollection)
		);
	}

	#[test]
	fn acceptance_criteria_reject_empty_and_duplicate_collections() {
		let build = |acceptance_criteria| {
			WorkItem::new(
				work_item_id(1),
				project_id(1),
				agent_id(1),
				None,
				vec![],
				"title",
				"description",
				WorkItemPriority::Medium,
				acceptance_criteria,
				vec!["validate".into()],
				timestamp(1),
				provenance(1),
			)
		};

		assert_eq!(build(vec![]), Err(WorkItemError::InvalidCollection));
		assert_eq!(
			build(vec!["same criterion".into(), "same criterion".into()]),
			Err(WorkItemError::InvalidCollection)
		);
	}

	#[test]
	fn references_are_same_project_sorted_unique_and_bounded() {
		let project = project_id(1);
		let mut references = (0..MAX_WORK_ITEM_OBJECTIVES)
			.rev()
			.map(|value| WorkItemObjectiveRef::new(objective_id(value), project.clone()))
			.collect::<Vec<_>>();
		let item = item_with(1, WorkItemState::Inbox, None, references.clone());

		assert!(item.objectives().windows(2).all(|pair| pair[0] < pair[1]));

		references.push(WorkItemObjectiveRef::new(
			objective_id(MAX_WORK_ITEM_OBJECTIVES),
			project.clone(),
		));

		assert_eq!(
			WorkItem::new(
				work_item_id(1),
				project.clone(),
				agent_id(1),
				None,
				references,
				"title",
				"description",
				WorkItemPriority::Medium,
				vec!["accept".into()],
				vec!["validate".into()],
				timestamp(1),
				provenance(1),
			),
			Err(WorkItemError::InvalidCollection)
		);
		assert_eq!(
			WorkItem::new(
				work_item_id(1),
				project.clone(),
				agent_id(1),
				Some(WorkItemProgramRef::new(program_id(1), project_id(2))),
				vec![],
				"title",
				"description",
				WorkItemPriority::Medium,
				vec!["accept".into()],
				vec!["validate".into()],
				timestamp(1),
				provenance(1),
			),
			Err(WorkItemError::CrossProjectReference)
		);

		let duplicate = WorkItemObjectiveRef::new(objective_id(1), project);

		assert_eq!(
			WorkItem::new(
				work_item_id(1),
				project_id(1),
				agent_id(1),
				None,
				vec![duplicate.clone(), duplicate],
				"title",
				"description",
				WorkItemPriority::Medium,
				vec!["accept".into()],
				vec!["validate".into()],
				timestamp(1),
				provenance(1),
			),
			Err(WorkItemError::InvalidCollection)
		);
	}

	#[test]
	fn every_ordinary_lifecycle_pair_matches_the_closed_matrix() {
		let states = [
			WorkItemState::Inbox,
			WorkItemState::Planned,
			WorkItemState::Ready,
			WorkItemState::Running,
			WorkItemState::Review,
			WorkItemState::Blocked,
			WorkItemState::Done,
			WorkItemState::Canceled,
		];

		for source in states {
			for target in states {
				let mut item = item_with(1, source, None, vec![]);
				let result = item.transition(1, target, timestamp(2), provenance(2));

				assert_eq!(
					result.is_ok(),
					source.can_transition_to(target),
					"{source:?} -> {target:?}"
				);
			}
		}
		for source in states {
			assert!(!source.can_transition_to(WorkItemState::Ready));
			assert!(!source.can_transition_to(WorkItemState::Running));
			assert!(!source.can_transition_to(WorkItemState::Done));
		}

		assert!(!WorkItemState::Blocked.can_transition_to(WorkItemState::Review));
	}

	#[test]
	fn lifecycle_and_priority_spellings_are_exhaustive() {
		assert_eq!(
			[
				WorkItemState::Inbox,
				WorkItemState::Planned,
				WorkItemState::Ready,
				WorkItemState::Running,
				WorkItemState::Review,
				WorkItemState::Blocked,
				WorkItemState::Done,
				WorkItemState::Canceled,
			]
			.map(WorkItemState::as_str),
			["inbox", "planned", "ready", "running", "review", "blocked", "done", "canceled"],
		);
		assert_eq!(
			[
				WorkItemPriority::Urgent,
				WorkItemPriority::High,
				WorkItemPriority::Medium,
				WorkItemPriority::Low,
				WorkItemPriority::None,
			]
			.map(WorkItemPriority::as_str),
			["urgent", "high", "medium", "low", "none"],
		);
	}

	#[test]
	fn ordinary_transitions_check_revision_chronology_and_overflow() {
		let mut item = item_with(1, WorkItemState::Inbox, None, vec![]);

		assert_eq!(
			item.transition(2, WorkItemState::Planned, timestamp(2), provenance(2)),
			Err(WorkItemError::RevisionConflict)
		);
		assert_eq!(
			item.transition(1, WorkItemState::Planned, timestamp(0), provenance(2)),
			Err(WorkItemError::InvalidChronology)
		);

		item.transition(1, WorkItemState::Planned, timestamp(2), provenance(2)).unwrap();

		assert_eq!(item.revision(), 2);
		assert_eq!(item.state(), WorkItemState::Planned);

		let mut maximum = WorkItem::from_stored(
			work_item_id(2),
			project_id(1),
			agent_id(1),
			None,
			vec![],
			"title".into(),
			"description".into(),
			WorkItemPriority::Medium,
			vec!["accept".into()],
			vec!["validate".into()],
			WorkItemState::Inbox,
			u64::MAX,
			timestamp(1),
			timestamp(1),
			provenance(1),
		)
		.unwrap();

		assert_eq!(
			maximum.transition(u64::MAX, WorkItemState::Planned, timestamp(2), provenance(2)),
			Err(WorkItemError::InvalidRevision)
		);
	}

	#[test]
	fn stored_work_item_rejects_zero_revision_and_reversed_chronology() {
		let build = |revision, created_at, updated_at| {
			WorkItem::from_stored(
				work_item_id(1),
				project_id(1),
				agent_id(1),
				None,
				vec![],
				"title".into(),
				"description".into(),
				WorkItemPriority::Medium,
				vec!["accept".into()],
				vec!["validate".into()],
				WorkItemState::Inbox,
				revision,
				created_at,
				updated_at,
				provenance(1),
			)
		};

		assert_eq!(build(0, timestamp(1), timestamp(1)), Err(WorkItemError::InvalidRevision));
		assert_eq!(build(1, timestamp(2), timestamp(1)), Err(WorkItemError::InvalidChronology));
	}

	#[test]
	fn edges_refuse_self_and_declared_cross_project() {
		assert_eq!(
			WorkItemEdge::new(
				WorkItemEdgeKind::DependsOn,
				work_item_id(1),
				project_id(1),
				work_item_id(1),
				project_id(1),
			),
			Err(WorkItemError::SelfEdge)
		);
		assert_eq!(
			WorkItemEdge::new(
				WorkItemEdgeKind::BlockedBy,
				work_item_id(1),
				project_id(1),
				work_item_id(2),
				project_id(2),
			),
			Err(WorkItemError::CrossProjectEdge)
		);
	}

	#[test]
	fn supplied_observations_require_exact_bindings_and_positive_revisions() {
		let dependency = edge(WorkItemEdgeKind::DependsOn, 1, 2);

		assert_eq!(
			RelatedWorkItemObservation::new(
				dependency.clone(),
				work_item_id(3),
				project_id(1),
				1,
				timestamp(1),
				WorkItemState::Done,
			),
			Err(WorkItemError::InvalidObservation)
		);
		assert_eq!(
			RelatedWorkItemObservation::new(
				dependency.clone(),
				work_item_id(2),
				project_id(2),
				1,
				timestamp(1),
				WorkItemState::Done,
			),
			Err(WorkItemError::InvalidObservation)
		);
		assert_eq!(
			RelatedWorkItemObservation::new(
				dependency,
				work_item_id(2),
				project_id(1),
				0,
				timestamp(1),
				WorkItemState::Done,
			),
			Err(WorkItemError::InvalidRevision)
		);
		assert_eq!(
			WorkItemProgramObservation::new(program_id(1), project_id(1), 0, ProgramState::Active,),
			Err(WorkItemError::InvalidRevision)
		);
		assert_eq!(
			WorkItemObjectiveObservation::new(
				objective_id(1),
				project_id(1),
				0,
				ObjectiveState::Active,
			),
			Err(WorkItemError::InvalidRevision)
		);
	}

	#[test]
	fn closed_graph_requires_one_project_even_without_edges() {
		assert_eq!(
			crate::validate_work_item_graph(&[], &[]),
			Err(WorkItemError::NonSingleProjectGraph)
		);
		assert!(
			crate::validate_work_item_graph(
				&[WorkItemNode::new(work_item_id(1), project_id(1))],
				&[]
			)
			.is_ok()
		);
		assert_eq!(
			crate::validate_work_item_graph(
				&[
					WorkItemNode::new(work_item_id(1), project_id(1)),
					WorkItemNode::new(work_item_id(2), project_id(2)),
				],
				&[]
			),
			Err(WorkItemError::NonSingleProjectGraph)
		);
	}

	#[test]
	fn closed_graph_refuses_duplicate_unknown_and_cycles() {
		let nodes = [
			WorkItemNode::new(work_item_id(1), project_id(1)),
			WorkItemNode::new(work_item_id(2), project_id(1)),
			WorkItemNode::new(work_item_id(3), project_id(1)),
		];
		let first = edge(WorkItemEdgeKind::DependsOn, 1, 2);

		assert_eq!(
			crate::validate_work_item_graph(&nodes, &[first.clone(), first.clone()]),
			Err(WorkItemError::DuplicateEdge)
		);
		assert_eq!(
			crate::validate_work_item_graph(
				&nodes[..2],
				&[edge(WorkItemEdgeKind::DependsOn, 1, 3)]
			),
			Err(WorkItemError::UnknownNode)
		);

		let other_project_edge = WorkItemEdge::new(
			WorkItemEdgeKind::DependsOn,
			work_item_id(1),
			project_id(2),
			work_item_id(2),
			project_id(2),
		)
		.unwrap();

		assert_eq!(
			crate::validate_work_item_graph(&nodes, &[other_project_edge]),
			Err(WorkItemError::CrossProjectEdge)
		);
		assert_eq!(
			crate::validate_work_item_graph(
				&nodes,
				&[
					edge(WorkItemEdgeKind::DependsOn, 1, 2),
					edge(WorkItemEdgeKind::BlockedBy, 2, 3),
					edge(WorkItemEdgeKind::DependsOn, 3, 1),
				]
			),
			Err(WorkItemError::DependencyCycle)
		);
		assert_eq!(
			crate::validate_work_item_graph(
				&[
					WorkItemNode::new(work_item_id(1), project_id(1)),
					WorkItemNode::new(work_item_id(1), project_id(1)),
				],
				&[]
			),
			Err(WorkItemError::DuplicateNode)
		);
	}

	#[test]
	fn graph_cardinality_accepts_exact_limits_and_refuses_over_limit() {
		let nodes = (0..MAX_WORK_ITEM_GRAPH_NODES)
			.map(|value| WorkItemNode::new(work_item_id(value), project_id(1)))
			.collect::<Vec<_>>();

		assert!(crate::validate_work_item_graph(&nodes, &[]).is_ok());

		let mut over = nodes.clone();

		over.push(WorkItemNode::new(work_item_id(MAX_WORK_ITEM_GRAPH_NODES), project_id(1)));

		assert_eq!(
			crate::validate_work_item_graph(&over, &[]),
			Err(WorkItemError::InputLimitExceeded)
		);

		let two_nodes = [
			WorkItemNode::new(work_item_id(1), project_id(1)),
			WorkItemNode::new(work_item_id(2), project_id(1)),
		];
		let repeated = edge(WorkItemEdgeKind::DependsOn, 1, 2);
		let too_many = vec![repeated; MAX_WORK_ITEM_GRAPH_EDGES + 1];

		assert_eq!(
			crate::validate_work_item_graph(&two_nodes, &too_many),
			Err(WorkItemError::InputLimitExceeded)
		);
	}

	#[test]
	fn graph_accepts_exact_edge_limit_with_an_acyclic_closed_set() {
		let nodes = (0..MAX_WORK_ITEM_GRAPH_NODES)
			.map(|value| WorkItemNode::new(work_item_id(value), project_id(1)))
			.collect::<Vec<_>>();
		let mut edges = Vec::with_capacity(MAX_WORK_ITEM_GRAPH_EDGES);

		'source: for source in 0..MAX_WORK_ITEM_GRAPH_NODES {
			for target in source + 1..MAX_WORK_ITEM_GRAPH_NODES {
				edges.push(edge(WorkItemEdgeKind::DependsOn, source, target));

				if edges.len() == MAX_WORK_ITEM_GRAPH_EDGES {
					break 'source;
				}
			}
		}

		assert_eq!(edges.len(), MAX_WORK_ITEM_GRAPH_EDGES);
		assert!(crate::validate_work_item_graph(&nodes, &edges).is_ok());
	}

	#[test]
	fn readiness_context_uses_one_combined_256_observation_limit() {
		let programs = (0..128)
			.map(|value| {
				WorkItemProgramObservation::new(
					program_id(value),
					project_id(1),
					1,
					ProgramState::Active,
				)
				.unwrap()
			})
			.collect::<Vec<_>>();
		let objectives = (0..128)
			.map(|value| {
				WorkItemObjectiveObservation::new(
					objective_id(value),
					project_id(1),
					1,
					ObjectiveState::Active,
				)
				.unwrap()
			})
			.collect::<Vec<_>>();

		assert!(ReadinessObservations::new(&[], &programs, &objectives).is_ok());

		let mut over = objectives;

		over.push(
			WorkItemObjectiveObservation::new(
				objective_id(128),
				project_id(1),
				1,
				ObjectiveState::Active,
			)
			.unwrap(),
		);

		assert!(matches!(
			ReadinessObservations::new(&[], &programs, &over),
			Err(WorkItemError::InputLimitExceeded)
		));
	}

	#[test]
	fn readiness_relation_limit_has_exact_boundary() {
		let edges = (1..=MAX_WORK_ITEM_READINESS_RELATIONS)
			.map(|target| edge(WorkItemEdgeKind::DependsOn, 0, target))
			.collect::<Vec<_>>();
		let observations = edges
			.iter()
			.cloned()
			.map(|value| related(value, WorkItemState::Done))
			.collect::<Vec<_>>();

		assert!(ReadinessObservations::new(&observations, &[], &[]).is_ok());

		let item = item_with(0, WorkItemState::Planned, None, vec![]);

		assert!(
			work_item::assess_work_item_readiness(
				&item,
				&edges,
				&ReadinessObservations::new(&observations, &[], &[]).unwrap()
			)
			.is_ok()
		);

		let mut over = observations;

		over.push(related(
			edge(WorkItemEdgeKind::DependsOn, 0, MAX_WORK_ITEM_READINESS_RELATIONS + 1),
			WorkItemState::Done,
		));

		assert!(matches!(
			ReadinessObservations::new(&over, &[], &[]),
			Err(WorkItemError::InputLimitExceeded)
		));
	}

	#[test]
	fn readiness_assessment_is_non_mutating_and_explainable() {
		let program = WorkItemProgramRef::new(program_id(1), project_id(1));
		let objective = WorkItemObjectiveRef::new(objective_id(1), project_id(1));
		let item = item_with(1, WorkItemState::Planned, Some(program), vec![objective]);
		let before = item.clone();
		let dependency = edge(WorkItemEdgeKind::DependsOn, 1, 2);
		let blocker = edge(WorkItemEdgeKind::BlockedBy, 1, 3);
		let related = [
			related(blocker.clone(), WorkItemState::Running),
			related(dependency.clone(), WorkItemState::Review),
		];
		let programs = [WorkItemProgramObservation::new(
			program_id(1),
			project_id(1),
			7,
			ProgramState::Paused,
		)
		.unwrap()];
		let objectives = [WorkItemObjectiveObservation::new(
			objective_id(1),
			project_id(1),
			9,
			ObjectiveState::Blocked,
		)
		.unwrap()];
		let assessment = work_item::assess_work_item_readiness(
			&item,
			&[blocker, dependency],
			&ReadinessObservations::new(&related, &programs, &objectives).unwrap(),
		)
		.unwrap();

		assert_eq!(item, before);
		assert_eq!(assessment.work_item_id(), item.id());
		assert_eq!(assessment.work_item_revision(), item.revision());
		assert_eq!(
			assessment.reasons(),
			[
				ReadinessReason::ProgramInactive(program_id(1), ProgramState::Paused),
				ReadinessReason::ObjectiveInactive(objective_id(1), ObjectiveState::Blocked),
				ReadinessReason::DependencyIncomplete(work_item_id(2), WorkItemState::Review),
				ReadinessReason::BlockerActive(work_item_id(3), WorkItemState::Running),
			]
		);
	}

	#[test]
	fn readiness_reason_order_is_independent_of_input_order() {
		let item = item_with(1, WorkItemState::Planned, None, vec![]);
		let dependency = edge(WorkItemEdgeKind::DependsOn, 1, 2);
		let blocker = edge(WorkItemEdgeKind::BlockedBy, 1, 3);
		let dependency_observation = related(dependency.clone(), WorkItemState::Running);
		let blocker_observation = related(blocker.clone(), WorkItemState::Review);
		let first = work_item::assess_work_item_readiness(
			&item,
			&[blocker.clone(), dependency.clone()],
			&ReadinessObservations::new(
				&[blocker_observation.clone(), dependency_observation.clone()],
				&[],
				&[],
			)
			.unwrap(),
		)
		.unwrap();
		let second = work_item::assess_work_item_readiness(
			&item,
			&[dependency, blocker],
			&ReadinessObservations::new(&[dependency_observation, blocker_observation], &[], &[])
				.unwrap(),
		)
		.unwrap();

		assert_eq!(first, second);
	}

	#[test]
	fn readiness_reports_missing_invalid_and_unrelated_observations() {
		let program = WorkItemProgramRef::new(program_id(1), project_id(1));
		let objective = WorkItemObjectiveRef::new(objective_id(1), project_id(1));
		let item = item_with(1, WorkItemState::Inbox, Some(program), vec![objective]);
		let expected = edge(WorkItemEdgeKind::DependsOn, 1, 2);
		let unrelated = edge(WorkItemEdgeKind::BlockedBy, 1, 3);
		let observations = [related(unrelated, WorkItemState::Done)];
		let assessment = work_item::assess_work_item_readiness(
			&item,
			&[expected],
			&ReadinessObservations::new(&observations, &[], &[]).unwrap(),
		)
		.unwrap();

		assert_eq!(
			assessment.reasons(),
			[
				ReadinessReason::Lifecycle(WorkItemState::Inbox),
				ReadinessReason::ProgramObservationMissing(program_id(1)),
				ReadinessReason::ObjectiveObservationMissing(objective_id(1)),
				ReadinessReason::RelatedObservationMissing(
					WorkItemEdgeKind::DependsOn,
					work_item_id(2)
				),
				ReadinessReason::InvalidRelatedObservation(work_item_id(3)),
			]
		);
	}

	#[test]
	fn readiness_reports_an_edge_for_another_subject() {
		let item = item_with(1, WorkItemState::Planned, None, vec![]);
		let another_subject = edge(WorkItemEdgeKind::DependsOn, 2, 3);
		let assessment = work_item::assess_work_item_readiness(
			&item,
			&[another_subject],
			&ReadinessObservations::new(&[], &[], &[]).unwrap(),
		)
		.unwrap();

		assert_eq!(assessment.reasons(), [ReadinessReason::InvalidEdge(work_item_id(3))]);
	}

	#[test]
	fn duplicate_and_cross_project_context_observations_fail_closed_as_reasons() {
		let program = WorkItemProgramRef::new(program_id(1), project_id(1));
		let objective = WorkItemObjectiveRef::new(objective_id(1), project_id(1));
		let item = item_with(1, WorkItemState::Blocked, Some(program), vec![objective]);
		let duplicate_program =
			WorkItemProgramObservation::new(program_id(1), project_id(1), 1, ProgramState::Active)
				.unwrap();
		let wrong_objective = WorkItemObjectiveObservation::new(
			objective_id(1),
			project_id(2),
			1,
			ObjectiveState::Active,
		)
		.unwrap();
		let assessment = work_item::assess_work_item_readiness(
			&item,
			&[],
			&ReadinessObservations::new(
				&[],
				&[duplicate_program.clone(), duplicate_program],
				&[wrong_objective],
			)
			.unwrap(),
		)
		.unwrap();

		assert_eq!(
			assessment.reasons(),
			[
				ReadinessReason::InvalidProgramObservation(program_id(1)),
				ReadinessReason::InvalidObjectiveObservation(objective_id(1)),
			]
		);
	}

	#[test]
	fn clear_assessment_is_not_consumed_by_any_transition() {
		let mut item = item_with(1, WorkItemState::Planned, None, vec![]);
		let assessment = work_item::assess_work_item_readiness(
			&item,
			&[],
			&ReadinessObservations::new(&[], &[], &[]).unwrap(),
		)
		.unwrap();

		assert!(assessment.reasons().is_empty());
		assert_eq!(
			item.transition(1, WorkItemState::Ready, timestamp(2), provenance(2)),
			Err(WorkItemError::InvalidLifecycle)
		);
		assert_eq!(
			item.transition(1, WorkItemState::Running, timestamp(2), provenance(2)),
			Err(WorkItemError::InvalidLifecycle)
		);
		assert_eq!(
			item.transition(1, WorkItemState::Done, timestamp(2), provenance(2)),
			Err(WorkItemError::InvalidLifecycle)
		);
	}
}
