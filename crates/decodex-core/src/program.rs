//! Open-ended Program and finite Objective domain authority.

use std::{
	collections::HashSet,
	error::Error,
	fmt::{Debug, Display, Formatter},
};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use sha2::{Digest as _, Sha256};

use crate::{AgentId, PolicyRevisionId, ProjectId};

macro_rules! stable_id {
	($name:ident, $error:ident, $label:literal) => {
		#[doc = concat!("Stable canonical ", $label, " identity.")]
		#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
		pub struct $name(String);
		impl $name {
			#[doc = concat!("Parse one canonical lowercase RFC 9562 UUID-v4 ", $label, " identity.")]
			pub fn new(value: impl Into<String>) -> Result<Self, ProgramError> {
				let value = value.into();

				if !is_canonical_uuid_v4(&value) {
					return Err(ProgramError::$error);
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

stable_id!(ProgramId, InvalidProgramId, "Program");
stable_id!(ObjectiveId, InvalidObjectiveId, "Objective");
stable_id!(ObjectiveEvidenceId, InvalidEvidenceId, "Objective evidence");
stable_id!(ProgramCorrelationId, InvalidCorrelationId, "Program correlation");
stable_id!(ProgramObservationId, InvalidObservationId, "Program observation");
stable_id!(ProgramClaimId, InvalidClaimId, "Program claim");
stable_id!(ProgramProposalId, InvalidProposalId, "Program proposal");
stable_id!(ProgramEvidenceId, InvalidProgramEvidenceId, "Program evidence");
stable_id!(ProgramReviewId, InvalidReviewId, "Program review");

/// Maximum bytes in a Program or Objective display name.
pub const MAX_PROGRAM_NAME_BYTES: usize = 256;
/// Maximum bytes in an ordinary Program/Objective text field.
pub const MAX_PROGRAM_TEXT_BYTES: usize = 4_096;
/// Maximum criteria on one finite Objective.
pub const MAX_OBJECTIVE_CRITERIA: usize = 32;
/// Maximum metrics or signals on one Program.
pub const MAX_PROGRAM_OBSERVATIONS: usize = 64;
/// Maximum recent decisions in one compiled Program context.
pub const MAX_PROGRAM_CONTEXT_DECISIONS: usize = 64;
/// Maximum deterministic compiled Program-context bytes.
pub const MAX_PROGRAM_CONTEXT_BYTES: usize = 256 * 1_024;
/// Maximum accepted interval for an explicit Program review cadence.
pub const MAX_REVIEW_CADENCE_DAYS: u16 = 365;
/// Latest finite timestamp representable by durable-store and RFC 3339, in Unix microseconds.
pub const MAX_PROGRAM_TIMESTAMP_MICROSECONDS: i64 = 253_402_300_799_999_999;

/// Closed Program-domain validation failure without caller-controlled text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgramError {
	/// Program identity was not canonical UUID-v4 text.
	InvalidProgramId,
	/// Objective identity was not canonical UUID-v4 text.
	InvalidObjectiveId,
	/// Objective completion-evidence identity was not canonical UUID-v4 text.
	InvalidEvidenceId,
	/// Mutation correlation identity was not canonical UUID-v4 text.
	InvalidCorrelationId,
	/// Metric, signal, or context-decision identity was not canonical UUID-v4 text.
	InvalidObservationId,
	/// Claim identity was not canonical UUID-v4 text.
	InvalidClaimId,
	/// Proposal identity was not canonical UUID-v4 text.
	InvalidProposalId,
	/// Program Evidence identity was not canonical UUID-v4 text.
	InvalidProgramEvidenceId,
	/// Program Review identity was not canonical UUID-v4 text.
	InvalidReviewId,
	/// A bounded ordinary text value was empty, oversized, or contained controls.
	InvalidText,
	/// A symbolic key was not canonical lowercase snake case.
	InvalidSymbol,
	/// A collection was empty, oversized, or repeated a stable key.
	InvalidCollection,
	/// A persisted or incremented revision was outside its positive domain.
	InvalidRevision,
	/// A timestamp was negative, unbounded, or chronologically inconsistent.
	InvalidChronology,
	/// Review cadence was outside its supported interval.
	InvalidReviewCadence,
	/// Requested lifecycle transition was not legal.
	InvalidLifecycle,
	/// Completion evidence did not bind the exact Objective revision and Project.
	InvalidCompletion,
	/// Program context exceeded its deterministic bound.
	ContextTooLarge,
	/// Ordinary domain data contained concrete credential material.
	CredentialRejected,
}
impl Error for ProgramError {}

impl Display for ProgramError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(match self {
			Self::InvalidProgramId => "invalid Program identity",
			Self::InvalidObjectiveId => "invalid Objective identity",
			Self::InvalidEvidenceId => "invalid Objective evidence identity",
			Self::InvalidCorrelationId => "invalid Program correlation identity",
			Self::InvalidObservationId => "invalid Program observation identity",
			Self::InvalidClaimId => "invalid Program claim identity",
			Self::InvalidProposalId => "invalid Program proposal identity",
			Self::InvalidProgramEvidenceId => "invalid Program evidence identity",
			Self::InvalidReviewId => "invalid Program review identity",
			Self::InvalidText => "invalid bounded Program text",
			Self::InvalidSymbol => "invalid canonical Program symbol",
			Self::InvalidCollection => "invalid bounded Program collection",
			Self::InvalidRevision => "invalid Program or Objective revision",
			Self::InvalidChronology => "invalid Program or Objective chronology",
			Self::InvalidReviewCadence => "invalid Program review cadence",
			Self::InvalidLifecycle => "invalid Program or Objective lifecycle transition",
			Self::InvalidCompletion => "invalid Objective completion evidence",
			Self::ContextTooLarge => "compiled Program context exceeds its byte bound",
			Self::CredentialRejected => "credential-bearing Program data rejected",
		})
	}
}

/// Database-compatible finite time represented as Unix epoch microseconds.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ProgramTimestamp(i64);
impl ProgramTimestamp {
	/// Validate one finite non-negative timestamp.
	pub const fn from_unix_microseconds(value: i64) -> Result<Self, ProgramError> {
		if value < 0 || value > MAX_PROGRAM_TIMESTAMP_MICROSECONDS {
			Err(ProgramError::InvalidChronology)
		} else {
			Ok(Self(value))
		}
	}

	/// Read Unix epoch microseconds.
	pub const fn unix_microseconds(self) -> i64 {
		self.0
	}
}

/// Open-ended Program lifecycle.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgramState {
	/// Responsibility is operating normally.
	Active,
	/// Responsibility requires Lead review without pretending it is complete.
	NeedsAttention,
	/// Responsibility cannot currently progress.
	Blocked,
	/// Responsibility is intentionally inactive but resumable.
	Paused,
	/// Responsibility is permanently closed and retained for readback.
	Retired,
}
impl ProgramState {
	/// Canonical persistence spelling.
	pub const fn as_str(self) -> &'static str {
		match self {
			Self::Active => "active",
			Self::NeedsAttention => "needs_attention",
			Self::Blocked => "blocked",
			Self::Paused => "paused",
			Self::Retired => "retired",
		}
	}

	/// Whether this state can transition to `next`.
	pub const fn can_transition_to(self, next: Self) -> bool {
		match self {
			Self::Active => {
				matches!(next, Self::NeedsAttention | Self::Blocked | Self::Paused | Self::Retired)
			},
			Self::NeedsAttention => {
				matches!(next, Self::Active | Self::Blocked | Self::Paused | Self::Retired)
			},
			Self::Blocked => {
				matches!(next, Self::Active | Self::NeedsAttention | Self::Paused | Self::Retired)
			},
			Self::Paused => matches!(next, Self::Active | Self::Retired),
			Self::Retired => false,
		}
	}
}

