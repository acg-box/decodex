//! Inert Automation definitions and immutable firing proposals.
//!
//! These values do not parse schedules, observe signals, schedule work, deliver messages, or
//! grant Agent authority.

use std::{
	error::Error,
	fmt::{Display, Formatter},
};

use crate::{
	AgentId, BlobHash, PolicyRevisionId, ProgramId, ProjectId, RepositoryIdentity, WorkItemId,
};

macro_rules! stable_id {
	($name:ident, $error:ident, $label:literal) => {
		#[doc = concat!("Stable canonical ", $label, " identity.")]
		#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
		pub struct $name(String);
		impl $name {
			#[doc = concat!("Parse one canonical lowercase RFC 9562 UUID-v4 ", $label, " identity.")]
			pub fn new(value: impl AsRef<str>) -> Result<Self, AutomationError> {
				let value = value.as_ref();

				if !is_canonical_uuid_v4(value) {
					return Err(AutomationError::$error);
				}

				Ok(Self(value.to_owned()))
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
	};
}

stable_id!(AutomationId, InvalidAutomationId, "Automation");
stable_id!(AutomationFiringId, InvalidFiringId, "Automation firing");
stable_id!(AutomationOccurrenceId, InvalidOccurrenceId, "Automation occurrence");

/// Maximum UTF-8 bytes in one inert RRULE.
pub const MAX_AUTOMATION_RRULE_BYTES: usize = 4_096;
/// Maximum UTF-8 bytes in one inert timezone source value.
pub const MAX_AUTOMATION_TIMEZONE_BYTES: usize = 256;
/// Maximum UTF-8 bytes in one credential-negative source symbol.
pub const MAX_AUTOMATION_SYMBOL_BYTES: usize = 64;
/// Latest finite timestamp representable by PostgreSQL and RFC 3339, in Unix microseconds.
pub const MAX_AUTOMATION_TIMESTAMP_MICROSECONDS: i64 = 253_402_300_799_999_999;

/// Closed Automation-domain validation failure without caller-controlled text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AutomationError {
	/// Automation identity was not canonical UUID-v4 text.
	InvalidAutomationId,
	/// Firing identity was not canonical UUID-v4 text.
	InvalidFiringId,
	/// Occurrence identity was not canonical UUID-v4 text.
	InvalidOccurrenceId,
	/// A persisted or incremented revision was outside its positive domain.
	InvalidRevision,
	/// A timestamp was outside the finite domain or chronology was invalid.
	InvalidChronology,
	/// A schedule source was empty, oversized, or contained controls.
	InvalidSchedule,
	/// A source or event symbol was not canonical lowercase snake case.
	InvalidSymbol,
	/// Ordinary source data contained concrete credential material.
	CredentialRejected,
	/// Requested lifecycle transition was not legal.
	InvalidLifecycle,
	/// Expected revision did not match the current Automation definition.
	RevisionConflict,
	/// A trigger or target declared another Project.
	CrossProjectReference,
	/// A paused or retired definition cannot propose a firing.
	FiringNotEnabled,
	/// A firing source did not match the definition trigger.
	SourceTriggerMismatch,
	/// An observed source was used with a schedule trigger or a schedule source with a signal.
	InvalidSourceKind,
}
impl Error for AutomationError {}

impl Display for AutomationError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(match self {
			Self::InvalidAutomationId => "invalid Automation identity",
			Self::InvalidFiringId => "invalid Automation firing identity",
			Self::InvalidOccurrenceId => "invalid Automation occurrence identity",
			Self::InvalidRevision => "invalid Automation revision",
			Self::InvalidChronology => "invalid Automation chronology",
			Self::InvalidSchedule => "invalid inert Automation schedule",
			Self::InvalidSymbol => "invalid canonical Automation symbol",
			Self::CredentialRejected => "credential-bearing Automation source rejected",
			Self::InvalidLifecycle => "invalid Automation lifecycle transition",
			Self::RevisionConflict => "Automation revision conflict",
			Self::CrossProjectReference => "cross-Project Automation reference rejected",
			Self::FiringNotEnabled => "Automation firing requires an enabled definition",
			Self::SourceTriggerMismatch => "Automation source does not match its trigger",
			Self::InvalidSourceKind => "invalid Automation firing source kind",
		})
	}
}

