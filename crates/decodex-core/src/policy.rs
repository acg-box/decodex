use std::{
	collections::BTreeMap,
	error::Error,
	fmt::{Debug, Display, Formatter},
	future::Future,
};

use crate::{AgentId, ProjectId};

/// Maximum UTF-8 bytes in one inert policy provenance value.
pub const MAX_POLICY_PROVENANCE_BYTES: usize = 256;
/// Maximum fields in one inert policy snapshot.
pub const MAX_POLICY_SNAPSHOT_FIELDS: usize = 32;
/// Maximum UTF-8 bytes in one policy snapshot key.
pub const MAX_POLICY_SNAPSHOT_KEY_BYTES: usize = 64;
/// Maximum UTF-8 bytes in one policy snapshot text value.
pub const MAX_POLICY_SNAPSHOT_VALUE_BYTES: usize = 256;

/// Application port for immutable Project policy authority.
pub trait PolicyRepository {
	/// Adapter-owned error.
	type Error: Error + Send + Sync + 'static;

	/// Idempotently create one Project-owned policy identity.
	fn create_policy(
		&self,
		id: PolicyId,
		project_id: ProjectId,
	) -> impl Future<Output = Result<Policy, Self::Error>> + Send;

	/// Accept one exact immutable policy revision.
	fn accept_policy_revision(
		&self,
		acceptance: PolicyRevisionAcceptance,
	) -> impl Future<Output = Result<AcceptedPolicyRevision, Self::Error>> + Send;

	/// Read one exact immutable policy revision.
	fn policy_revision(
		&self,
		id: &PolicyRevisionId,
	) -> impl Future<Output = Result<Option<AcceptedPolicyRevision>, Self::Error>> + Send;

	/// List Policy identities for one Project in stable identity order.
	fn policies_for_project(
		&self,
		project_id: &ProjectId,
	) -> impl Future<Output = Result<Vec<Policy>, Self::Error>> + Send;
}

/// Stable canonical Policy identity owned only by this module.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PolicyId(String);
impl PolicyId {
	/// Parse one canonical lowercase RFC 9562 UUID version 4 identity.
	pub fn new(value: impl Into<String>) -> Result<Self, PolicyError> {
		let value = value.into();

		if !is_canonical_uuid_v4(&value) {
			return Err(PolicyError::InvalidPolicyId);
		}

		Ok(Self(value))
	}

	/// Borrow the canonical Policy identity.
	pub fn as_str(&self) -> &str {
		&self.0
	}
}

impl Display for PolicyId {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(&self.0)
	}
}

/// Positive immutable revision number within one Policy.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PolicyRevision(u64);
impl PolicyRevision {
	/// Validate one positive policy revision.
	pub const fn new(value: u64) -> Result<Self, PolicyError> {
		if value == 0 { Err(PolicyError::InvalidRevision) } else { Ok(Self(value)) }
	}

	/// Read the positive revision number.
	pub const fn get(self) -> u64 {
		self.0
	}
}

/// Exact Project-owned Policy revision identity imported by downstream domains.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PolicyRevisionId {
	project_id: ProjectId,
	policy_id: PolicyId,
	revision: PolicyRevision,
}
impl PolicyRevisionId {
	/// Bind an exact revision to its authoritative Project and Policy identities.
	pub const fn new(project_id: ProjectId, policy_id: PolicyId, revision: PolicyRevision) -> Self {
		Self { project_id, policy_id, revision }
	}

	/// Owning Project identity.
	pub const fn project_id(&self) -> &ProjectId {
		&self.project_id
	}

	/// Owning Policy identity.
	pub const fn policy_id(&self) -> &PolicyId {
		&self.policy_id
	}

	/// Exact positive revision.
	pub const fn revision(&self) -> PolicyRevision {
		self.revision
	}
}

/// Database-authored timestamp represented as Unix epoch microseconds.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct PolicyTimestamp(i64);
impl PolicyTimestamp {
	/// Validate deterministic persistence readback.
	pub const fn from_unix_microseconds(value: i64) -> Result<Self, PolicyError> {
		if value < 0 { Err(PolicyError::InvalidChronology) } else { Ok(Self(value)) }
	}

	/// Read Unix epoch microseconds.
	pub const fn unix_microseconds(self) -> i64 {
		self.0
	}
}