/// Finite Objective lifecycle.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectiveState {
	/// Outcome is defined but has not begun.
	Proposed,
	/// Work toward the finite outcome is active.
	Active,
	/// Progress toward the outcome is temporarily blocked.
	Blocked,
	/// Immutable acceptance and validation evidence established the outcome.
	Achieved,
	/// Outcome ended intentionally without achievement.
	Abandoned,
}

/// Closed evidence kinds required by the first Program review loop.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgramEvidenceKind {
	/// Reproducible validation with a deterministic command, check, or equivalent witness.
	DeterministicValidation,
	/// Observation from outside the produced artifact or model response.
	External,
}
impl ProgramEvidenceKind {
	/// Canonical persistence spelling.
	pub const fn as_str(self) -> &'static str {
		match self {
			Self::DeterministicValidation => "deterministic_validation",
			Self::External => "external",
		}
	}
}

/// Evidence-backed classification recorded by one Program review.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgramReviewClassification {
	/// An external or user-visible result improved.
	OutcomeProgress,
	/// Material uncertainty decreased.
	KnowledgeProgress,
	/// A reusable ability or validation mechanism improved.
	CapabilityProgress,
	/// The cycle produced no material delta.
	NoMaterialChange,
	/// Evidence shows that the state became worse.
	Regression,
	/// Evidence is missing, stale, ambiguous, or contradictory.
	Unknown,
}
impl ProgramReviewClassification {
	/// Canonical persistence spelling.
	pub const fn as_str(self) -> &'static str {
		match self {
			Self::OutcomeProgress => "outcome_progress",
			Self::KnowledgeProgress => "knowledge_progress",
			Self::CapabilityProgress => "capability_progress",
			Self::NoMaterialChange => "no_material_change",
			Self::Regression => "regression",
			Self::Unknown => "unknown",
		}
	}
}
impl ObjectiveState {
	/// Canonical persistence spelling.
	pub const fn as_str(self) -> &'static str {
		match self {
			Self::Proposed => "proposed",
			Self::Active => "active",
			Self::Blocked => "blocked",
			Self::Achieved => "achieved",
			Self::Abandoned => "abandoned",
		}
	}

	/// Whether an ordinary lifecycle command may perform this transition.
	pub const fn can_transition_to(self, next: Self) -> bool {
		matches!(
			(self, next),
			(Self::Proposed, Self::Active | Self::Abandoned)
				| (Self::Active, Self::Blocked | Self::Abandoned)
				| (Self::Blocked, Self::Active | Self::Abandoned)
		)
	}
}

/// Bounded provenance supplied for every Program/Objective mutation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramProvenance {
	actor_id: AgentId,
	correlation_id: ProgramCorrelationId,
	summary: String,
}
impl ProgramProvenance {
	/// Construct credential-negative active-Lead provenance.
	///
	/// Storage verifies the Agent role, state, and Project scope transactionally.
	pub fn new(
		actor_id: AgentId,
		correlation_id: ProgramCorrelationId,
		summary: impl Into<String>,
	) -> Result<Self, ProgramError> {
		let summary = summary.into();

		validate_text(&summary, MAX_PROGRAM_TEXT_BYTES)?;

		Ok(Self { actor_id, correlation_id, summary })
	}

	/// Stable canonical Agent whose authority storage verifies.
	pub const fn actor_id(&self) -> &AgentId {
		&self.actor_id
	}

	/// Stable correlation identity.
	pub const fn correlation_id(&self) -> &ProgramCorrelationId {
		&self.correlation_id
	}

	/// Opaque bounded provenance summary.
	pub fn summary(&self) -> &str {
		&self.summary
	}
}

/// Bounded recurring review schedule for an open-ended Program.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReviewCadence {
	interval_days: u16,
	next_review_at: ProgramTimestamp,
}
impl ReviewCadence {
	/// Construct a cadence with an explicit next review.
	pub const fn new(
		interval_days: u16,
		next_review_at: ProgramTimestamp,
	) -> Result<Self, ProgramError> {
		if interval_days == 0 || interval_days > MAX_REVIEW_CADENCE_DAYS {
			return Err(ProgramError::InvalidReviewCadence);
		}

		Ok(Self { interval_days, next_review_at })
	}