/// Database-compatible finite time represented as Unix epoch microseconds.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AutomationTimestamp(i64);
impl AutomationTimestamp {
	/// Validate one finite non-negative timestamp.
	pub const fn from_unix_microseconds(value: i64) -> Result<Self, AutomationError> {
		if value < 0 || value > MAX_AUTOMATION_TIMESTAMP_MICROSECONDS {
			Err(AutomationError::InvalidChronology)
		} else {
			Ok(Self(value))
		}
	}

	/// Read Unix epoch microseconds without rounding or truncation.
	pub const fn unix_microseconds(self) -> i64 {
		self.0
	}
}

/// Positive immutable revision number within one Automation.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AutomationRevision(u64);
impl AutomationRevision {
	/// Validate one positive Automation revision.
	pub const fn new(value: u64) -> Result<Self, AutomationError> {
		if value == 0 { Err(AutomationError::InvalidRevision) } else { Ok(Self(value)) }
	}

	/// Read the positive revision number.
	pub const fn get(self) -> u64 {
		self.0
	}
}

/// Inert Automation lifecycle.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum AutomationState {
	/// The definition may propose a firing.
	Enabled,
	/// The definition is retained but cannot propose a firing.
	Paused,
	/// The definition is permanently closed and retained for readback.
	Retired,
}
impl AutomationState {
	/// Canonical persistence spelling.
	pub const fn as_str(self) -> &'static str {
		match self {
			Self::Enabled => "enabled",
			Self::Paused => "paused",
			Self::Retired => "retired",
		}
	}

	/// Whether this state can transition to `next`.
	pub const fn can_transition_to(self, next: Self) -> bool {
		matches!(
			(self, next),
			(Self::Enabled, Self::Paused | Self::Retired)
				| (Self::Paused, Self::Enabled | Self::Retired)
		)
	}
}

/// Bounded inert schedule text. This type does not parse or execute either value.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AutomationSchedule {
	rrule: String,
	timezone: String,
}
impl AutomationSchedule {
	/// Retain bounded credential-negative RRULE and timezone source text.
	pub fn new(rrule: impl AsRef<str>, timezone: impl AsRef<str>) -> Result<Self, AutomationError> {
		let rrule = rrule.as_ref();
		let timezone = timezone.as_ref();

		validate_source_text(rrule, MAX_AUTOMATION_RRULE_BYTES)?;
		validate_source_text(timezone, MAX_AUTOMATION_TIMEZONE_BYTES)?;

		Ok(Self { rrule: rrule.to_owned(), timezone: timezone.to_owned() })
	}

	/// Inert RRULE source text.
	pub fn rrule(&self) -> &str {
		&self.rrule
	}

	/// Inert timezone source text.
	pub fn timezone(&self) -> &str {
		&self.timezone
	}
}

/// Bounded canonical symbol used by external Automation sources.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AutomationSymbol(String);
impl AutomationSymbol {
	/// Parse one lowercase snake-case credential-negative symbol.
	pub fn new(value: impl AsRef<str>) -> Result<Self, AutomationError> {
		let value = value.as_ref();

		if value.is_empty()
			|| value.len() > MAX_AUTOMATION_SYMBOL_BYTES
			|| !value.bytes().enumerate().all(|(index, byte)| {
				if index == 0 {
					byte.is_ascii_lowercase()
				} else {
					byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'
				}
			}) {
			return Err(AutomationError::InvalidSymbol);
		}
		if crate::contains_credential_material(value) {
			return Err(AutomationError::CredentialRejected);
		}

		Ok(Self(value.to_owned()))
	}

	/// Borrow the canonical symbol.
	pub fn as_str(&self) -> &str {
		&self.0
	}
}
impl Display for AutomationSymbol {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(&self.0)
	}
}

