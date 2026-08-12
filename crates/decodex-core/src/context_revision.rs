//! Immutable, owner-safe, provenance-linked long-term context revisions.
//!
//! # Non-authority boundary
//!
//! This module constructs bounded structural values and pure transition proposals. These values
//! do not prove persistence, source resolution, owner selection, currentness, acceptance, or the
//! uniqueness of an unscoped Advisor. A storage or application transaction must prove those facts,
//! compare the expected revision, and decide whether to accept and persist the proposal.

use std::{
	cmp::Ordering,
	error::Error,
	fmt::{Debug, Display, Formatter},
};

use crate::{
	Agent, AgentId, AgentRole, BlobHash, PolicyRevisionId, Program, ProgramId, Project, ProjectId,
	RepositoryContentRevision, RepositoryIdentity,
};

/// Maximum items in one immutable ContextRevision.
pub const MAX_CONTEXT_REVISION_ITEMS: usize = 256;
/// Maximum UTF-8 bytes in one ContextRevision item.
pub const MAX_CONTEXT_REVISION_ITEM_BYTES: usize = 4_096;
/// Maximum bytes in one canonical ContextRevision encoding.
pub const MAX_CONTEXT_REVISION_BYTES: usize = 128 * 1_024;

const CONTEXT_REVISION_MAGIC: &[u8] = b"decodex/context-revision/1\0";
const UUID_BYTES: usize = 36;
const DIGEST_HEX_BYTES: usize = 64;