	/// Whole days between reviews.
	pub const fn interval_days(self) -> u16 {
		self.interval_days
	}

	/// Explicit next review time.
	pub const fn next_review_at(self) -> ProgramTimestamp {
		self.next_review_at
	}
}

/// Source and observation time for a metric, signal, or context decision.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ProgramObservationProvenance {
	source: String,
	observed_at_microseconds: i64,
}
impl ProgramObservationProvenance {
	/// Construct bounded inspectable observation provenance.
	pub fn new(
		source: impl Into<String>,
		observed_at: ProgramTimestamp,
	) -> Result<Self, ProgramError> {
		let source = source.into();

		validate_text(&source, MAX_PROGRAM_NAME_BYTES)?;

		Ok(Self { source, observed_at_microseconds: observed_at.unix_microseconds() })
	}

	/// Inspectable source text; ordinary thread/conversation mentions remain data.
	pub fn source(&self) -> &str {
		&self.source
	}

	/// Observation time.
	pub fn observed_at(&self) -> ProgramTimestamp {
		ProgramTimestamp(self.observed_at_microseconds)
	}

	fn validate(&self) -> Result<(), ProgramError> {
		validate_text(&self.source, MAX_PROGRAM_NAME_BYTES)?;

		ProgramTimestamp::from_unix_microseconds(self.observed_at_microseconds)?;

		Ok(())
	}
}

/// One bounded provenance-bearing Program metric.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ProgramMetric {
	key: String,
	value: String,
	unit: String,
	provenance: ProgramObservationProvenance,
}
impl ProgramMetric {
	/// Construct one canonical metric.
	pub fn new(
		key: impl Into<String>,
		value: impl Into<String>,
		unit: impl Into<String>,
		provenance: ProgramObservationProvenance,
	) -> Result<Self, ProgramError> {
		let metric = Self { key: key.into(), value: value.into(), unit: unit.into(), provenance };

		metric.validate()?;

		Ok(metric)
	}

	/// Stable lowercase metric key.
	pub fn key(&self) -> &str {
		&self.key
	}

	/// Bounded metric value.
	pub fn value(&self) -> &str {
		&self.value
	}

	/// Bounded metric unit.
	pub fn unit(&self) -> &str {
		&self.unit
	}

	/// Inspectable observation provenance.
	pub const fn provenance(&self) -> &ProgramObservationProvenance {
		&self.provenance
	}

	fn validate(&self) -> Result<(), ProgramError> {
		validate_symbol(&self.key)?;
		validate_text(&self.value, MAX_PROGRAM_NAME_BYTES)?;
		validate_text(&self.unit, 64)?;

		self.provenance.validate()
	}
}

/// One bounded provenance-bearing Program signal.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ProgramSignal {
	id: ProgramObservationId,
	kind: String,
	summary: String,
	provenance: ProgramObservationProvenance,
}
impl ProgramSignal {
	/// Construct one canonical signal.
	pub fn new(
		id: ProgramObservationId,
		kind: impl Into<String>,
		summary: impl Into<String>,
		provenance: ProgramObservationProvenance,
	) -> Result<Self, ProgramError> {
		let signal = Self { id, kind: kind.into(), summary: summary.into(), provenance };

		signal.validate()?;

		Ok(signal)
	}

	/// Stable signal identity.
	pub const fn id(&self) -> &ProgramObservationId {
		&self.id
	}

	/// Canonical signal kind.
	pub fn kind(&self) -> &str {
		&self.kind
	}

	/// Inspectable bounded signal summary.
	pub fn summary(&self) -> &str {
		&self.summary
	}

	/// Inspectable observation provenance.
	pub const fn provenance(&self) -> &ProgramObservationProvenance {
		&self.provenance
	}

	fn validate(&self) -> Result<(), ProgramError> {
		validate_symbol(&self.kind)?;
		validate_text(&self.summary, MAX_PROGRAM_TEXT_BYTES)?;

		self.provenance.validate()
	}
}

/// Open-ended responsibility owned by one canonical Project Lead under exact policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Program {
	id: ProgramId,
	project_id: ProjectId,
	owner_agent_id: AgentId,
	name: String,
	responsibility: String,
	state: ProgramState,
	policy_revision_id: PolicyRevisionId,
	review_cadence: ReviewCadence,
	metrics: Vec<ProgramMetric>,
	signals: Vec<ProgramSignal>,
	revision: u64,
}
impl Program {
	/// Create revision one of an active open-ended responsibility.
	pub fn new(
		id: ProgramId,
		project_id: ProjectId,
		owner_agent_id: AgentId,
		name: impl Into<String>,
		responsibility: impl Into<String>,
		policy_revision_id: PolicyRevisionId,
		review_cadence: ReviewCadence,
	) -> Result<Self, ProgramError> {
		Self::from_stored(
			id,
			project_id,
			owner_agent_id,
			name.into(),
			responsibility.into(),
			ProgramState::Active,
			policy_revision_id,
			review_cadence,
			Vec::new(),
			Vec::new(),
			1,
		)
	}

	/// Validate deterministic persistence readback.
	#[allow(clippy::too_many_arguments)]
	pub fn from_stored(
		id: ProgramId,
		project_id: ProjectId,
		owner_agent_id: AgentId,
		name: String,
		responsibility: String,
		state: ProgramState,
		policy_revision_id: PolicyRevisionId,
		review_cadence: ReviewCadence,
		metrics: Vec<ProgramMetric>,
		signals: Vec<ProgramSignal>,
		revision: u64,
	) -> Result<Self, ProgramError> {
		validate_text(&name, MAX_PROGRAM_NAME_BYTES)?;
		validate_text(&responsibility, MAX_PROGRAM_TEXT_BYTES)?;

		if policy_revision_id.project_id() != &project_id || revision == 0 {
			return Err(ProgramError::InvalidRevision);
		}

		validate_observations(&metrics, &signals)?;

		Ok(Self {
			id,
			project_id,
			owner_agent_id,
			name,
			responsibility,
			state,
			policy_revision_id,
			review_cadence,
			metrics,
			signals,
			revision,
		})
	}