/// Canonical repository identity accepted as credential-negative Automation source data.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AutomationRepositorySource(RepositoryIdentity);
impl AutomationRepositorySource {
	/// Reject a canonical repository identity that still has a concrete credential shape.
	pub fn new(identity: RepositoryIdentity) -> Result<Self, AutomationError> {
		if crate::contains_credential_material(identity.as_str()) {
			return Err(AutomationError::CredentialRejected);
		}

		Ok(Self(identity))
	}

	/// Borrow the validated canonical repository identity.
	pub const fn identity(&self) -> &RepositoryIdentity {
		&self.0
	}
}

/// Closed inert Automation trigger.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum AutomationTrigger {
	/// Bounded RRULE and timezone source text.
	Schedule(AutomationSchedule),
	/// Canonical external webhook source and event symbols.
	Webhook {
		/// Stable source namespace.
		source: AutomationSymbol,
		/// Stable event name within the source.
		event: AutomationSymbol,
	},
	/// One canonical repository identity and event symbol.
	RepositoryEvent {
		/// Credential-negative repository source; storage resolves its Project binding.
		repository: AutomationRepositorySource,
		/// Stable repository event name.
		event: AutomationSymbol,
	},
	/// One Project-scoped Program metric observation.
	MetricObservation {
		/// Declared owning Project.
		project_id: ProjectId,
		/// Declared same-Project Program.
		program_id: ProgramId,
		/// Stable Program metric key.
		metric: AutomationSymbol,
	},
}
impl AutomationTrigger {
	/// Declared Project scope carried by a metric trigger.
	pub const fn project_id(&self) -> Option<&ProjectId> {
		match self {
			Self::MetricObservation { project_id, .. } => Some(project_id),
			Self::Schedule(_) | Self::Webhook { .. } | Self::RepositoryEvent { .. } => None,
		}
	}

	const fn is_schedule(&self) -> bool {
		matches!(self, Self::Schedule(_))
	}
}

/// Closed Automation delivery target.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum AutomationTarget {
	/// One same-Project Program inbox.
	Program {
		/// Declared owning Project.
		project_id: ProjectId,
		/// Stable Program identity.
		program_id: ProgramId,
	},
	/// One same-Project WorkItem inbox.
	WorkItem {
		/// Declared owning Project.
		project_id: ProjectId,
		/// Stable WorkItem identity.
		work_item_id: WorkItemId,
	},
	/// The one global Advisor inbox.
	Advisor {
		/// Declared stable Advisor identity; storage resolves its role and existence.
		agent_id: AgentId,
	},
	/// One Project's canonical Lead inbox.
	Lead {
		/// Declared owning Project.
		project_id: ProjectId,
		/// Declared stable Lead identity; storage resolves its role and existence.
		agent_id: AgentId,
	},
}
impl AutomationTarget {
	/// Declared Project scope, absent only for the global Advisor.
	pub const fn project_id(&self) -> Option<&ProjectId> {
		match self {
			Self::Program { project_id, .. }
			| Self::WorkItem { project_id, .. }
			| Self::Lead { project_id, .. } => Some(project_id),
			Self::Advisor { .. } => None,
		}
	}
}

/// One validated revision of an inert Project-owned Automation definition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AutomationDefinition {
	id: AutomationId,
	project_id: ProjectId,
	owner_agent_id: AgentId,
	policy_revision_id: PolicyRevisionId,
	payload_schema: BlobHash,
	revision: AutomationRevision,
	trigger: AutomationTrigger,
	target: AutomationTarget,
	state: AutomationState,
}
impl AutomationDefinition {
	/// Create revision one of an enabled inert Automation definition.
	pub fn new(
		id: AutomationId,
		project_id: ProjectId,
		owner_agent_id: AgentId,
		policy_revision_id: PolicyRevisionId,
		payload_schema: BlobHash,
		trigger: AutomationTrigger,
		target: AutomationTarget,
	) -> Result<Self, AutomationError> {
		Self::from_stored(
			id,
			project_id,
			owner_agent_id,
			policy_revision_id,
			payload_schema,
			AutomationRevision::new(1)?,
			trigger,
			target,
			AutomationState::Enabled,
		)
	}