macro_rules! stable_id {
	($name:ident, $label:literal) => {
		#[doc = concat!("Stable canonical ", $label, " identity.")]
		#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
		pub struct $name(String);

		impl $name {
			#[doc = concat!("Parse one canonical lowercase RFC 9562 UUID-v4 ", $label, " identity.")]
			pub fn new(value: impl Into<String>) -> Result<Self, ContextRevisionError> {
				let value = value.into();

				if !is_canonical_uuid_v4(&value) {
					return Err(ContextRevisionError::InvalidIdentity($label));
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
	};
}

stable_id!(ContextRevisionId, "ContextRevision");
stable_id!(ContextRevisionItemId, "ContextRevision item");

/// Positive immutable revision number within one ContextRevision identity.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ContextRevisionNumber(u64);
impl ContextRevisionNumber {
	/// Validate one positive revision number.
	pub const fn new(value: u64) -> Result<Self, ContextRevisionError> {
		if value == 0 { Err(ContextRevisionError::InvalidRevision) } else { Ok(Self(value)) }
	}

	/// Read the positive revision number.
	pub const fn get(self) -> u64 {
		self.0
	}

	fn next(self) -> Result<Self, ContextRevisionError> {
		self.0.checked_add(1).map(Self).ok_or(ContextRevisionError::InvalidRevision)
	}
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum ContextRevisionOwnerValue {
	Project(ProjectId),
	Advisor(AgentId),
	Program { program_id: ProgramId, project_id: ProjectId },
}

/// Closed structural owner tag copied from supplied domain values.
///
/// This tag does not prove that the owner exists, is current, or is selected by storage authority.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ContextRevisionOwner(ContextRevisionOwnerValue);
impl ContextRevisionOwner {
	/// Tag one supplied Project identity.
	pub fn project(project: &Project) -> Self {
		Self(ContextRevisionOwnerValue::Project(project.id().clone()))
	}

	/// Tag an unscoped Advisor-shaped Agent.
	///
	/// This structural check does not prove global selection or uniqueness.
	pub fn advisor(advisor: &Agent) -> Result<Self, ContextRevisionError> {
		if advisor.role() != AgentRole::Advisor || advisor.project_id().is_some() {
			return Err(ContextRevisionError::InvalidOwner);
		}

		Ok(Self(ContextRevisionOwnerValue::Advisor(advisor.id().clone())))
	}

	/// Tag one supplied Program together with its embedded Project relation.
	pub fn program(program: &Program) -> Self {
		Self(ContextRevisionOwnerValue::Program {
			program_id: program.id().clone(),
			project_id: program.project_id().clone(),
		})
	}

	/// Owning Project for Project and Program context.
	pub const fn project_id(&self) -> Option<&ProjectId> {
		match &self.0 {
			ContextRevisionOwnerValue::Project(id) => Some(id),
			ContextRevisionOwnerValue::Program { project_id, .. } => Some(project_id),
			ContextRevisionOwnerValue::Advisor(_) => None,
		}
	}

	/// Program identity only for Program context.
	pub const fn program_id(&self) -> Option<&ProgramId> {
		match &self.0 {
			ContextRevisionOwnerValue::Program { program_id, .. } => Some(program_id),
			ContextRevisionOwnerValue::Project(_) | ContextRevisionOwnerValue::Advisor(_) => None,
		}
	}

	/// Agent identity only for an unscoped Advisor tag.
	pub const fn advisor_id(&self) -> Option<&AgentId> {
		match &self.0 {
			ContextRevisionOwnerValue::Advisor(id) => Some(id),
			ContextRevisionOwnerValue::Project(_) | ContextRevisionOwnerValue::Program { .. } =>
				None,
		}
	}
}

/// Structurally complete immutable ContextRevision lineage reference.
///
/// The value identifies bytes by digest but does not prove that those bytes exist or are accepted.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ContextRevisionReference {
	id: ContextRevisionId,
	owner: ContextRevisionOwner,
	revision: ContextRevisionNumber,
	digest: BlobHash,
}
impl ContextRevisionReference {
	/// Construct one fully typed reference from supplied fields.
	pub const fn new(
		id: ContextRevisionId,
		owner: ContextRevisionOwner,
		revision: ContextRevisionNumber,
		digest: BlobHash,
	) -> Self {
		Self { id, owner, revision, digest }
	}

	/// Stable ContextRevision identity.
	pub const fn id(&self) -> &ContextRevisionId {
		&self.id
	}

	/// Closed owner scope.
	pub const fn owner(&self) -> &ContextRevisionOwner {
		&self.owner
	}

	/// Supplied positive revision.
	pub const fn revision(&self) -> ContextRevisionNumber {
		self.revision
	}

	/// Supplied digest identifying the referenced canonical bytes.
	pub const fn digest(&self) -> BlobHash {
		self.digest
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ContextRevisionSourceValue {
	Project {
		project_id: ProjectId,
		revision: u64,
	},
	Program {
		program_id: ProgramId,
		project_id: ProjectId,
		revision: u64,
	},
	Repository {
		project_id: ProjectId,
		repository_id: RepositoryIdentity,
		revision: RepositoryContentRevision,
	},
	Policy(PolicyRevisionId),
	ContextRevision(ContextRevisionReference),
}

/// Typed provenance locator copied from supplied domain values.
///
/// Construction does not prove that the source exists, is current, or resolves successfully.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextRevisionSource(ContextRevisionSourceValue);
impl ContextRevisionSource {
	/// Record the revision carried by one supplied Project value.
	pub fn project(project: &Project) -> Self {
		Self::project_from_stored(project.id().clone(), project.revision())
			.expect("Project revisions are validated as positive")
	}

	/// Reconstruct one Project source from canonical typed stored fields.
	///
	/// This validates structural shape only and does not resolve or select a Project.
	pub fn project_from_stored(
		project_id: ProjectId,
		revision: u64,
	) -> Result<Self, ContextRevisionError> {
		let revision = ContextRevisionNumber::new(revision)?.get();

		Ok(Self(ContextRevisionSourceValue::Project { project_id, revision }))
	}

	/// Record the revision and Project relation carried by one supplied Program value.
	pub fn program(program: &Program) -> Self {
		Self::program_from_stored(
			program.id().clone(),
			program.project_id().clone(),
			program.revision(),
		)
		.expect("Program revisions are validated as positive")
	}

	/// Reconstruct one Program source from canonical typed stored fields.
	///
	/// This validates structural shape only and does not resolve or select a Program or Project.
	pub fn program_from_stored(
		program_id: ProgramId,
		project_id: ProjectId,
		revision: u64,
	) -> Result<Self, ContextRevisionError> {
		let revision = ContextRevisionNumber::new(revision)?.get();

		Ok(Self(ContextRevisionSourceValue::Program { program_id, project_id, revision }))
	}

	/// Bind one exact repository content revision to the supplied Project repository relation.
	pub fn repository(project: &Project, revision: RepositoryContentRevision) -> Self {
		Self(ContextRevisionSourceValue::Repository {
			project_id: project.id().clone(),
			repository_id: project.repository().identity().clone(),
			revision,
		})
	}

	/// Record one supplied Project-owned Policy revision.
	pub const fn policy(revision: PolicyRevisionId) -> Self {
		Self(ContextRevisionSourceValue::Policy(revision))
	}

	/// Record one supplied immutable ContextRevision reference.
	pub const fn context_revision(reference: ContextRevisionReference) -> Self {
		Self(ContextRevisionSourceValue::ContextRevision(reference))
	}

	/// Exact Project identity and positive revision only for a Project source.
	pub const fn project_revision(&self) -> Option<(&ProjectId, u64)> {
		match &self.0 {
			ContextRevisionSourceValue::Project { project_id, revision } =>
				Some((project_id, *revision)),
			_ => None,
		}
	}

	/// Exact Program, owning Project, and positive revision only for a Program source.
	pub const fn program_revision(&self) -> Option<(&ProgramId, &ProjectId, u64)> {
		match &self.0 {
			ContextRevisionSourceValue::Program { program_id, project_id, revision } =>
				Some((program_id, project_id, *revision)),
			_ => None,
		}
	}

	/// Exact Project, repository, and content revision only for a repository source.
	pub const fn repository_revision(
		&self,
	) -> Option<(&ProjectId, &RepositoryIdentity, &RepositoryContentRevision)> {
		match &self.0 {
			ContextRevisionSourceValue::Repository { project_id, repository_id, revision } =>
				Some((project_id, repository_id, revision)),
			_ => None,
		}
	}

	/// Exact Policy revision only for a Policy source.
	pub const fn policy_revision(&self) -> Option<&PolicyRevisionId> {
		match &self.0 {
			ContextRevisionSourceValue::Policy(revision) => Some(revision),
			_ => None,
		}
	}

	/// Exact ContextRevision reference only for a ContextRevision source.
	pub const fn context_revision_reference(&self) -> Option<&ContextRevisionReference> {
		match &self.0 {
			ContextRevisionSourceValue::ContextRevision(reference) => Some(reference),
			_ => None,
		}
	}
}

/// Required provenance for one ContextRevision item.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ContextRevisionItemProvenance {
	/// Explicit user assertion represented by the exact item value.
	UserAssertion,
	/// Typed source whose structural relationship to the owner is validated before sealing.
	Source(ContextRevisionSource),
}

/// Closed semantic class for immutable ContextRevision content.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ContextRevisionItemKind {
	/// Durable decision item.
	Decision,
	/// Active operating or implementation constraint.
	Constraint,
	/// Project, Advisor, or Program fact.
	Fact,
	/// Unresolved risk.
	Risk,
	/// Cross-run or owner-safe handoff item.
	Handoff,
}

/// One bounded immutable context item with mandatory provenance.
#[derive(Clone, Eq, PartialEq)]
pub struct ContextRevisionItem {
	id: ContextRevisionItemId,
	kind: ContextRevisionItemKind,
	text: String,
	provenance: ContextRevisionItemProvenance,
	pinned: bool,
}
impl ContextRevisionItem {
	/// Construct one unpinned item.
	pub fn new(
		id: ContextRevisionItemId,
		kind: ContextRevisionItemKind,
		text: impl Into<String>,
		provenance: ContextRevisionItemProvenance,
	) -> Result<Self, ContextRevisionError> {
		Self::from_stored(id, kind, text, provenance, false)
	}

	/// Reconstruct one stored-shaped item without proving that it was persisted or accepted.
	pub fn from_stored(
		id: ContextRevisionItemId,
		kind: ContextRevisionItemKind,
		text: impl Into<String>,
		provenance: ContextRevisionItemProvenance,
		pinned: bool,
	) -> Result<Self, ContextRevisionError> {
		let text = text.into();

		validate_item_text(&text)?;

		Ok(Self { id, kind, text, provenance, pinned })
	}

	/// Stable item identity.
	pub const fn id(&self) -> &ContextRevisionItemId {
		&self.id
	}

	/// Closed semantic class.
	pub const fn kind(&self) -> ContextRevisionItemKind {
		self.kind
	}

	/// Inspectable bounded item value.
	pub fn text(&self) -> &str {
		&self.text
	}

	/// Typed source or explicit user assertion.
	pub const fn provenance(&self) -> &ContextRevisionItemProvenance {
		&self.provenance
	}

	/// Whether the user pinned this item in this immutable revision.
	pub const fn pinned(&self) -> bool {
		self.pinned
	}
}

/// One immutable, structurally owner-safe ContextRevision with deterministic canonical bytes.
///
/// A value of this type is not proof of persistence, currentness, or acceptance.
#[derive(Clone, Eq, PartialEq)]
pub struct ContextRevision {
	id: ContextRevisionId,
	owner: ContextRevisionOwner,
	revision: ContextRevisionNumber,
	supersedes: Option<ContextRevisionReference>,
	items: Vec<ContextRevisionItem>,
	canonical_bytes: Vec<u8>,
	digest: BlobHash,
}
impl ContextRevision {
	/// Reconstruct from supplied stored fields and verify their canonical bytes and digest.
	///
	/// Successful validation proves structural consistency only. It does not prove persistence,
	/// currentness, source resolution, or acceptance.
	pub fn from_stored(
		id: ContextRevisionId,
		owner: ContextRevisionOwner,
		revision: ContextRevisionNumber,
		supersedes: Option<ContextRevisionReference>,
		items: Vec<ContextRevisionItem>,
		expected_canonical_bytes: &[u8],
		expected_digest: BlobHash,
	) -> Result<Self, ContextRevisionError> {
		let items = prepare_items(&owner, items)?;
		let value = Self::build_prepared(id, owner, revision, supersedes, items)?;

		if value.canonical_bytes.as_slice() != expected_canonical_bytes
			|| value.digest != expected_digest
		{
			return Err(ContextRevisionError::DigestMismatch);
		}

		Ok(value)
	}

	fn build_prepared(
		id: ContextRevisionId,
		owner: ContextRevisionOwner,
		revision: ContextRevisionNumber,
		supersedes: Option<ContextRevisionReference>,
		items: Vec<ContextRevisionItem>,
	) -> Result<Self, ContextRevisionError> {
		validate_supersession(&id, &owner, revision, supersedes.as_ref())?;

		let canonical_bytes = encode_revision(&id, &owner, revision, supersedes.as_ref(), &items)?;
		let digest = BlobHash::digest(&canonical_bytes);

		Ok(Self { id, owner, revision, supersedes, items, canonical_bytes, digest })
	}

	/// Stable ContextRevision identity.
	pub const fn id(&self) -> &ContextRevisionId {
		&self.id
	}

	/// Closed structural Project, unscoped Advisor, or Program owner tag.
	pub const fn owner(&self) -> &ContextRevisionOwner {
		&self.owner
	}

	/// Positive immutable revision number.
	pub const fn revision(&self) -> ContextRevisionNumber {
		self.revision
	}

	/// Structurally immediate predecessor reference, absent only on revision one.
	pub const fn supersedes(&self) -> Option<&ContextRevisionReference> {
		self.supersedes.as_ref()
	}

	/// Items in canonical semantic-class and identity order.
	pub fn items(&self) -> &[ContextRevisionItem] {
		&self.items
	}

	/// Exact canonical bytes.
	pub fn canonical_bytes(&self) -> &[u8] {
		&self.canonical_bytes
	}

	/// Digest of the exact canonical bytes.
	pub const fn digest(&self) -> BlobHash {
		self.digest
	}

	/// Construct an exact immutable reference.
	pub fn reference(&self) -> ContextRevisionReference {
		ContextRevisionReference::new(
			self.id.clone(),
			self.owner.clone(),
			self.revision,
			self.digest,
		)
	}
}

impl Debug for ContextRevision {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("ContextRevision")
			.field("id", &self.id)
			.field("owner", &self.owner)
			.field("revision", &self.revision)
			.field("supersedes", &self.supersedes)
			.field("item_count", &self.items.len())
			.field("byte_length", &self.canonical_bytes.len())
			.field("digest", &self.digest)
			.finish()
	}
}

/// Closed class of a proposed ContextRevision transition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ContextRevisionOperation {
	/// Propose revision one when the caller observed no current revision.
	Create,
	/// Propose new immutable content after the caller-observed revision.
	Supersede,
	/// Propose pinning one item after the caller-observed revision.
	Pin(ContextRevisionItemId),
	/// Propose unpinning one item after the caller-observed revision.
	Unpin(ContextRevisionItemId),
}

/// Pure optimistic transition proposal with no persistence or acceptance authority.
///
/// A storage or application transaction must load its authoritative current value, compare the
/// expected revision, resolve required sources, and decide whether to accept and persist this
/// proposal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextRevisionDecision {
	operation: ContextRevisionOperation,
	expected_revision: Option<ContextRevisionNumber>,
	proposed_revision: ContextRevision,
}
impl ContextRevisionDecision {
	/// Closed requested mutation.
	pub const fn operation(&self) -> &ContextRevisionOperation {
		&self.operation
	}