	/// Apply one legal expected-revision lifecycle transition.
	pub fn transition(
		&mut self,
		expected_revision: u64,
		state: ProgramState,
	) -> Result<(), ProgramError> {
		self.require_revision(expected_revision)?;

		if !self.state.can_transition_to(state) {
			return Err(ProgramError::InvalidLifecycle);
		}

		self.advance_revision()?;

		self.state = state;

		Ok(())
	}

	/// Replace mutable metric/signal/review context at one expected revision.
	pub fn replace_context(
		&mut self,
		expected_revision: u64,
		review_cadence: ReviewCadence,
		metrics: Vec<ProgramMetric>,
		signals: Vec<ProgramSignal>,
	) -> Result<(), ProgramError> {
		self.require_revision(expected_revision)?;

		if self.state == ProgramState::Retired {
			return Err(ProgramError::InvalidLifecycle);
		}

		validate_observations(&metrics, &signals)?;

		if self.review_cadence == review_cadence
			&& self.metrics == metrics
			&& self.signals == signals
		{
			return Err(ProgramError::InvalidLifecycle);
		}

		self.advance_revision()?;

		self.review_cadence = review_cadence;
		self.metrics = metrics;
		self.signals = signals;

		Ok(())
	}

	fn require_revision(&self, expected_revision: u64) -> Result<(), ProgramError> {
		if expected_revision == 0 || expected_revision != self.revision {
			Err(ProgramError::InvalidRevision)
		} else {
			Ok(())
		}
	}

	fn advance_revision(&mut self) -> Result<(), ProgramError> {
		self.revision = self.revision.checked_add(1).ok_or(ProgramError::InvalidRevision)?;

		Ok(())
	}

	/// Stable Program identity.
	pub const fn id(&self) -> &ProgramId {
		&self.id
	}

	/// Owning canonical Project.
	pub const fn project_id(&self) -> &ProjectId {
		&self.project_id
	}

	/// Canonical Agent identity assigned as owner.
	pub const fn owner_agent_id(&self) -> &AgentId {
		&self.owner_agent_id
	}

	/// Bounded display name.
	pub fn name(&self) -> &str {
		&self.name
	}

	/// Open-ended responsibility statement.
	pub fn responsibility(&self) -> &str {
		&self.responsibility
	}

	/// Current open-ended lifecycle.
	pub const fn state(&self) -> ProgramState {
		self.state
	}

	/// Exact accepted Project-owned Policy revision.
	pub const fn policy_revision_id(&self) -> &PolicyRevisionId {
		&self.policy_revision_id
	}

	/// Explicit review cadence.
	pub const fn review_cadence(&self) -> ReviewCadence {
		self.review_cadence
	}

	/// Bounded metrics.
	pub fn metrics(&self) -> &[ProgramMetric] {
		&self.metrics
	}

	/// Bounded signals.
	pub fn signals(&self) -> &[ProgramSignal] {
		&self.signals
	}

	/// Positive optimistic revision.
	pub const fn revision(&self) -> u64 {
		self.revision
	}
}

/// Immutable Objective-level acceptance and validation evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectiveCompletionEvidence {
	id: ObjectiveEvidenceId,
	objective_id: ObjectiveId,
	project_id: ProjectId,
	objective_revision: u64,
	objective_updated_at: Option<ProgramTimestamp>,
	acceptance_result: String,
	accepted_by: AgentId,
	accepted_at: ProgramTimestamp,
	acceptance_provenance: String,
	validation_result: String,
	validated_by: AgentId,
	validated_at: ProgramTimestamp,
	validation_provenance: String,
	correlation_id: ProgramCorrelationId,
	recorded_at: ProgramTimestamp,
}
impl ObjectiveCompletionEvidence {
	/// Construct proposed evidence; storage authors `recorded_at` after authority checks.
	#[allow(clippy::too_many_arguments)]
	pub fn proposed(
		id: ObjectiveEvidenceId,
		objective_id: ObjectiveId,
		project_id: ProjectId,
		objective_revision: u64,
		acceptance_result: impl Into<String>,
		accepted_by: AgentId,
		accepted_at: ProgramTimestamp,
		acceptance_provenance: impl Into<String>,
		validation_result: impl Into<String>,
		validated_by: AgentId,
		validated_at: ProgramTimestamp,
		validation_provenance: impl Into<String>,
		correlation_id: ProgramCorrelationId,
	) -> Result<Self, ProgramError> {
		Self::from_stored(
			id,
			objective_id,
			project_id,
			objective_revision,
			None,
			acceptance_result.into(),
			accepted_by,
			accepted_at,
			acceptance_provenance.into(),
			validation_result.into(),
			validated_by,
			validated_at,
			validation_provenance.into(),
			correlation_id,
			validated_at,
		)
	}

	/// Validate exact immutable persistence readback and chronology.
	#[allow(clippy::too_many_arguments)]
	pub fn from_stored(
		id: ObjectiveEvidenceId,
		objective_id: ObjectiveId,
		project_id: ProjectId,
		objective_revision: u64,
		objective_updated_at: Option<ProgramTimestamp>,
		acceptance_result: String,
		accepted_by: AgentId,
		accepted_at: ProgramTimestamp,
		acceptance_provenance: String,
		validation_result: String,
		validated_by: AgentId,
		validated_at: ProgramTimestamp,
		validation_provenance: String,
		correlation_id: ProgramCorrelationId,
		recorded_at: ProgramTimestamp,
	) -> Result<Self, ProgramError> {
		if objective_revision == 0
			|| objective_updated_at.is_some_and(|updated_at| updated_at > accepted_at)
			|| accepted_at > validated_at
			|| validated_at > recorded_at
		{
			return Err(ProgramError::InvalidCompletion);
		}

		for value in
			[&acceptance_result, &acceptance_provenance, &validation_result, &validation_provenance]
		{
			validate_text(value, MAX_PROGRAM_TEXT_BYTES)?;
		}

		Ok(Self {
			id,
			objective_id,
			project_id,
			objective_revision,
			objective_updated_at,
			acceptance_result,
			accepted_by,
			accepted_at,
			acceptance_provenance,
			validation_result,
			validated_by,
			validated_at,
			validation_provenance,
			correlation_id,
			recorded_at,
		})
	}