	/// Validate deterministic persistence readback.
	///
	/// Agent roles and the existence of referenced Agents, Programs, WorkItems, repositories,
	/// policy revisions, and blobs remain storage obligations. This constructor validates only
	/// carried identity and same-Project structure.
	#[allow(clippy::too_many_arguments)]
	pub fn from_stored(
		id: AutomationId,
		project_id: ProjectId,
		owner_agent_id: AgentId,
		policy_revision_id: PolicyRevisionId,
		payload_schema: BlobHash,
		revision: AutomationRevision,
		trigger: AutomationTrigger,
		target: AutomationTarget,
		state: AutomationState,
	) -> Result<Self, AutomationError> {
		if policy_revision_id.project_id() != &project_id
			|| trigger.project_id().is_some_and(|value| value != &project_id)
			|| target.project_id().is_some_and(|value| value != &project_id)
		{
			return Err(AutomationError::CrossProjectReference);
		}

		Ok(Self {
			id,
			project_id,
			owner_agent_id,
			policy_revision_id,
			payload_schema,
			revision,
			trigger,
			target,
			state,
		})
	}

	/// Apply one legal expected-revision lifecycle transition.
	pub fn transition(
		&mut self,
		expected_revision: AutomationRevision,
		state: AutomationState,
	) -> Result<(), AutomationError> {
		if expected_revision != self.revision {
			return Err(AutomationError::RevisionConflict);
		}
		if !self.state.can_transition_to(state) {
			return Err(AutomationError::InvalidLifecycle);
		}

		let revision =
			self.revision.get().checked_add(1).ok_or(AutomationError::InvalidRevision)?;

		self.revision = AutomationRevision::new(revision)?;
		self.state = state;

		Ok(())
	}

	/// Stable Automation identity.
	pub const fn id(&self) -> &AutomationId {
		&self.id
	}

	/// Owning Project identity.
	pub const fn project_id(&self) -> &ProjectId {
		&self.project_id
	}

	/// Declared owner Agent identity; storage resolves role and existence.
	pub const fn owner_agent_id(&self) -> &AgentId {
		&self.owner_agent_id
	}

	/// Exact accepted Project Policy revision.
	pub const fn policy_revision_id(&self) -> &PolicyRevisionId {
		&self.policy_revision_id
	}

	/// Content-addressed payload schema.
	pub const fn payload_schema(&self) -> BlobHash {
		self.payload_schema
	}

	/// Positive immutable definition revision.
	pub const fn revision(&self) -> AutomationRevision {
		self.revision
	}

	/// Closed inert trigger.
	pub const fn trigger(&self) -> &AutomationTrigger {
		&self.trigger
	}

	/// Closed delivery target.
	pub const fn target(&self) -> &AutomationTarget {
		&self.target
	}

	/// Current inert lifecycle.
	pub const fn state(&self) -> AutomationState {
		self.state
	}
}

/// Exact source of one inert firing proposal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AutomationFiringSource {
	/// One schedule occurrence from exact retained schedule text.
	Schedule {
		/// Schedule text that must equal the definition trigger.
		schedule: AutomationSchedule,
	},
	/// One positively observed non-schedule signal.
	ObservedSignal {
		/// Exact non-schedule trigger value that was observed.
		trigger: AutomationTrigger,
		/// Finite time when the signal was observed.
		observed_at: AutomationTimestamp,
	},
	/// One explicit manual run-now request, without pretending it was triggered externally.
	ManualRunNow,
}

/// Stable occurrence-level dedupe identity.
///
/// Definition revision and definition contents are deliberately absent. The same Automation
/// occurrence therefore has one equal key before and after a definition revision. Persistence
/// must enforce uniqueness of this complete key.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AutomationDedupeKey {
	automation_id: AutomationId,
	occurrence_id: AutomationOccurrenceId,
}
impl AutomationDedupeKey {
	fn new(automation_id: AutomationId, occurrence_id: AutomationOccurrenceId) -> Self {
		Self { automation_id, occurrence_id }
	}