	/// Revision that the later transaction must compare; absent only for create.
	pub const fn expected_revision(&self) -> Option<ContextRevisionNumber> {
		self.expected_revision
	}

	/// Structurally valid proposed next revision.
	pub const fn proposed_revision(&self) -> &ContextRevision {
		&self.proposed_revision
	}

	/// Consume the pure proposal without granting persistence authority.
	pub fn into_revision(self) -> ContextRevision {
		self.proposed_revision
	}
}

/// Propose revision-one creation against a caller-supplied observation.
pub fn decide_create_context_revision(
	current: Option<&ContextRevision>,
	expected_revision: Option<ContextRevisionNumber>,
	id: ContextRevisionId,
	owner: ContextRevisionOwner,
	items: Vec<ContextRevisionItem>,
) -> Result<ContextRevisionDecision, ContextRevisionError> {
	let actual_revision = current.map(ContextRevision::revision);

	if expected_revision != actual_revision {
		return Err(ContextRevisionError::RevisionConflict);
	}
	if current.is_some() {
		return Err(ContextRevisionError::AlreadyExists);
	}
	if items.iter().any(ContextRevisionItem::pinned) {
		return Err(ContextRevisionError::PinnedItemViolation);
	}

	let items = prepare_items(&owner, items)?;
	let proposed_revision =
		ContextRevision::build_prepared(id, owner, ContextRevisionNumber(1), None, items)?;

	Ok(ContextRevisionDecision {
		operation: ContextRevisionOperation::Create,
		expected_revision: None,
		proposed_revision,
	})
}

/// Propose one exact immediate supersession without mutating the observed value.
pub fn decide_supersede_context_revision(
	current: &ContextRevision,
	expected_revision: ContextRevisionNumber,
	items: Vec<ContextRevisionItem>,
) -> Result<ContextRevisionDecision, ContextRevisionError> {
	require_current_revision(current, expected_revision)?;

	let items = prepare_items(current.owner(), items)?;

	validate_pinned_preservation(current.items(), &items)?;

	if current.items() == items.as_slice() {
		return Err(ContextRevisionError::UnchangedContent);
	}

	build_successor(current, ContextRevisionOperation::Supersede, items)
}

/// Propose one item pin as a new immutable immediate successor.
pub fn decide_pin_context_item(
	current: &ContextRevision,
	expected_revision: ContextRevisionNumber,
	item_id: &ContextRevisionItemId,
) -> Result<ContextRevisionDecision, ContextRevisionError> {
	require_current_revision(current, expected_revision)?;

	let mut items = current.items.clone();
	let item = items
		.iter_mut()
		.find(|item| item.id() == item_id)
		.ok_or(ContextRevisionError::MissingItem)?;

	if item.pinned {
		return Err(ContextRevisionError::InvalidPinState);
	}

	item.pinned = true;

	build_successor(current, ContextRevisionOperation::Pin(item_id.clone()), items)
}

/// Propose one item unpin as a new immutable immediate successor.
pub fn decide_unpin_context_item(
	current: &ContextRevision,
	expected_revision: ContextRevisionNumber,
	item_id: &ContextRevisionItemId,
) -> Result<ContextRevisionDecision, ContextRevisionError> {
	require_current_revision(current, expected_revision)?;

	let mut items = current.items.clone();
	let item = items
		.iter_mut()
		.find(|item| item.id() == item_id)
		.ok_or(ContextRevisionError::MissingItem)?;

	if !item.pinned {
		return Err(ContextRevisionError::InvalidPinState);
	}

	item.pinned = false;

	build_successor(current, ContextRevisionOperation::Unpin(item_id.clone()), items)
}

/// Closed ContextRevision construction or optimistic-decision failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContextRevisionError {
	/// A stable identity was not canonical lowercase UUID-v4 text.
	InvalidIdentity(&'static str),
	/// A ContextRevision or provenance source revision was not positive, or incrementing
	/// overflowed.
	InvalidRevision,
	/// An owner tag was not a Project, unscoped Advisor, or Program relation.
	InvalidOwner,
	/// An item value was empty, oversized, or contained controls.
	InvalidItemText,
	/// Item content or identities were duplicated or outside their closed bound.
	InvalidContent,
	/// Item provenance was unrelated to the ContextRevision owner.
	InvalidProvenance,
	/// Ordinary context content contained concrete credential material.
	CredentialRejected,
	/// Immutable supersession did not name the exact immediate predecessor.
	InvalidSupersession,
	/// A canonical revision encoding exceeded its hard byte bound.
	ContextTooLarge,
	/// Supplied stored bytes or digest did not match canonical immutable content.
	DigestMismatch,
	/// The expected revision did not equal the caller-supplied observed revision.
	RevisionConflict,
	/// Create was requested with a caller-supplied existing revision.
	AlreadyExists,
	/// A requested item identity was not in the caller-supplied revision.
	MissingItem,
	/// Pin or unpin requested the item's existing state.
	InvalidPinState,
	/// Create or ordinary supersession attempted to bypass an explicit pin operation.
	PinnedItemViolation,
	/// Supersession proposed byte-identical canonical content.
	UnchangedContent,
}
impl Error for ContextRevisionError {}

impl Display for ContextRevisionError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::InvalidIdentity(kind) => write!(formatter, "invalid {kind} identity"),
			Self::InvalidRevision => formatter.write_str("invalid ContextRevision revision value"),
			Self::InvalidOwner => formatter.write_str("invalid ContextRevision owner"),
			Self::InvalidItemText => formatter.write_str("invalid ContextRevision item text"),
			Self::InvalidContent => formatter.write_str("invalid ContextRevision content"),
			Self::InvalidProvenance =>
				formatter.write_str("ContextRevision provenance is unrelated to its owner"),
			Self::CredentialRejected =>
				formatter.write_str("credential-bearing ContextRevision content rejected"),
			Self::InvalidSupersession =>
				formatter.write_str("invalid ContextRevision supersession"),
			Self::ContextTooLarge => formatter.write_str("ContextRevision exceeds its byte bound"),
			Self::DigestMismatch => formatter.write_str("ContextRevision digest mismatch"),
			Self::RevisionConflict => formatter.write_str("ContextRevision revision conflict"),
			Self::AlreadyExists => formatter.write_str("ContextRevision already exists"),
			Self::MissingItem => formatter.write_str("ContextRevision item not found"),
			Self::InvalidPinState => formatter.write_str("invalid ContextRevision pin state"),
			Self::PinnedItemViolation =>
				formatter.write_str("ContextRevision pinned item transition violated"),
			Self::UnchangedContent =>
				formatter.write_str("ContextRevision supersession is unchanged"),
		}
	}
}

fn build_successor(
	current: &ContextRevision,
	operation: ContextRevisionOperation,
	items: Vec<ContextRevisionItem>,
) -> Result<ContextRevisionDecision, ContextRevisionError> {
	let expected_revision = current.revision();
	let proposed_revision = ContextRevision::build_prepared(
		current.id().clone(),
		current.owner().clone(),
		expected_revision.next()?,
		Some(current.reference()),
		items,
	)?;

	Ok(ContextRevisionDecision {
		operation,
		expected_revision: Some(expected_revision),
		proposed_revision,
	})
}

fn require_current_revision(
	current: &ContextRevision,
	expected_revision: ContextRevisionNumber,
) -> Result<(), ContextRevisionError> {
	if current.revision() != expected_revision {
		Err(ContextRevisionError::RevisionConflict)
	} else {
		Ok(())
	}
}

fn validate_pinned_preservation(
	current: &[ContextRevisionItem],
	proposed: &[ContextRevisionItem],
) -> Result<(), ContextRevisionError> {
	let all_current_pins_preserved = current
		.iter()
		.filter(|item| item.pinned())
		.all(|item| proposed.iter().any(|candidate| candidate == item));
	let no_new_pins = proposed
		.iter()
		.filter(|item| item.pinned())
		.all(|item| current.iter().any(|candidate| candidate.pinned() && candidate == item));

	if all_current_pins_preserved && no_new_pins {
		Ok(())
	} else {
		Err(ContextRevisionError::PinnedItemViolation)
	}
}

fn prepare_items(
	owner: &ContextRevisionOwner,
	mut items: Vec<ContextRevisionItem>,
) -> Result<Vec<ContextRevisionItem>, ContextRevisionError> {
	if items.len() > MAX_CONTEXT_REVISION_ITEMS {
		return Err(ContextRevisionError::InvalidContent);
	}
	if items
		.iter()
		.enumerate()
		.any(|(position, item)| items[..position].iter().any(|previous| previous.id() == item.id()))
	{
		return Err(ContextRevisionError::InvalidContent);
	}

	for item in &items {
		validate_item_provenance(owner, item)?;
	}

	items.sort_by(compare_items);

	Ok(items)
}