/// Minimal Project-owned policy identity and current accepted revision pointer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Policy {
	id: PolicyId,
	project_id: ProjectId,
	created_at: PolicyTimestamp,
	current_revision: Option<PolicyRevision>,
}
impl Policy {
	/// Validate deterministic persistence readback.
	pub const fn from_stored(
		id: PolicyId,
		project_id: ProjectId,
		created_at: PolicyTimestamp,
		current_revision: Option<PolicyRevision>,
	) -> Self {
		Self { id, project_id, created_at, current_revision }
	}

	/// Stable Policy identity.
	pub const fn id(&self) -> &PolicyId {
		&self.id
	}

	/// Owning Project identity.
	pub const fn project_id(&self) -> &ProjectId {
		&self.project_id
	}

	/// Database-authored identity creation time.
	pub const fn created_at(&self) -> PolicyTimestamp {
		self.created_at
	}

	/// Latest accepted revision, when one exists.
	pub const fn current_revision(&self) -> Option<PolicyRevision> {
		self.current_revision
	}

	/// Minimal inert lifecycle derived from whether an accepted revision exists.
	pub const fn status(&self) -> PolicyStatus {
		if self.current_revision.is_some() {
			PolicyStatus::Accepted
		} else {
			PolicyStatus::Unaccepted
		}
	}
}

/// Minimal inert Policy lifecycle. It enables no effective resolution or behavior.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyStatus {
	/// Stable identity exists but has no accepted revision.
	Unaccepted,
	/// At least one immutable accepted revision exists.
	Accepted,
}

/// Bounded credential-negative provenance retained with an accepted revision.
#[derive(Clone, Eq, PartialEq)]
pub struct PolicyProvenance(String);
impl PolicyProvenance {
	/// Validate one ordinary opaque provenance value.
	pub fn new(value: impl Into<String>) -> Result<Self, PolicyError> {
		let value = value.into();

		if value.is_empty()
			|| value.len() > MAX_POLICY_PROVENANCE_BYTES
			|| value.chars().any(char::is_control)
		{
			return Err(PolicyError::InvalidProvenance);
		}
		if crate::contains_credential_material(&value) {
			return Err(PolicyError::CredentialRejected);
		}

		Ok(Self(value))
	}

	/// Borrow the opaque provenance value.
	pub fn as_str(&self) -> &str {
		&self.0
	}
}

impl Debug for PolicyProvenance {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str("PolicyProvenance(<redacted>)")
	}
}

/// Closed scalar in an inert policy snapshot. Values have no effective semantics here.
#[derive(Clone, Eq, PartialEq)]
pub enum PolicySnapshotValue {
	/// Bounded ordinary text.
	Text(String),
	/// Non-secret boolean fact.
	Boolean(bool),
}
impl Debug for PolicySnapshotValue {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Text(_) => formatter.write_str("Text(<redacted>)"),
			Self::Boolean(value) => formatter.debug_tuple("Boolean").field(value).finish(),
		}
	}
}

/// Bounded deterministic inert snapshot with no resolver or effects.
#[derive(Clone, Default, Eq, PartialEq)]
pub struct PolicySnapshot(BTreeMap<String, PolicySnapshotValue>);
impl PolicySnapshot {
	/// Validate one closed credential-negative snapshot.
	pub fn new(values: BTreeMap<String, PolicySnapshotValue>) -> Result<Self, PolicyError> {
		if values.len() > MAX_POLICY_SNAPSHOT_FIELDS {
			return Err(PolicyError::InvalidSnapshot);
		}

		for (key, value) in &values {
			if key.is_empty()
				|| key.len() > MAX_POLICY_SNAPSHOT_KEY_BYTES
				|| !key.bytes().enumerate().all(|(index, byte)| {
					if index == 0 {
						byte.is_ascii_lowercase()
					} else {
						byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'
					}
				}) {
				return Err(PolicyError::InvalidSnapshot);
			}
			if crate::is_credential_metadata_key(key) {
				return Err(PolicyError::CredentialRejected);
			}

			if let PolicySnapshotValue::Text(text) = value {
				if text.len() > MAX_POLICY_SNAPSHOT_VALUE_BYTES
					|| text.chars().any(char::is_control)
				{
					return Err(PolicyError::InvalidSnapshot);
				}
				if crate::contains_credential_material(text) {
					return Err(PolicyError::CredentialRejected);
				}
			}
		}

		Ok(Self(values))
	}

	/// Empty inert snapshot.
	pub const fn empty() -> Self {
		Self(BTreeMap::new())
	}