	/// Stable immutable evidence identity.
	pub const fn id(&self) -> &ObjectiveEvidenceId {
		&self.id
	}

	/// Exact Objective identity.
	pub const fn objective_id(&self) -> &ObjectiveId {
		&self.objective_id
	}

	/// Exact owning Project identity.
	pub const fn project_id(&self) -> &ProjectId {
		&self.project_id
	}

	/// Exact pre-achievement Objective revision.
	pub const fn objective_revision(&self) -> u64 {
		self.objective_revision
	}

	/// Database-authored timestamp of the exact pre-achievement Objective revision.
	pub const fn objective_updated_at(&self) -> Option<ProgramTimestamp> {
		self.objective_updated_at
	}

	/// Non-empty acceptance result.
	pub fn acceptance_result(&self) -> &str {
		&self.acceptance_result
	}

	/// Canonical accepting Agent.
	pub const fn accepted_by(&self) -> &AgentId {
		&self.accepted_by
	}

	/// Acceptance observation time.
	pub const fn accepted_at(&self) -> ProgramTimestamp {
		self.accepted_at
	}

	/// Inspectable acceptance provenance.
	pub fn acceptance_provenance(&self) -> &str {
		&self.acceptance_provenance
	}

	/// Non-empty validation result.
	pub fn validation_result(&self) -> &str {
		&self.validation_result
	}

	/// Canonical validating Agent.
	pub const fn validated_by(&self) -> &AgentId {
		&self.validated_by
	}

	/// Validation observation time.
	pub const fn validated_at(&self) -> ProgramTimestamp {
		self.validated_at
	}

	/// Inspectable validation provenance.
	pub fn validation_provenance(&self) -> &str {
		&self.validation_provenance
	}

	/// Stable correlation identity.
	pub const fn correlation_id(&self) -> &ProgramCorrelationId {
		&self.correlation_id
	}

	/// Database-authored persistence time.
	pub const fn recorded_at(&self) -> ProgramTimestamp {
		self.recorded_at
	}
}

/// Finite outcome attached directly to a Project and optionally one same-Project Program.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Objective {
	id: ObjectiveId,
	project_id: ProjectId,
	program_id: Option<ProgramId>,
	outcome: String,
	acceptance_criteria: Vec<String>,
	validation_criteria: Vec<String>,
	target_at: ProgramTimestamp,
	state: ObjectiveState,
	revision: u64,
	completion: Option<ObjectiveCompletionEvidence>,
}
impl Objective {
	/// Create revision one of a finite proposed Objective.
	pub fn new(
		id: ObjectiveId,
		project_id: ProjectId,
		program_id: Option<ProgramId>,
		outcome: impl Into<String>,
		acceptance_criteria: Vec<String>,
		validation_criteria: Vec<String>,
		target_at: ProgramTimestamp,
	) -> Result<Self, ProgramError> {
		Self::from_stored(
			id,
			project_id,
			program_id,
			outcome.into(),
			acceptance_criteria,
			validation_criteria,
			target_at,
			ObjectiveState::Proposed,
			1,
			None,
		)
	}

	/// Validate deterministic persistence readback.
	#[allow(clippy::too_many_arguments)]
	pub fn from_stored(
		id: ObjectiveId,
		project_id: ProjectId,
		program_id: Option<ProgramId>,
		outcome: String,
		acceptance_criteria: Vec<String>,
		validation_criteria: Vec<String>,
		target_at: ProgramTimestamp,
		state: ObjectiveState,
		revision: u64,
		completion: Option<ObjectiveCompletionEvidence>,
	) -> Result<Self, ProgramError> {
		validate_text(&outcome, MAX_PROGRAM_TEXT_BYTES)?;
		validate_criteria(&acceptance_criteria)?;
		validate_criteria(&validation_criteria)?;

		if revision == 0 {
			return Err(ProgramError::InvalidRevision);
		}

		match (&completion, state) {
			(Some(evidence), ObjectiveState::Achieved)
				if evidence.objective_id() == &id
					&& evidence.project_id() == &project_id
					&& evidence.objective_revision().checked_add(1) == Some(revision) => {},
			(None, state) if state != ObjectiveState::Achieved => {},
			_ => return Err(ProgramError::InvalidCompletion),
		}

		Ok(Self {
			id,
			project_id,
			program_id,
			outcome,
			acceptance_criteria,
			validation_criteria,
			target_at,
			state,
			revision,
			completion,
		})
	}

	/// Apply one ordinary legal expected-revision transition; achievement is separate.
	pub fn transition(
		&mut self,
		expected_revision: u64,
		state: ObjectiveState,
	) -> Result<(), ProgramError> {
		self.require_revision(expected_revision)?;

		if !self.state.can_transition_to(state) {
			return Err(ProgramError::InvalidLifecycle);
		}

		self.revision = self.revision.checked_add(1).ok_or(ProgramError::InvalidRevision)?;
		self.state = state;

		Ok(())
	}

	/// Achieve only through exact revision-bound acceptance and validation evidence.
	pub fn achieve(
		&mut self,
		expected_revision: u64,
		evidence: ObjectiveCompletionEvidence,
	) -> Result<(), ProgramError> {
		self.require_revision(expected_revision)?;

		if !matches!(self.state, ObjectiveState::Active | ObjectiveState::Blocked)
			|| evidence.objective_id() != &self.id
			|| evidence.project_id() != &self.project_id
			|| evidence.objective_revision() != expected_revision
		{
			return Err(ProgramError::InvalidCompletion);
		}

		self.revision = self.revision.checked_add(1).ok_or(ProgramError::InvalidRevision)?;
		self.state = ObjectiveState::Achieved;
		self.completion = Some(evidence);

		Ok(())
	}