fn validate_item_provenance(
	owner: &ContextRevisionOwner,
	item: &ContextRevisionItem,
) -> Result<(), ContextRevisionError> {
	let valid = match item.provenance() {
		ContextRevisionItemProvenance::UserAssertion => true,
		ContextRevisionItemProvenance::Source(source) =>
			source_is_related(owner, item.kind(), source),
	};

	if valid { Ok(()) } else { Err(ContextRevisionError::InvalidProvenance) }
}

fn source_is_related(
	owner: &ContextRevisionOwner,
	item_kind: ContextRevisionItemKind,
	source: &ContextRevisionSource,
) -> bool {
	match &source.0 {
		ContextRevisionSourceValue::Project { project_id, .. }
		| ContextRevisionSourceValue::Repository { project_id, .. } =>
			owner.project_id() == Some(project_id),
		ContextRevisionSourceValue::Program { program_id, project_id, .. } => match &owner.0 {
			ContextRevisionOwnerValue::Project(owner_project_id) => owner_project_id == project_id,
			ContextRevisionOwnerValue::Program {
				program_id: owner_program_id,
				project_id: owner_project_id,
			} => owner_program_id == program_id && owner_project_id == project_id,
			ContextRevisionOwnerValue::Advisor(_) => false,
		},
		ContextRevisionSourceValue::Policy(revision) =>
			owner.project_id() == Some(revision.project_id()),
		ContextRevisionSourceValue::ContextRevision(reference) =>
			revision_source_is_related(owner, item_kind, reference.owner()),
	}
}

fn revision_source_is_related(
	owner: &ContextRevisionOwner,
	item_kind: ContextRevisionItemKind,
	source_owner: &ContextRevisionOwner,
) -> bool {
	if source_owner == owner {
		return true;
	}
	if item_kind != ContextRevisionItemKind::Handoff {
		return false;
	}

	match (&owner.0, &source_owner.0) {
		(ContextRevisionOwnerValue::Advisor(_), ContextRevisionOwnerValue::Project(_))
		| (ContextRevisionOwnerValue::Advisor(_), ContextRevisionOwnerValue::Program { .. })
		| (ContextRevisionOwnerValue::Project(_), ContextRevisionOwnerValue::Advisor(_))
		| (ContextRevisionOwnerValue::Program { .. }, ContextRevisionOwnerValue::Advisor(_)) => true,
		(
			ContextRevisionOwnerValue::Project(project_id),
			ContextRevisionOwnerValue::Program { project_id: source_project_id, .. },
		)
		| (
			ContextRevisionOwnerValue::Program { project_id, .. },
			ContextRevisionOwnerValue::Project(source_project_id),
		) => project_id == source_project_id,
		_ => false,
	}
}

fn validate_supersession(
	id: &ContextRevisionId,
	owner: &ContextRevisionOwner,
	revision: ContextRevisionNumber,
	supersedes: Option<&ContextRevisionReference>,
) -> Result<(), ContextRevisionError> {
	match (revision.get(), supersedes) {
		(1, None) => Ok(()),
		(next, Some(previous))
			if previous.id() == id
				&& previous.owner() == owner
				&& previous.revision().get().checked_add(1) == Some(next) =>
			Ok(()),
		_ => Err(ContextRevisionError::InvalidSupersession),
	}
}

fn validate_item_text(value: &str) -> Result<(), ContextRevisionError> {
	if value.is_empty()
		|| value.len() > MAX_CONTEXT_REVISION_ITEM_BYTES
		|| value.chars().any(char::is_control)
	{
		return Err(ContextRevisionError::InvalidItemText);
	}
	if crate::contains_credential_material(value) {
		return Err(ContextRevisionError::CredentialRejected);
	}

	Ok(())
}

fn compare_items(left: &ContextRevisionItem, right: &ContextRevisionItem) -> Ordering {
	item_kind_tag(left.kind())
		.cmp(&item_kind_tag(right.kind()))
		.then_with(|| left.id().cmp(right.id()))
}

fn encoded_owner_length(owner: &ContextRevisionOwner) -> usize {
	match &owner.0 {
		ContextRevisionOwnerValue::Project(_) | ContextRevisionOwnerValue::Advisor(_) =>
			1 + UUID_BYTES,
		ContextRevisionOwnerValue::Program { .. } => 1 + UUID_BYTES * 2,
	}
}

fn encoded_reference_length(reference: &ContextRevisionReference) -> usize {
	UUID_BYTES + encoded_owner_length(reference.owner()) + 8 + DIGEST_HEX_BYTES
}

fn encoded_source_length(source: &ContextRevisionSource) -> usize {
	match &source.0 {
		ContextRevisionSourceValue::Project { .. } => 1 + UUID_BYTES + 8,
		ContextRevisionSourceValue::Program { .. } => 1 + UUID_BYTES * 2 + 8,
		ContextRevisionSourceValue::Repository { repository_id, revision, .. } =>
			1 + UUID_BYTES + 2 + repository_id.as_str().len() + 2 + revision.as_str().len(),
		ContextRevisionSourceValue::Policy(_) => 1 + UUID_BYTES * 2 + 8,
		ContextRevisionSourceValue::ContextRevision(reference) =>
			1 + encoded_reference_length(reference),
	}
}

fn encoded_item_length(item: &ContextRevisionItem) -> usize {
	let provenance_length = match item.provenance() {
		ContextRevisionItemProvenance::UserAssertion => 1,
		ContextRevisionItemProvenance::Source(source) => 1 + encoded_source_length(source),
	};

	1 + UUID_BYTES + 1 + 2 + item.text().len() + provenance_length
}

fn encoded_revision_length(
	owner: &ContextRevisionOwner,
	supersedes: Option<&ContextRevisionReference>,
	items: &[ContextRevisionItem],
) -> Result<usize, ContextRevisionError> {
	let predecessor_length = supersedes.map_or(0, encoded_reference_length);
	let header_length = CONTEXT_REVISION_MAGIC.len()
		+ UUID_BYTES
		+ encoded_owner_length(owner)
		+ 8 + 1
		+ predecessor_length
		+ 2;
	let length = items.iter().try_fold(header_length, |length, item| {
		length.checked_add(encoded_item_length(item)).ok_or(ContextRevisionError::ContextTooLarge)
	})?;

	if length > MAX_CONTEXT_REVISION_BYTES {
		return Err(ContextRevisionError::ContextTooLarge);
	}

	Ok(length)
}

fn encode_revision(
	id: &ContextRevisionId,
	owner: &ContextRevisionOwner,
	revision: ContextRevisionNumber,
	supersedes: Option<&ContextRevisionReference>,
	items: &[ContextRevisionItem],
) -> Result<Vec<u8>, ContextRevisionError> {
	let encoded_length = encoded_revision_length(owner, supersedes, items)?;
	let mut bytes = Vec::with_capacity(encoded_length);

	bytes.extend_from_slice(CONTEXT_REVISION_MAGIC);
	bytes.extend_from_slice(id.as_str().as_bytes());

	push_owner(&mut bytes, owner);

	bytes.extend_from_slice(&revision.get().to_be_bytes());

	match supersedes {
		Some(previous) => {
			bytes.push(1);
			push_reference(&mut bytes, previous);
		},
		None => bytes.push(0),
	}

	let item_count =
		u16::try_from(items.len()).map_err(|_| ContextRevisionError::InvalidContent)?;

	bytes.extend_from_slice(&item_count.to_be_bytes());

	for item in items {
		bytes.push(item_kind_tag(item.kind()));
		bytes.extend_from_slice(item.id().as_str().as_bytes());
		bytes.push(u8::from(item.pinned()));

		push_text_u16(&mut bytes, item.text())?;
		push_provenance(&mut bytes, item.provenance())?;
	}

	if bytes.len() != encoded_length {
		return Err(ContextRevisionError::ContextTooLarge);
	}

	Ok(bytes)
}

fn push_owner(bytes: &mut Vec<u8>, owner: &ContextRevisionOwner) {
	match &owner.0 {
		ContextRevisionOwnerValue::Project(project_id) => {
			bytes.push(0);
			bytes.extend_from_slice(project_id.as_str().as_bytes());
		},
		ContextRevisionOwnerValue::Advisor(agent_id) => {
			bytes.push(1);
			bytes.extend_from_slice(agent_id.as_str().as_bytes());
		},
		ContextRevisionOwnerValue::Program { program_id, project_id } => {
			bytes.push(2);
			bytes.extend_from_slice(program_id.as_str().as_bytes());
			bytes.extend_from_slice(project_id.as_str().as_bytes());
		},
	}
}