	/// Stable Automation identity.
	pub const fn automation_id(&self) -> &AutomationId {
		&self.automation_id
	}

	/// Stable occurrence identity shared across definition revisions.
	pub const fn occurrence_id(&self) -> &AutomationOccurrenceId {
		&self.occurrence_id
	}
}

/// Immutable inert proposal for one exact Automation occurrence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AutomationFiring {
	id: AutomationFiringId,
	definition: AutomationDefinition,
	source: AutomationFiringSource,
	due_at: AutomationTimestamp,
	dedupe_key: AutomationDedupeKey,
}
impl AutomationFiring {
	/// Stable firing identity.
	pub const fn id(&self) -> &AutomationFiringId {
		&self.id
	}

	/// Exact immutable definition revision and authority snapshot.
	pub const fn definition(&self) -> &AutomationDefinition {
		&self.definition
	}

	/// Exact schedule, observed signal, or explicit manual source.
	pub const fn source(&self) -> &AutomationFiringSource {
		&self.source
	}

	/// Finite due time for this occurrence.
	pub const fn due_at(&self) -> AutomationTimestamp {
		self.due_at
	}

	/// Occurrence-level key that remains equal across definition revisions.
	pub const fn dedupe_key(&self) -> &AutomationDedupeKey {
		&self.dedupe_key
	}

	/// Stable occurrence identity.
	pub const fn occurrence_id(&self) -> &AutomationOccurrenceId {
		self.dedupe_key.occurrence_id()
	}
}

/// Propose one inert firing without scheduling, delivery, persistence, or side effects.
pub fn propose_automation_firing(
	definition: &AutomationDefinition,
	id: AutomationFiringId,
	occurrence_id: AutomationOccurrenceId,
	source: AutomationFiringSource,
	due_at: AutomationTimestamp,
) -> Result<AutomationFiring, AutomationError> {
	if definition.state != AutomationState::Enabled {
		return Err(AutomationError::FiringNotEnabled);
	}

	match (&definition.trigger, &source) {
		(
			AutomationTrigger::Schedule(trigger_schedule),
			AutomationFiringSource::Schedule { schedule },
		) if trigger_schedule == schedule => {},
		(AutomationTrigger::Schedule(_), AutomationFiringSource::Schedule { .. }) => {
			return Err(AutomationError::SourceTriggerMismatch);
		},
		(AutomationTrigger::Schedule(_), AutomationFiringSource::ObservedSignal { .. })
		| (
			AutomationTrigger::Webhook { .. }
			| AutomationTrigger::RepositoryEvent { .. }
			| AutomationTrigger::MetricObservation { .. },
			AutomationFiringSource::Schedule { .. },
		) => return Err(AutomationError::InvalidSourceKind),
		(
			AutomationTrigger::Webhook { .. }
			| AutomationTrigger::RepositoryEvent { .. }
			| AutomationTrigger::MetricObservation { .. },
			AutomationFiringSource::ObservedSignal { trigger: source_trigger, observed_at },
		) => {
			if source_trigger.is_schedule() {
				return Err(AutomationError::InvalidSourceKind);
			}
			if source_trigger != &definition.trigger {
				return Err(AutomationError::SourceTriggerMismatch);
			}
			if due_at < *observed_at {
				return Err(AutomationError::InvalidChronology);
			}
		},
		(_, AutomationFiringSource::ManualRunNow) => {},
	}

	let dedupe_key = AutomationDedupeKey::new(definition.id.clone(), occurrence_id);

	Ok(AutomationFiring { id, definition: definition.clone(), source, due_at, dedupe_key })
}

fn validate_source_text(value: &str, maximum: usize) -> Result<(), AutomationError> {
	if value.is_empty() || value.len() > maximum || value.chars().any(char::is_control) {
		return Err(AutomationError::InvalidSchedule);
	}
	if crate::contains_credential_material(value) {
		return Err(AutomationError::CredentialRejected);
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