	fn require_revision(&self, expected_revision: u64) -> Result<(), ProgramError> {
		if expected_revision == 0 || expected_revision != self.revision {
			Err(ProgramError::InvalidRevision)
		} else {
			Ok(())
		}
	}

	/// Stable Objective identity.
	pub const fn id(&self) -> &ObjectiveId {
		&self.id
	}

	/// Owning canonical Project.
	pub const fn project_id(&self) -> &ProjectId {
		&self.project_id
	}

	/// Optional same-Project open-ended Program context.
	pub const fn program_id(&self) -> Option<&ProgramId> {
		self.program_id.as_ref()
	}

	/// Finite outcome statement.
	pub fn outcome(&self) -> &str {
		&self.outcome
	}

	/// Explicit acceptance criteria.
	pub fn acceptance_criteria(&self) -> &[String] {
		&self.acceptance_criteria
	}

	/// Explicit validation criteria.
	pub fn validation_criteria(&self) -> &[String] {
		&self.validation_criteria
	}

	/// Finite target horizon.
	pub const fn target_at(&self) -> ProgramTimestamp {
		self.target_at
	}

	/// Current finite lifecycle.
	pub const fn state(&self) -> ObjectiveState {
		self.state
	}

	/// Positive optimistic revision.
	pub const fn revision(&self) -> u64 {
		self.revision
	}

	/// Immutable outcome evidence, present only when achieved.
	pub const fn completion(&self) -> Option<&ObjectiveCompletionEvidence> {
		self.completion.as_ref()
	}
}

/// One bounded recent decision included as pure Program context data.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramContextDecision {
	id: ProgramObservationId,
	summary: String,
	provenance: ProgramObservationProvenance,
}
impl ProgramContextDecision {
	/// Construct a bounded inspectable decision.
	pub fn new(
		id: ProgramObservationId,
		summary: impl Into<String>,
		provenance: ProgramObservationProvenance,
	) -> Result<Self, ProgramError> {
		let summary = summary.into();

		validate_text(&summary, MAX_PROGRAM_TEXT_BYTES)?;

		provenance.validate()?;

		Ok(Self { id, summary, provenance })
	}
}

/// Closed optional automation quiet period retained only as context data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProgramQuietPeriod {
	start: ProgramTimestamp,
	end: ProgramTimestamp,
}
impl ProgramQuietPeriod {
	/// Construct a finite ordered period.
	pub const fn new(start: ProgramTimestamp, end: ProgramTimestamp) -> Result<Self, ProgramError> {
		if start.0 >= end.0 {
			Err(ProgramError::InvalidChronology)
		} else {
			Ok(Self { start, end })
		}
	}
}

/// Pure bounded inputs for deterministic Program-context compilation.
pub struct ProgramContextInput<'a> {
	/// Authoritative Program snapshot.
	pub program: &'a Program,
	/// Recent inspectable decisions.
	pub recent_decisions: Vec<ProgramContextDecision>,
	/// Optional data-only quiet period.
	pub quiet_period: Option<ProgramQuietPeriod>,
}

/// Deterministic Program data with no runtime, thread, session, or conversation identity.
#[derive(Clone, Eq, PartialEq)]
pub struct ProgramContext {
	program_id: ProgramId,
	program_revision: u64,
	bytes: Vec<u8>,
	digest: [u8; 32],
}
impl ProgramContext {
	/// Source Program identity.
	pub const fn program_id(&self) -> &ProgramId {
		&self.program_id
	}

	/// Exact source Program revision.
	pub const fn program_revision(&self) -> u64 {
		self.program_revision
	}

	/// Canonical bounded context encoding.
	pub fn bytes(&self) -> &[u8] {
		&self.bytes
	}

	/// SHA-256 of the canonical context encoding.
	pub const fn digest(&self) -> [u8; 32] {
		self.digest
	}
}
impl Debug for ProgramContext {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("ProgramContext")
			.field("program_id", &self.program_id)
			.field("program_revision", &self.program_revision)
			.field("byte_length", &self.bytes.len())
			.field("digest", &self.digest)
			.finish()
	}
}

/// Compile bounded Program data without creating or owning any runtime identity.
pub fn compile_program_context(
	mut input: ProgramContextInput<'_>,
) -> Result<ProgramContext, ProgramError> {
	if input.recent_decisions.len() > MAX_PROGRAM_CONTEXT_DECISIONS {
		return Err(ProgramError::InvalidCollection);
	}

	input.recent_decisions.sort_by(|left, right| left.id.cmp(&right.id));

	if input.recent_decisions.windows(2).any(|pair| pair[0].id == pair[1].id) {
		return Err(ProgramError::InvalidCollection);
	}

	let mut metrics = input.program.metrics.clone();
	let mut signals = input.program.signals.clone();

	metrics.sort_by(|left, right| left.key.cmp(&right.key));
	signals.sort_by(|left, right| left.id.cmp(&right.id));

	let mut bytes = b"decodex/program-context/1\0".to_vec();

	push_text(&mut bytes, input.program.id.as_str())?;
	push_u64(&mut bytes, input.program.revision);
	push_text(&mut bytes, input.program.project_id.as_str())?;
	push_text(&mut bytes, input.program.owner_agent_id.as_str())?;
	push_text(&mut bytes, input.program.name())?;
	push_text(&mut bytes, input.program.responsibility())?;
	push_text(&mut bytes, input.program.state.as_str())?;
	push_text(&mut bytes, input.program.policy_revision_id.policy_id().as_str())?;
	push_u64(&mut bytes, input.program.policy_revision_id.revision().get());
	push_u64(&mut bytes, u64::from(input.program.review_cadence.interval_days()));
	push_i64(&mut bytes, input.program.review_cadence.next_review_at().unix_microseconds());
	push_u64(&mut bytes, metrics.len() as u64);

	for metric in metrics {
		push_text(&mut bytes, metric.key())?;
		push_text(&mut bytes, metric.value())?;
		push_text(&mut bytes, metric.unit())?;
		push_observation_provenance(&mut bytes, metric.provenance())?;
	}

	push_u64(&mut bytes, signals.len() as u64);

	for signal in signals {
		push_text(&mut bytes, signal.id().as_str())?;
		push_text(&mut bytes, signal.kind())?;
		push_text(&mut bytes, signal.summary())?;
		push_observation_provenance(&mut bytes, signal.provenance())?;
	}

	push_u64(&mut bytes, input.recent_decisions.len() as u64);

	for decision in input.recent_decisions {
		push_text(&mut bytes, decision.id.as_str())?;
		push_text(&mut bytes, &decision.summary)?;
		push_observation_provenance(&mut bytes, &decision.provenance)?;
	}

	match input.quiet_period {
		Some(period) => {
			bytes.push(1);

			push_i64(&mut bytes, period.start.unix_microseconds());
			push_i64(&mut bytes, period.end.unix_microseconds());
		},
		None => bytes.push(0),
	}

	if bytes.len() > MAX_PROGRAM_CONTEXT_BYTES {
		return Err(ProgramError::ContextTooLarge);
	}

	let digest = Sha256::digest(&bytes).into();

	Ok(ProgramContext {
		program_id: input.program.id.clone(),
		program_revision: input.program.revision,
		bytes,
		digest,
	})
}