fn push_reference(bytes: &mut Vec<u8>, reference: &ContextRevisionReference) {
	bytes.extend_from_slice(reference.id().as_str().as_bytes());
	push_owner(bytes, reference.owner());
	bytes.extend_from_slice(&reference.revision().get().to_be_bytes());
	bytes.extend_from_slice(reference.digest().to_hex().as_bytes());
}

fn push_provenance(
	bytes: &mut Vec<u8>,
	provenance: &ContextRevisionItemProvenance,
) -> Result<(), ContextRevisionError> {
	match provenance {
		ContextRevisionItemProvenance::UserAssertion => bytes.push(0),
		ContextRevisionItemProvenance::Source(source) => {
			bytes.push(1);
			push_source(bytes, source)?;
		},
	}

	Ok(())
}

fn push_source(
	bytes: &mut Vec<u8>,
	source: &ContextRevisionSource,
) -> Result<(), ContextRevisionError> {
	match &source.0 {
		ContextRevisionSourceValue::Project { project_id, revision } => {
			bytes.push(0);
			bytes.extend_from_slice(project_id.as_str().as_bytes());
			bytes.extend_from_slice(&revision.to_be_bytes());
		},
		ContextRevisionSourceValue::Program { program_id, project_id, revision } => {
			bytes.push(1);
			bytes.extend_from_slice(program_id.as_str().as_bytes());
			bytes.extend_from_slice(project_id.as_str().as_bytes());
			bytes.extend_from_slice(&revision.to_be_bytes());
		},
		ContextRevisionSourceValue::Repository { project_id, repository_id, revision } => {
			bytes.push(2);
			bytes.extend_from_slice(project_id.as_str().as_bytes());
			push_text_u16(bytes, repository_id.as_str())?;
			push_text_u16(bytes, revision.as_str())?;
		},
		ContextRevisionSourceValue::Policy(revision) => {
			bytes.push(3);
			bytes.extend_from_slice(revision.project_id().as_str().as_bytes());
			bytes.extend_from_slice(revision.policy_id().as_str().as_bytes());
			bytes.extend_from_slice(&revision.revision().get().to_be_bytes());
		},
		ContextRevisionSourceValue::ContextRevision(reference) => {
			bytes.push(4);
			push_reference(bytes, reference);
		},
	}

	Ok(())
}

fn push_text_u16(bytes: &mut Vec<u8>, value: &str) -> Result<(), ContextRevisionError> {
	let length = u16::try_from(value.len()).map_err(|_| ContextRevisionError::ContextTooLarge)?;

	bytes.extend_from_slice(&length.to_be_bytes());
	bytes.extend_from_slice(value.as_bytes());

	Ok(())
}

fn item_kind_tag(kind: ContextRevisionItemKind) -> u8 {
	match kind {
		ContextRevisionItemKind::Decision => 0,
		ContextRevisionItemKind::Constraint => 1,
		ContextRevisionItemKind::Fact => 2,
		ContextRevisionItemKind::Risk => 3,
		ContextRevisionItemKind::Handoff => 4,
	}
}