	/// Borrow fields in canonical key order.
	pub const fn as_map(&self) -> &BTreeMap<String, PolicySnapshotValue> {
		&self.0
	}
}

impl Debug for PolicySnapshot {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("PolicySnapshot")
			.field("field_count", &self.0.len())
			.finish_non_exhaustive()
	}
}

/// Proposed immutable bytes for one acceptance transaction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PolicyRevisionAcceptance {
	/// Exact Project-owned revision identity.
	pub id: PolicyRevisionId,
	/// Opaque bounded provenance.
	pub provenance: PolicyProvenance,
	/// Inert bounded snapshot.
	pub snapshot: PolicySnapshot,
	/// Stable Agent identity whose active same-Project Lead authority is checked by storage.
	pub accepted_by: AgentId,
	/// Exact prior revision replaced by this acceptance.
	pub supersedes: Option<PolicyRevisionId>,
}
impl PolicyRevisionAcceptance {
	/// Validate typed first-revision or exact immediate-predecessor lineage.
	pub fn validate(&self) -> Result<(), PolicyError> {
		match (&self.supersedes, self.id.revision().get()) {
			(None, 1) => Ok(()),
			(Some(previous), revision)
				if previous.project_id() == self.id.project_id()
					&& previous.policy_id() == self.id.policy_id()
					&& previous.revision().get().checked_add(1) == Some(revision) =>
				Ok(()),
			_ => Err(PolicyError::InvalidSupersession),
		}
	}
}

/// Exact immutable accepted policy revision readback.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AcceptedPolicyRevision {
	id: PolicyRevisionId,
	provenance: PolicyProvenance,
	snapshot: PolicySnapshot,
	accepted_by: AgentId,
	policy_created_at: PolicyTimestamp,
	accepted_at: PolicyTimestamp,
	supersedes: Option<PolicyRevisionId>,
}
impl AcceptedPolicyRevision {
	/// Validate deterministic persistence readback and chronology.
	pub fn from_stored(
		acceptance: PolicyRevisionAcceptance,
		policy_created_at: PolicyTimestamp,
		accepted_at: PolicyTimestamp,
	) -> Result<Self, PolicyError> {
		acceptance.validate()?;

		if accepted_at < policy_created_at {
			return Err(PolicyError::InvalidChronology);
		}

		Ok(Self {
			id: acceptance.id,
			provenance: acceptance.provenance,
			snapshot: acceptance.snapshot,
			accepted_by: acceptance.accepted_by,
			policy_created_at,
			accepted_at,
			supersedes: acceptance.supersedes,
		})
	}

	/// Exact Project-owned revision identity.
	pub const fn id(&self) -> &PolicyRevisionId {
		&self.id
	}

	/// Opaque bounded provenance.
	pub const fn provenance(&self) -> &PolicyProvenance {
		&self.provenance
	}

	/// Inert immutable snapshot.
	pub const fn snapshot(&self) -> &PolicySnapshot {
		&self.snapshot
	}

	/// Accepting stable Agent identity.
	pub const fn accepted_by(&self) -> &AgentId {
		&self.accepted_by
	}

	/// Database-authored Policy identity creation time.
	pub const fn policy_created_at(&self) -> PolicyTimestamp {
		self.policy_created_at
	}

	/// Database-authored acceptance time.
	pub const fn accepted_at(&self) -> PolicyTimestamp {
		self.accepted_at
	}

	/// Typed exact supersession lineage.
	pub const fn supersedes(&self) -> Option<&PolicyRevisionId> {
		self.supersedes.as_ref()
	}
}

/// Closed Policy-domain validation failure without caller-controlled text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyError {
	/// Policy identity was not one canonical UUID version 4.
	InvalidPolicyId,
	/// Revision was not positive.
	InvalidRevision,
	/// Provenance was empty, unbounded, or contained control text.
	InvalidProvenance,
	/// Snapshot exceeded its closed field, key, or value bounds.
	InvalidSnapshot,
	/// Provenance or snapshot contained credential-shaped material.
	CredentialRejected,
	/// Supersession did not name the exact same-Project immediate predecessor.
	InvalidSupersession,
	/// Stored database-authored timestamps were malformed or reversed.
	InvalidChronology,
}
impl Error for PolicyError {}