fn validate_observations(
	metrics: &[ProgramMetric],
	signals: &[ProgramSignal],
) -> Result<(), ProgramError> {
	if metrics.len() > MAX_PROGRAM_OBSERVATIONS || signals.len() > MAX_PROGRAM_OBSERVATIONS {
		return Err(ProgramError::InvalidCollection);
	}

	let mut metric_keys = HashSet::new();

	for metric in metrics {
		metric.validate()?;

		if !metric_keys.insert(metric.key()) {
			return Err(ProgramError::InvalidCollection);
		}
	}

	let mut signal_ids = HashSet::new();

	for signal in signals {
		signal.validate()?;

		if !signal_ids.insert(signal.id()) {
			return Err(ProgramError::InvalidCollection);
		}
	}

	Ok(())
}

fn validate_criteria(values: &[String]) -> Result<(), ProgramError> {
	if values.is_empty() || values.len() > MAX_OBJECTIVE_CRITERIA {
		return Err(ProgramError::InvalidCollection);
	}

	let mut unique = HashSet::new();

	for value in values {
		validate_text(value, MAX_PROGRAM_TEXT_BYTES)?;

		if !unique.insert(value) {
			return Err(ProgramError::InvalidCollection);
		}
	}

	Ok(())
}

fn validate_symbol(value: &str) -> Result<(), ProgramError> {
	if value.is_empty()
		|| value.len() > 64
		|| !value.bytes().enumerate().all(|(index, byte)| {
			if index == 0 {
				byte.is_ascii_lowercase()
			} else {
				byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'
			}
		}) {
		return Err(ProgramError::InvalidSymbol);
	}

	Ok(())
}

fn validate_text(value: &str, maximum: usize) -> Result<(), ProgramError> {
	if value.is_empty() || value.len() > maximum || value.chars().any(char::is_control) {
		return Err(ProgramError::InvalidText);
	}
	if crate::contains_credential_material(value) {
		return Err(ProgramError::CredentialRejected);
	}

	Ok(())
}

fn push_observation_provenance(
	bytes: &mut Vec<u8>,
	provenance: &ProgramObservationProvenance,
) -> Result<(), ProgramError> {
	push_text(bytes, provenance.source())?;
	push_i64(bytes, provenance.observed_at().unix_microseconds());

	Ok(())
}

fn push_text(bytes: &mut Vec<u8>, value: &str) -> Result<(), ProgramError> {
	let length = u32::try_from(value.len()).map_err(|_| ProgramError::ContextTooLarge)?;

	bytes.extend_from_slice(&length.to_be_bytes());
	bytes.extend_from_slice(value.as_bytes());

	Ok(())
}

fn push_u64(bytes: &mut Vec<u8>, value: u64) {
	bytes.extend_from_slice(&value.to_be_bytes());
}