fn is_canonical_uuid_v4(value: &str) -> bool {
	let bytes = value.as_bytes();

	bytes.len() == UUID_BYTES
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
	use std::path::PathBuf;

	use super::{CONTEXT_REVISION_MAGIC, UUID_BYTES, push_source};
	use crate::{
		Agent, AgentId, BlobHash, ContextRevision, ContextRevisionError, ContextRevisionId,
		ContextRevisionItem, ContextRevisionItemId, ContextRevisionItemKind,
		ContextRevisionItemProvenance, ContextRevisionNumber, ContextRevisionOwner,
		ContextRevisionReference, ContextRevisionSource, MAX_CONTEXT_REVISION_BYTES,
		MAX_CONTEXT_REVISION_ITEM_BYTES, MAX_CONTEXT_REVISION_ITEMS, PolicyId, PolicyRevision,
		PolicyRevisionId, Program, ProgramId, ProgramState, ProgramTimestamp, Project, ProjectId,
		ProjectMetadata, ProjectRepositoryBinding, ProjectStatus, RepositoryContentRevision,
		RepositoryIdentity, ReviewCadence, decide_create_context_revision, decide_pin_context_item,
		decide_supersede_context_revision, decide_unpin_context_item,
	};

	fn project(value: u8) -> Project {
		let root = PathBuf::from(format!("/tmp/decodex-context-project-{value}"));
		Project::new(
			ProjectId::new(format!("10000000-0000-4000-8000-{value:012x}")).unwrap(),
			ProjectRepositoryBinding::new(
				RepositoryIdentity::new(format!("acg-box/context-project-{value}")).unwrap(),
				root.clone(),
				root,
			)
			.unwrap(),
			ProjectMetadata::empty(),
		)
	}

	fn program(value: u8, project: &Project) -> Program {
		Program::new(
			ProgramId::new(format!("30000000-0000-4000-8000-{value:012x}")).unwrap(),
			project.id().clone(),
			AgentId::new(format!("20000000-0000-4000-8000-{value:012x}")).unwrap(),
			format!("Program {value}"),
			"Own one bounded responsibility.",
			PolicyRevisionId::new(
				project.id().clone(),
				PolicyId::new(format!("40000000-0000-4000-8000-{value:012x}")).unwrap(),
				PolicyRevision::new(1).unwrap(),
			),
			ReviewCadence::new(
				7,
				ProgramTimestamp::from_unix_microseconds(i64::from(value)).unwrap(),
			)
			.unwrap(),
		)
		.unwrap()
	}

	fn revision_id(value: u16) -> ContextRevisionId {
		ContextRevisionId::new(format!("70000000-0000-4000-8000-{value:012x}")).unwrap()
	}

	fn item_id(value: u16) -> ContextRevisionItemId {
		ContextRevisionItemId::new(format!("71000000-0000-4000-8000-{value:012x}")).unwrap()
	}

	fn revision(value: u64) -> ContextRevisionNumber {
		ContextRevisionNumber::new(value).unwrap()
	}

	fn item(
		value: u16,
		kind: ContextRevisionItemKind,
		text: impl Into<String>,
		provenance: ContextRevisionItemProvenance,
		pinned: bool,
	) -> ContextRevisionItem {
		ContextRevisionItem::from_stored(item_id(value), kind, text, provenance, pinned).unwrap()
	}

	fn assertion(
		value: u16,
		kind: ContextRevisionItemKind,
		text: impl Into<String>,
	) -> ContextRevisionItem {
		item(value, kind, text, ContextRevisionItemProvenance::UserAssertion, false)
	}

	fn sourced_fact(
		value: u16,
		text: impl Into<String>,
		source: ContextRevisionSource,
	) -> ContextRevisionItem {
		item(
			value,
			ContextRevisionItemKind::Fact,
			text,
			ContextRevisionItemProvenance::Source(source),
			false,
		)
	}

	fn create(
		value: u16,
		owner: ContextRevisionOwner,
		items: Vec<ContextRevisionItem>,
	) -> ContextRevision {
		decide_create_context_revision(None, None, revision_id(value), owner, items)
			.unwrap()
			.into_revision()
	}

	fn advisor(value: u16) -> Agent {
		Agent::advisor(AgentId::new(format!("22000000-0000-4000-8000-{value:012x}")).unwrap())
	}

	fn assertions(count: usize, text_bytes: usize) -> Vec<ContextRevisionItem> {
		(1..=count)
			.map(|value| {
				assertion(
					u16::try_from(value).unwrap(),
					ContextRevisionItemKind::Fact,
					"x".repeat(text_bytes),
				)
			})
			.collect()
	}

	fn aggregate_bound_items(last_text_bytes: usize) -> Vec<ContextRevisionItem> {
		let mut items = assertions(31, MAX_CONTEXT_REVISION_ITEM_BYTES);
		items.push(assertion(32, ContextRevisionItemKind::Fact, "x".repeat(last_text_bytes)));
		items
	}

	fn context_source(value: u16, owner: &ContextRevisionOwner) -> ContextRevisionSource {
		ContextRevisionSource::context_revision(ContextRevisionReference::new(
			revision_id(value),
			owner.clone(),
			revision(1),
			BlobHash::digest(&value.to_be_bytes()),
		))
	}

	fn provenance_result(
		owner: &ContextRevisionOwner,
		kind: ContextRevisionItemKind,
		provenance: ContextRevisionItemProvenance,
	) -> Result<(), ContextRevisionError> {
		let item = ContextRevisionItem::new(item_id(900), kind, "matrix", provenance)?;

		decide_create_context_revision(None, None, revision_id(900), owner.clone(), vec![item])
			.map(|_| ())
	}

	fn assert_provenance_cases(
		kind: ContextRevisionItemKind,
		cases: &[(&ContextRevisionOwner, &ContextRevisionSource, bool)],
	) {
		for &(owner, source, expected) in cases {
			let expected =
				if expected { Ok(()) } else { Err(ContextRevisionError::InvalidProvenance) };

			assert_eq!(
				provenance_result(
					owner,
					kind,
					ContextRevisionItemProvenance::Source(source.clone()),
				),
				expected
			);
		}
	}

	fn restore(
		value: &ContextRevision,
		expected_canonical_bytes: &[u8],
		expected_digest: BlobHash,
	) -> Result<ContextRevision, ContextRevisionError> {
		ContextRevision::from_stored(
			value.id().clone(),
			value.owner().clone(),
			value.revision(),
			value.supersedes().cloned(),
			value.items().to_vec(),
			expected_canonical_bytes,
			expected_digest,
		)
	}

	fn restore_lineage(
		value: &ContextRevision,
		revision: ContextRevisionNumber,
		supersedes: Option<ContextRevisionReference>,
	) -> Result<ContextRevision, ContextRevisionError> {
		ContextRevision::from_stored(
			value.id().clone(),
			value.owner().clone(),
			revision,
			supersedes,
			value.items().to_vec(),
			value.canonical_bytes(),
			value.digest(),
		)
	}

	#[test]
	fn version_one_encoding_matches_a_fixed_byte_vector_and_digest() {
		let project = project(1);
		let value = create(
			1,
			ContextRevisionOwner::project(&project),
			vec![assertion(1, ContextRevisionItemKind::Decision, "fixed")],
		);
		let expected = b"decodex/context-revision/1\x00\
70000000-0000-4000-8000-000000000001\
\x00\
10000000-0000-4000-8000-000000000001\
\x00\x00\x00\x00\x00\x00\x00\x01\
\x00\
\x00\x01\
\x00\
71000000-0000-4000-8000-000000000001\
\x00\
\x00\x05fixed\
\x00";

		assert_eq!(value.canonical_bytes(), expected);
		assert_eq!(
			value.digest().to_hex(),
			"247a8da1c6299fe62be87ddd7c49ae4dfe339638fc3d199865e9c03123ebba81"
		);
	}

	#[test]
	fn every_source_variant_has_a_fixed_v1_encoding_tag() {
		let project = project(1);
		let program = program(1, &project);
		let project_owner = ContextRevisionOwner::project(&project);

		let sources = [
			(ContextRevisionSource::project_from_stored(project.id().clone(), 7).unwrap(), 0_u8),
			(
				ContextRevisionSource::program_from_stored(
					program.id().clone(),
					project.id().clone(),
					9,
				)
				.unwrap(),
				1,
			),
			(
				ContextRevisionSource::repository(
					&project,
					RepositoryContentRevision::new("repository-v1").unwrap(),
				),
				2,
			),
			(ContextRevisionSource::policy(program.policy_revision_id().clone()), 3),
			(
				ContextRevisionSource::context_revision(ContextRevisionReference::new(
					revision_id(70),
					project_owner,
					revision(3),
					BlobHash::digest(b"context-source-v1"),
				)),
				4,
			),
		];

		for (source, expected_tag) in sources {
			let mut encoded = Vec::new();
			push_source(&mut encoded, &source).unwrap();

			assert_eq!(encoded[0], expected_tag);
		}
	}

	#[test]
	fn item_and_collection_bounds_are_exact() {
		let project = project(1);
		let owner = ContextRevisionOwner::project(&project);
		let below_text = ContextRevisionItem::new(
			item_id(901),
			ContextRevisionItemKind::Fact,
			"x".repeat(MAX_CONTEXT_REVISION_ITEM_BYTES - 1),
			ContextRevisionItemProvenance::UserAssertion,
		)
		.unwrap();
		let at_text = ContextRevisionItem::new(
			item_id(902),
			ContextRevisionItemKind::Fact,
			"x".repeat(MAX_CONTEXT_REVISION_ITEM_BYTES),
			ContextRevisionItemProvenance::UserAssertion,
		)
		.unwrap();

		assert_eq!(below_text.text().len(), MAX_CONTEXT_REVISION_ITEM_BYTES - 1);
		assert_eq!(at_text.text().len(), MAX_CONTEXT_REVISION_ITEM_BYTES);
		assert!(matches!(
			ContextRevisionItem::new(
				item_id(903),
				ContextRevisionItemKind::Fact,
				"x".repeat(MAX_CONTEXT_REVISION_ITEM_BYTES + 1),
				ContextRevisionItemProvenance::UserAssertion,
			),
			Err(ContextRevisionError::InvalidItemText)
		));

		let below_count = create(10, owner.clone(), assertions(MAX_CONTEXT_REVISION_ITEMS - 1, 1));
		let at_count = create(11, owner.clone(), assertions(MAX_CONTEXT_REVISION_ITEMS, 1));

		assert_eq!(below_count.items().len(), MAX_CONTEXT_REVISION_ITEMS - 1);
		assert_eq!(at_count.items().len(), MAX_CONTEXT_REVISION_ITEMS);
		assert!(matches!(
			decide_create_context_revision(
				None,
				None,
				revision_id(12),
				owner.clone(),
				assertions(MAX_CONTEXT_REVISION_ITEMS + 1, 1),
			),
			Err(ContextRevisionError::InvalidContent)
		));
	}

	#[test]
	fn item_api_rejects_concrete_credential_material() {
		assert!(matches!(
			ContextRevisionItem::new(
				item_id(905),
				ContextRevisionItemKind::Fact,
				"secret=abcd",
				ContextRevisionItemProvenance::UserAssertion,
			),
			Err(ContextRevisionError::CredentialRejected)
		));
		assert!(
			ContextRevisionItem::new(
				item_id(906),
				ContextRevisionItemKind::Fact,
				"token budget",
				ContextRevisionItemProvenance::UserAssertion,
			)
			.is_ok()
		);
	}

	#[test]
	fn aggregate_canonical_byte_bound_is_exact() {
		let project = project(1);
		let owner = ContextRevisionOwner::project(&project);

		// The V1 Project/revision-one header is 111 bytes. Each assertion item adds 41 bytes.
		let below = create(20, owner.clone(), aggregate_bound_items(2_672));
		let at = create(21, owner.clone(), aggregate_bound_items(2_673));

		assert_eq!(below.canonical_bytes().len(), MAX_CONTEXT_REVISION_BYTES - 1);
		assert_eq!(at.canonical_bytes().len(), MAX_CONTEXT_REVISION_BYTES);
		assert!(matches!(
			decide_create_context_revision(
				None,
				None,
				revision_id(22),
				owner,
				aggregate_bound_items(2_674),
			),
			Err(ContextRevisionError::ContextTooLarge)
		));
	}

	#[test]
	fn equivalent_inputs_have_deterministic_canonical_bytes_and_digest() {
		let project = project(1);
		let owner = ContextRevisionOwner::project(&project);
		let sourced = ContextRevisionItem::new(
			item_id(2),
			ContextRevisionItemKind::Fact,
			"The repository content is exact.",
			ContextRevisionItemProvenance::Source(ContextRevisionSource::repository(
				&project,
				RepositoryContentRevision::new("89f7c7b6768f28c34035c7b161921f6c7ce127fc").unwrap(),
			)),
		)
		.unwrap();
		let asserted = assertion(1, ContextRevisionItemKind::Decision, "Keep immutable context.");
		let first = create(1, owner.clone(), vec![sourced.clone(), asserted.clone()]);
		let second = create(1, owner, vec![asserted, sourced]);

		assert_eq!(first.canonical_bytes(), second.canonical_bytes());
		assert_eq!(first.digest(), second.digest());
		assert_eq!(first.items()[0].kind(), ContextRevisionItemKind::Decision);
		assert_eq!(first.items()[1].kind(), ContextRevisionItemKind::Fact);
	}

	#[test]
	fn historical_project_and_program_sources_survive_live_aggregate_advancement() {
		let mut project = project(1);
		let mut program = program(1, &project);
		let project_id = project.id().clone();
		let program_id = program.id().clone();
		let project_revision = project.revision();
		let program_revision = program.revision();
		let project_owner = ContextRevisionOwner::project(&project);
		let program_owner = ContextRevisionOwner::program(&program);
		let historical_project_source = ContextRevisionSource::project(&project);
		let historical_program_source = ContextRevisionSource::program(&program);
		let project_before = create(
			60,
			project_owner.clone(),
			vec![sourced_fact(60, "Historical Project revision.", historical_project_source)],
		);
		let program_before = create(
			61,
			program_owner.clone(),
			vec![sourced_fact(61, "Historical Program revision.", historical_program_source)],
		);
		let owner_tag_offset = CONTEXT_REVISION_MAGIC.len() + UUID_BYTES;

		assert_eq!(program_before.canonical_bytes()[owner_tag_offset], 2);

		project.transition(project_revision, ProjectStatus::Paused).unwrap();
		program.transition(program_revision, ProgramState::Paused).unwrap();

		assert_eq!(project.revision(), project_revision + 1);
		assert_eq!(program.revision(), program_revision + 1);

		let historical_project_source =
			ContextRevisionSource::project_from_stored(project_id.clone(), project_revision)
				.unwrap();
		let historical_program_source = ContextRevisionSource::program_from_stored(
			program_id.clone(),
			project_id.clone(),
			program_revision,
		)
		.unwrap();

		assert_eq!(
			historical_project_source.project_revision(),
			Some((&project_id, project_revision))
		);
		assert_eq!(
			historical_program_source.program_revision(),
			Some((&program_id, &project_id, program_revision))
		);

		let project_reconstructed = ContextRevision::from_stored(
			project_before.id().clone(),
			project_owner,
			project_before.revision(),
			project_before.supersedes().cloned(),
			vec![sourced_fact(60, "Historical Project revision.", historical_project_source)],
			project_before.canonical_bytes(),
			project_before.digest(),
		)
		.unwrap();
		let program_reconstructed = ContextRevision::from_stored(
			program_before.id().clone(),
			program_owner,
			program_before.revision(),
			program_before.supersedes().cloned(),
			vec![sourced_fact(61, "Historical Program revision.", historical_program_source)],
			program_before.canonical_bytes(),
			program_before.digest(),
		)
		.unwrap();

		assert_eq!(project_reconstructed.canonical_bytes(), project_before.canonical_bytes());
		assert_eq!(project_reconstructed.digest(), project_before.digest());
		assert_eq!(program_reconstructed.canonical_bytes(), program_before.canonical_bytes());
		assert_eq!(program_reconstructed.digest(), program_before.digest());
		assert_eq!(
			ContextRevisionSource::project_from_stored(project_id.clone(), 0),
			Err(ContextRevisionError::InvalidRevision)
		);
		assert_eq!(
			ContextRevisionSource::program_from_stored(program_id, project_id, 0),
			Err(ContextRevisionError::InvalidRevision)
		);
	}

	#[test]
	fn from_stored_rejects_hostile_bytes_digest_and_lineage() {
		let project_a = project(1);
		let project_b = project(2);
		let current = create(
			30,
			ContextRevisionOwner::project(&project_a),
			vec![assertion(1, ContextRevisionItemKind::Decision, "Revision one.")],
		);
		let successor = decide_supersede_context_revision(
			&current,
			current.revision(),
			vec![assertion(1, ContextRevisionItemKind::Decision, "Revision two.")],
		)
		.unwrap()
		.into_revision();

		assert_eq!(
			restore(&current, current.canonical_bytes(), current.digest()),
			Ok(current.clone())
		);
		assert_eq!(
			restore(&successor, successor.canonical_bytes(), successor.digest()),
			Ok(successor.clone())
		);

		let mut hostile_bytes = successor.canonical_bytes().to_vec();
		hostile_bytes[0] ^= 1;
		assert_eq!(
			restore(&successor, &hostile_bytes, successor.digest()),
			Err(ContextRevisionError::DigestMismatch)
		);
		assert_eq!(
			restore(&successor, successor.canonical_bytes(), BlobHash::digest(b"hostile digest"),),
			Err(ContextRevisionError::DigestMismatch)
		);

		assert_eq!(
			restore_lineage(&successor, successor.revision(), None),
			Err(ContextRevisionError::InvalidSupersession)
		);
		assert_eq!(
			restore_lineage(
				&successor,
				successor.revision(),
				Some(ContextRevisionReference::new(
					revision_id(31),
					successor.owner().clone(),
					current.revision(),
					current.digest(),
				)),
			),
			Err(ContextRevisionError::InvalidSupersession)
		);
		assert_eq!(
			restore_lineage(
				&successor,
				successor.revision(),
				Some(ContextRevisionReference::new(
					successor.id().clone(),
					ContextRevisionOwner::project(&project_b),
					current.revision(),
					current.digest(),
				)),
			),
			Err(ContextRevisionError::InvalidSupersession)
		);
		assert_eq!(
			restore_lineage(
				&successor,
				successor.revision(),
				Some(ContextRevisionReference::new(
					successor.id().clone(),
					successor.owner().clone(),
					successor.revision(),
					current.digest(),
				)),
			),
			Err(ContextRevisionError::InvalidSupersession)
		);
		assert_eq!(
			restore_lineage(&current, current.revision(), Some(current.reference())),
			Err(ContextRevisionError::InvalidSupersession)
		);
		assert_eq!(
			restore_lineage(&successor, revision(3), Some(current.reference())),
			Err(ContextRevisionError::InvalidSupersession)
		);
		assert_eq!(
			restore_lineage(&successor, revision(3), Some(successor.reference())),
			Err(ContextRevisionError::DigestMismatch)
		);
	}

	#[test]
	fn stale_expected_revision_fails_before_any_successor_is_proposed() {
		let project = project(1);
		let current = create(
			1,
			ContextRevisionOwner::project(&project),
			vec![assertion(1, ContextRevisionItemKind::Decision, "Retain exact provenance.")],
		);
		let old_bytes = current.canonical_bytes().to_vec();

		assert_eq!(
			decide_supersede_context_revision(
				&current,
				revision(2),
				vec![assertion(
					1,
					ContextRevisionItemKind::Decision,
					"Retain owner-safe provenance.",
				)],
			),
			Err(ContextRevisionError::RevisionConflict)
		);
		assert_eq!(
			decide_pin_context_item(&current, revision(2), &item_id(1)),
			Err(ContextRevisionError::RevisionConflict)
		);
		assert_eq!(current.canonical_bytes(), old_bytes.as_slice());
		assert_eq!(current.revision().get(), 1);
	}

	#[test]
	fn ordinary_supersession_cannot_bypass_explicit_pin_operations() {
		let project = project(1);
		let owner = ContextRevisionOwner::project(&project);
		let initial = create(
			1,
			owner.clone(),
			vec![assertion(
				1,
				ContextRevisionItemKind::Constraint,
				"Pinned content must remain exact.",
			)],
		);
		let pinned = decide_pin_context_item(&initial, initial.revision(), &item_id(1))
			.unwrap()
			.into_revision();
		let changed_text = item(
			1,
			ContextRevisionItemKind::Constraint,
			"Changed pinned content.",
			ContextRevisionItemProvenance::UserAssertion,
			true,
		);
		let changed_provenance = item(
			1,
			ContextRevisionItemKind::Constraint,
			"Pinned content must remain exact.",
			ContextRevisionItemProvenance::Source(ContextRevisionSource::project(&project)),
			true,
		);
		let changed_kind = item(
			1,
			ContextRevisionItemKind::Decision,
			"Pinned content must remain exact.",
			ContextRevisionItemProvenance::UserAssertion,
			true,
		);
		let cleared_pin =
			assertion(1, ContextRevisionItemKind::Constraint, "Pinned content must remain exact.");
		let new_pinned = item(
			2,
			ContextRevisionItemKind::Fact,
			"New pre-pinned item.",
			ContextRevisionItemProvenance::UserAssertion,
			true,
		);

		for proposed in [
			Vec::new(),
			vec![changed_text],
			vec![changed_provenance],
			vec![changed_kind],
			vec![cleared_pin],
			vec![pinned.items()[0].clone(), new_pinned.clone()],
		] {
			assert_eq!(
				decide_supersede_context_revision(&pinned, pinned.revision(), proposed),
				Err(ContextRevisionError::PinnedItemViolation)
			);
		}

		assert_eq!(
			decide_create_context_revision(None, None, revision_id(2), owner, vec![new_pinned],),
			Err(ContextRevisionError::PinnedItemViolation)
		);

		let successor = decide_supersede_context_revision(
			&pinned,
			pinned.revision(),
			vec![
				pinned.items()[0].clone(),
				assertion(2, ContextRevisionItemKind::Risk, "A new unpinned risk."),
			],
		)
		.unwrap();

		assert!(successor.proposed_revision().items()[0].pinned());
		assert!(pinned.items()[0].pinned());

		let unpinned = decide_unpin_context_item(&pinned, pinned.revision(), &item_id(1))
			.unwrap()
			.into_revision();

		assert!(!unpinned.items()[0].pinned());
		assert!(pinned.items()[0].pinned());
	}

	#[test]
	fn advisor_owner_tag_is_structural_and_rejects_project_scoped_roles() {
		let project = project(1);
		let first = advisor(1);
		let second = advisor(2);
		let first_owner = ContextRevisionOwner::advisor(&first).unwrap();
		let second_owner = ContextRevisionOwner::advisor(&second).unwrap();

		assert_eq!(first_owner.advisor_id(), Some(first.id()));
		assert_eq!(first_owner.project_id(), None);
		assert_eq!(first_owner.program_id(), None);
		assert_ne!(first_owner, second_owner);
		assert_eq!(
			ContextRevisionOwner::advisor(&Agent::lead(
				AgentId::new("23000000-0000-4000-8000-000000000001").unwrap(),
				project.id().clone(),
			)),
			Err(ContextRevisionError::InvalidOwner)
		);

		let value = create(40, first_owner, Vec::new());
		let owner_tag_offset = CONTEXT_REVISION_MAGIC.len() + UUID_BYTES;

		assert_eq!(value.canonical_bytes()[owner_tag_offset], 1);
	}

	#[test]
	fn domain_source_provenance_relationship_matrix_is_closed() {
		let project_a = project(1);
		let project_b = project(2);
		let program_a = program(1, &project_a);
		let program_peer = program(2, &project_a);
		let program_b = program(3, &project_b);
		let owner_project_a = ContextRevisionOwner::project(&project_a);
		let owner_project_b = ContextRevisionOwner::project(&project_b);
		let owner_program_a = ContextRevisionOwner::program(&program_a);
		let owner_program_peer = ContextRevisionOwner::program(&program_peer);
		let owner_program_b = ContextRevisionOwner::program(&program_b);
		let owner_advisor = ContextRevisionOwner::advisor(&advisor(1)).unwrap();
		let source_project_a = ContextRevisionSource::project(&project_a);
		let source_project_b = ContextRevisionSource::project(&project_b);
		let source_program_a = ContextRevisionSource::program(&program_a);
		let source_program_peer = ContextRevisionSource::program(&program_peer);
		let source_program_b = ContextRevisionSource::program(&program_b);
		let source_repository_a = ContextRevisionSource::repository(
			&project_a,
			RepositoryContentRevision::new("repository-a").unwrap(),
		);
		let source_repository_b = ContextRevisionSource::repository(
			&project_b,
			RepositoryContentRevision::new("repository-b").unwrap(),
		);
		let source_policy_a = ContextRevisionSource::policy(program_a.policy_revision_id().clone());
		let source_policy_b = ContextRevisionSource::policy(program_b.policy_revision_id().clone());

		assert_provenance_cases(
			ContextRevisionItemKind::Fact,
			&[
				(&owner_project_a, &source_project_a, true),
				(&owner_program_a, &source_project_a, true),
				(&owner_program_peer, &source_project_a, true),
				(&owner_advisor, &source_project_a, false),
				(&owner_project_b, &source_project_a, false),
				(&owner_program_b, &source_project_a, false),
				(&owner_project_b, &source_project_b, true),
				(&owner_program_b, &source_project_b, true),
				(&owner_project_a, &source_repository_a, true),
				(&owner_program_a, &source_repository_a, true),
				(&owner_program_peer, &source_repository_a, true),
				(&owner_advisor, &source_repository_a, false),
				(&owner_project_b, &source_repository_a, false),
				(&owner_project_b, &source_repository_b, true),
				(&owner_program_b, &source_repository_b, true),
				(&owner_project_a, &source_policy_a, true),
				(&owner_program_a, &source_policy_a, true),
				(&owner_program_peer, &source_policy_a, true),
				(&owner_advisor, &source_policy_a, false),
				(&owner_project_b, &source_policy_a, false),
				(&owner_project_b, &source_policy_b, true),
				(&owner_program_b, &source_policy_b, true),
				(&owner_project_a, &source_program_a, true),
				(&owner_program_a, &source_program_a, true),
				(&owner_program_peer, &source_program_a, false),
				(&owner_advisor, &source_program_a, false),
				(&owner_project_b, &source_program_a, false),
				(&owner_program_b, &source_program_a, false),
				(&owner_project_a, &source_program_peer, true),
				(&owner_program_a, &source_program_peer, false),
				(&owner_program_peer, &source_program_peer, true),
				(&owner_project_b, &source_program_b, true),
				(&owner_program_b, &source_program_b, true),
				(&owner_project_a, &source_program_b, false),
				(&owner_program_a, &source_program_b, false),
			],
		);

		for owner in [&owner_project_a, &owner_program_a, &owner_advisor] {
			assert_eq!(
				provenance_result(
					owner,
					ContextRevisionItemKind::Decision,
					ContextRevisionItemProvenance::UserAssertion,
				),
				Ok(())
			);
		}
	}

	#[test]
	fn context_revision_source_fact_and_handoff_matrix_is_closed() {
		let project_a = project(1);
		let project_b = project(2);
		let program_a = program(1, &project_a);
		let program_peer = program(2, &project_a);
		let program_b = program(3, &project_b);
		let owner_project_a = ContextRevisionOwner::project(&project_a);
		let owner_project_b = ContextRevisionOwner::project(&project_b);
		let owner_program_a = ContextRevisionOwner::program(&program_a);
		let owner_program_peer = ContextRevisionOwner::program(&program_peer);
		let owner_program_b = ContextRevisionOwner::program(&program_b);
		let owner_advisor = ContextRevisionOwner::advisor(&advisor(1)).unwrap();
		let owner_advisor_peer = ContextRevisionOwner::advisor(&advisor(2)).unwrap();
		let context_project_a = context_source(50, &owner_project_a);
		let context_project_b = context_source(51, &owner_project_b);
		let context_program_a = context_source(52, &owner_program_a);
		let context_program_peer = context_source(53, &owner_program_peer);
		let context_program_b = context_source(54, &owner_program_b);
		let context_advisor = context_source(55, &owner_advisor);
		let context_advisor_peer = context_source(56, &owner_advisor_peer);

		assert_provenance_cases(
			ContextRevisionItemKind::Fact,
			&[
				(&owner_project_a, &context_project_a, true),
				(&owner_project_b, &context_project_b, true),
				(&owner_program_a, &context_program_a, true),
				(&owner_program_peer, &context_program_peer, true),
				(&owner_program_b, &context_program_b, true),
				(&owner_advisor, &context_advisor, true),
				(&owner_project_a, &context_project_b, false),
				(&owner_project_a, &context_program_a, false),
				(&owner_program_a, &context_project_a, false),
				(&owner_program_a, &context_program_peer, false),
				(&owner_advisor, &context_project_a, false),
				(&owner_project_a, &context_advisor, false),
				(&owner_advisor, &context_advisor_peer, false),
			],
		);

		assert_provenance_cases(
			ContextRevisionItemKind::Handoff,
			&[
				(&owner_project_a, &context_project_a, true),
				(&owner_program_a, &context_program_a, true),
				(&owner_advisor, &context_advisor, true),
				(&owner_project_a, &context_program_a, true),
				(&owner_project_a, &context_program_peer, true),
				(&owner_program_a, &context_project_a, true),
				(&owner_program_peer, &context_project_a, true),
				(&owner_project_b, &context_program_b, true),
				(&owner_program_b, &context_project_b, true),
				(&owner_advisor, &context_project_a, true),
				(&owner_advisor, &context_project_b, true),
				(&owner_advisor, &context_program_a, true),
				(&owner_advisor, &context_program_b, true),
				(&owner_project_a, &context_advisor, true),
				(&owner_program_a, &context_advisor, true),
				(&owner_project_b, &context_advisor, true),
				(&owner_program_b, &context_advisor, true),
				(&owner_project_a, &context_project_b, false),
				(&owner_project_a, &context_program_b, false),
				(&owner_program_a, &context_project_b, false),
				(&owner_program_a, &context_program_peer, false),
				(&owner_program_a, &context_program_b, false),
				(&owner_program_peer, &context_program_a, false),
				(&owner_project_b, &context_program_a, false),
				(&owner_program_b, &context_project_a, false),
				(&owner_advisor, &context_advisor_peer, false),
				(&owner_advisor_peer, &context_advisor, false),
			],
		);
	}

	#[test]
	fn immediate_lineage_is_exact_and_prior_revision_stays_immutable() {
		let project = project(1);
		let current = create(
			1,
			ContextRevisionOwner::project(&project),
			vec![assertion(1, ContextRevisionItemKind::Decision, "Use immutable revisions.")],
		);
		let current_reference = current.reference();
		let old_bytes = current.canonical_bytes().to_vec();
		let successor = decide_supersede_context_revision(
			&current,
			current.revision(),
			vec![assertion(
				1,
				ContextRevisionItemKind::Decision,
				"Use owner-safe immutable revisions.",
			)],
		)
		.unwrap()
		.into_revision();

		assert_eq!(current.revision().get(), 1);
		assert_eq!(current.canonical_bytes(), old_bytes.as_slice());
		assert_eq!(successor.revision().get(), 2);
		assert_eq!(successor.id(), current.id());
		assert_eq!(successor.owner(), current.owner());
		assert_eq!(successor.supersedes(), Some(&current_reference));
		assert_ne!(successor.canonical_bytes(), current.canonical_bytes());
	}
}