impl Display for PolicyError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(match self {
			Self::InvalidPolicyId => "invalid Policy identity",
			Self::InvalidRevision => "invalid Policy revision",
			Self::InvalidProvenance => "invalid Policy provenance",
			Self::InvalidSnapshot => "invalid Policy snapshot",
			Self::CredentialRejected => "credential-bearing Policy data rejected",
			Self::InvalidSupersession => "invalid Policy supersession lineage",
			Self::InvalidChronology => "invalid Policy chronology",
		})
	}
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
	use std::collections::BTreeMap;

	use crate::{
		AcceptedPolicyRevision, AgentId, PolicyError, PolicyId, PolicyProvenance, PolicyRevision,
		PolicyRevisionAcceptance, PolicyRevisionId, PolicySnapshot, PolicySnapshotValue,
		PolicyTimestamp, ProjectId,
	};

	fn revision(number: u64) -> PolicyRevisionId {
		PolicyRevisionId::new(
			ProjectId::new("10000000-0000-4000-8000-000000000001").unwrap(),
			PolicyId::new("30000000-0000-4000-8000-000000000001").unwrap(),
			PolicyRevision::new(number).unwrap(),
		)
	}

	fn acceptance(number: u64) -> PolicyRevisionAcceptance {
		PolicyRevisionAcceptance {
			id: revision(number),
			provenance: PolicyProvenance::new("user-accepted baseline").unwrap(),
			snapshot: PolicySnapshot::new(BTreeMap::from([
				("note".into(), PolicySnapshotValue::Text("inert only".into())),
				("reviewed".into(), PolicySnapshotValue::Boolean(true)),
			]))
			.unwrap(),
			accepted_by: AgentId::new("20000000-0000-4000-8000-000000000001").unwrap(),
			supersedes: (number > 1).then(|| revision(number - 1)),
		}
	}

	#[test]
	fn policy_ids_and_revisions_are_canonical_and_positive() {
		for value in [
			"",
			"30000000-0000-4000-8000-00000000000A",
			"30000000-0000-5000-8000-000000000001",
			"not-a-policy-id",
		] {
			assert_eq!(PolicyId::new(value), Err(PolicyError::InvalidPolicyId));
		}

		assert_eq!(PolicyRevision::new(0), Err(PolicyError::InvalidRevision));
	}

	#[test]
	fn policy_payloads_are_bounded_credential_negative_and_debug_redacted() {
		assert_eq!(
			PolicyProvenance::new("Bearer abcdefghijklmnop"),
			Err(PolicyError::CredentialRejected)
		);
		assert_eq!(
			PolicySnapshot::new(BTreeMap::from([(
				"refresh_token".into(),
				PolicySnapshotValue::Text("ordinary".into()),
			)])),
			Err(PolicyError::CredentialRejected)
		);

		let provenance = PolicyProvenance::new("private marker").unwrap();
		let snapshot = PolicySnapshot::new(BTreeMap::from([(
			"note".into(),
			PolicySnapshotValue::Text("private marker".into()),
		)]))
		.unwrap();

		assert!(!format!("{provenance:?}").contains("private marker"));
		assert!(!format!("{snapshot:?}").contains("private marker"));
	}

	#[test]
	fn supersession_is_exact_same_project_immediate_lineage() {
		assert!(acceptance(1).validate().is_ok());
		assert!(acceptance(2).validate().is_ok());

		let mut malformed = acceptance(2);

		malformed.supersedes = Some(revision(2));

		assert_eq!(malformed.validate(), Err(PolicyError::InvalidSupersession));

		let mut malformed = acceptance(1);

		malformed.supersedes = Some(revision(1));

		assert_eq!(malformed.validate(), Err(PolicyError::InvalidSupersession));
	}

	#[test]
	fn immutable_readback_rejects_malformed_chronology() {
		let accepted = AcceptedPolicyRevision::from_stored(
			acceptance(1),
			PolicyTimestamp::from_unix_microseconds(10).unwrap(),
			PolicyTimestamp::from_unix_microseconds(11).unwrap(),
		)
		.unwrap();

		assert_eq!(accepted.id(), &revision(1));
		assert_eq!(accepted.supersedes(), None);
		assert_eq!(accepted.accepted_at().unix_microseconds(), 11);
		assert_eq!(
			AcceptedPolicyRevision::from_stored(
				acceptance(1),
				PolicyTimestamp::from_unix_microseconds(11).unwrap(),
				PolicyTimestamp::from_unix_microseconds(10).unwrap(),
			),
			Err(PolicyError::InvalidChronology)
		);
	}
}