fn push_i64(bytes: &mut Vec<u8>, value: i64) {
	bytes.extend_from_slice(&value.to_be_bytes());
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
		AgentId, Objective, ObjectiveCompletionEvidence, ObjectiveEvidenceId, ObjectiveId,
		ObjectiveState, PolicyId, PolicyRevision, PolicyRevisionId, Program,
		ProgramContextDecision, ProgramContextInput, ProgramCorrelationId, ProgramError, ProgramId,
		ProgramMetric, ProgramObservationId, ProgramObservationProvenance, ProgramQuietPeriod,
		ProgramSignal, ProgramState, ProgramTimestamp, ProjectId, ReviewCadence,
	};

	fn timestamp(value: i64) -> ProgramTimestamp {
		ProgramTimestamp::from_unix_microseconds(value).unwrap()
	}

	fn project_id() -> ProjectId {
		ProjectId::new("10000000-0000-4000-8000-000000000001").unwrap()
	}

	fn agent_id(suffix: &str) -> AgentId {
		AgentId::new(format!("20000000-0000-4000-8000-{suffix}")).unwrap()
	}

	fn program_id() -> ProgramId {
		ProgramId::new("30000000-0000-4000-8000-000000000001").unwrap()
	}

	fn policy_revision() -> PolicyRevisionId {
		PolicyRevisionId::new(
			project_id(),
			PolicyId::new("40000000-0000-4000-8000-000000000001").unwrap(),
			PolicyRevision::new(1).unwrap(),
		)
	}

	fn program() -> Program {
		Program::new(
			program_id(),
			project_id(),
			agent_id("000000000001"),
			"Search visibility",
			"Sustain SEO and GEO quality without fake completion",
			policy_revision(),
			ReviewCadence::new(7, timestamp(10_000_000)).unwrap(),
		)
		.unwrap()
	}

	fn objective() -> Objective {
		Objective::new(
			ObjectiveId::new("50000000-0000-4000-8000-000000000001").unwrap(),
			project_id(),
			Some(program_id()),
			"Publish the finite Q3 search baseline",
			vec!["Baseline is accepted".into()],
			vec!["Independent query fixture passes".into()],
			timestamp(20_000_000),
		)
		.unwrap()
	}

	fn evidence(revision: u64) -> ObjectiveCompletionEvidence {
		ObjectiveCompletionEvidence::from_stored(
			ObjectiveEvidenceId::new("60000000-0000-4000-8000-000000000001").unwrap(),
			ObjectiveId::new("50000000-0000-4000-8000-000000000001").unwrap(),
			project_id(),
			revision,
			Some(timestamp(14_000_000)),
			"Acceptance criteria satisfied".into(),
			agent_id("000000000001"),
			timestamp(15_000_000),
			"Lead acceptance record".into(),
			"Validation criteria satisfied".into(),
			agent_id("000000000002"),
			timestamp(16_000_000),
			"Lead validation record".into(),
			ProgramCorrelationId::new("70000000-0000-4000-8000-000000000001").unwrap(),
			timestamp(17_000_000),
		)
		.unwrap()
	}

	#[test]
	fn program_is_open_ended_and_retirement_is_terminal() {
		let mut program = program();

		program.transition(1, ProgramState::NeedsAttention).unwrap();
		program.transition(2, ProgramState::Blocked).unwrap();
		program.transition(3, ProgramState::Paused).unwrap();
		program.transition(4, ProgramState::Active).unwrap();
		program.transition(5, ProgramState::Retired).unwrap();

		assert_eq!(program.revision(), 6);
		assert_eq!(program.state(), ProgramState::Retired);
		assert_eq!(
			program.transition(6, ProgramState::Active),
			Err(ProgramError::InvalidLifecycle)
		);
	}

	#[test]
	fn objective_achievement_requires_exact_revision_bound_evidence() {
		let mut objective = objective();

		assert_eq!(objective.achieve(1, evidence(1)), Err(ProgramError::InvalidCompletion));

		objective.transition(1, ObjectiveState::Active).unwrap();

		assert_eq!(objective.achieve(2, evidence(1)), Err(ProgramError::InvalidCompletion));

		objective.achieve(2, evidence(2)).unwrap();

		assert_eq!(objective.state(), ObjectiveState::Achieved);
		assert_eq!(objective.revision(), 3);
		assert_eq!(objective.completion().unwrap().objective_revision(), 2);
		assert_eq!(
			objective.transition(3, ObjectiveState::Abandoned),
			Err(ProgramError::InvalidLifecycle)
		);
	}

	#[test]
	fn finite_objectives_can_be_abandoned_independently() {
		let mut first = objective();
		let mut second = Objective::new(
			ObjectiveId::new("50000000-0000-4000-8000-000000000002").unwrap(),
			project_id(),
			Some(program_id()),
			"Ship another finite baseline",
			vec!["Accepted".into()],
			vec!["Validated".into()],
			timestamp(30_000_000),
		)
		.unwrap();

		first.transition(1, ObjectiveState::Abandoned).unwrap();
		second.transition(1, ObjectiveState::Active).unwrap();

		assert_eq!(first.state(), ObjectiveState::Abandoned);
		assert_eq!(second.state(), ObjectiveState::Active);
		assert_eq!(program().state(), ProgramState::Active);
	}

	#[test]
	fn program_context_is_deterministic_data_and_preserves_inspectable_text() {
		let observation = ProgramObservationProvenance::new(
			"conversation thread mentioned only as provenance text",
			timestamp(8_000_000),
		)
		.unwrap();
		let metric =
			ProgramMetric::new("organic_clicks", "12", "count", observation.clone()).unwrap();
		let signal = ProgramSignal::new(
			ProgramObservationId::new("80000000-0000-4000-8000-000000000001").unwrap(),
			"trend",
			"Search visibility increased",
			observation.clone(),
		)
		.unwrap();
		let decision = ProgramContextDecision::new(
			ProgramObservationId::new("80000000-0000-4000-8000-000000000002").unwrap(),
			"Keep the responsibility active",
			observation,
		)
		.unwrap();
		let mut program = program();

		program
			.replace_context(
				1,
				ReviewCadence::new(7, timestamp(11_000_000)).unwrap(),
				vec![metric],
				vec![signal],
			)
			.unwrap();

		let compile = || {
			crate::compile_program_context(ProgramContextInput {
				program: &program,
				recent_decisions: vec![decision.clone()],
				quiet_period: Some(
					ProgramQuietPeriod::new(timestamp(12_000_000), timestamp(13_000_000)).unwrap(),
				),
			})
			.unwrap()
		};
		let first = compile();
		let second = compile();

		assert_eq!(first, second);
		assert!(
			String::from_utf8_lossy(first.bytes())
				.contains("conversation thread mentioned only as provenance text")
		);
		assert_eq!(first.program_revision(), 2);
	}

	#[test]
	fn criteria_observations_and_evidence_are_closed_and_bounded() {
		assert_eq!(
			Objective::new(
				ObjectiveId::new("50000000-0000-4000-8000-000000000003").unwrap(),
				project_id(),
				None,
				"Finite outcome",
				Vec::new(),
				vec!["Validated".into()],
				timestamp(30_000_000),
			),
			Err(ProgramError::InvalidCollection)
		);
		assert!(matches!(
			ProgramMetric::new(
				"NotCanonical",
				"1",
				"count",
				ProgramObservationProvenance::new("source", timestamp(1)).unwrap(),
			),
			Err(ProgramError::InvalidSymbol)
		));
		assert!(matches!(
			ObjectiveCompletionEvidence::proposed(
				ObjectiveEvidenceId::new("60000000-0000-4000-8000-000000000004").unwrap(),
				ObjectiveId::new("50000000-0000-4000-8000-000000000004").unwrap(),
				project_id(),
				1,
				"",
				agent_id("000000000001"),
				timestamp(1),
				"accepted",
				"validated",
				agent_id("000000000002"),
				timestamp(2),
				"validated",
				ProgramCorrelationId::new("70000000-0000-4000-8000-000000000004").unwrap(),
			),
			Err(ProgramError::InvalidText)
		));
	}
}
