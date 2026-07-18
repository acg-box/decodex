//! GitHub pull-request and check-run effects behind persistence-issued dispatch receipts.
//!
//! This module is crate-private to the later composition owner. It never discovers repository or
//! account authority from the process, filesystem, Git, or a remote URL. Dispatch receipts are
//! affine, have no constructor in this slice, and must eventually be minted here only from the
//! accepted persistence/control path. Readback continuations retain the original local authority
//! and desired fields; provider responses add consistency constraints but never become authority.

use std::{
	collections::{BTreeMap, BTreeSet},
	error::Error,
	fmt::{Debug, Display, Formatter},
};

use decodex_core::contains_credential_material;

const MAX_PAGES: usize = 256;
const MAX_OBJECTS_PER_PAGE: usize = 100;
const MAX_OPAQUE_BYTES: usize = 512;
const MAX_IDENTITY_BYTES: usize = 256;
const MAX_PULL_REQUEST_TITLE_BYTES: usize = 256;
const MAX_PULL_REQUEST_BODY_BYTES: usize = 65_536;
const MAX_REQUIRED_CHECK_RUNS: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GitHubContractError {
	InvalidIdentity,
	InvalidRepository,
	InvalidBranch,
	InvalidRevision,
	InvalidMarker,
	InvalidText,
	CredentialRejected,
	InvalidPagination,
	InvalidCheckContract,
	UnknownProviderValue,
	ImpossibleProviderState,
}
impl Display for GitHubContractError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(match self {
			Self::InvalidIdentity => "invalid GitHub identity",
			Self::InvalidRepository => "invalid GitHub repository identity",
			Self::InvalidBranch => "invalid GitHub branch identity",
			Self::InvalidRevision => "invalid GitHub revision",
			Self::InvalidMarker => "invalid GitHub durable marker",
			Self::InvalidText => "invalid bounded GitHub public text",
			Self::CredentialRejected => "credential-bearing GitHub value rejected",
			Self::InvalidPagination => "invalid GitHub pagination metadata",
			Self::InvalidCheckContract => "invalid GitHub required-check contract",
			Self::UnknownProviderValue => "unknown GitHub provider value",
			Self::ImpossibleProviderState => "impossible GitHub provider state",
		})
	}
}
impl Error for GitHubContractError {}

macro_rules! positive_id {
	($name:ident) => {
		#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
		pub(crate) struct $name(u64);
		impl $name {
			pub(crate) fn new(value: u64) -> Result<Self, GitHubContractError> {
				if value == 0 { Err(GitHubContractError::InvalidIdentity) } else { Ok(Self(value)) }
			}
			pub(crate) const fn get(self) -> u64 { self.0 }
		}
	};
}

positive_id!(GitHubRepositoryId);
positive_id!(GitHubInstallationId);
positive_id!(GitHubAccountId);
positive_id!(GitHubPullRequestId);
positive_id!(GitHubCheckSuiteId);
positive_id!(GitHubCheckRunId);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) enum GitHubProviderIdentity {
	GitHubDotCom,
}

macro_rules! bounded_identity {
	($name:ident, $validator:ident, $error:ident) => {
		#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
		pub(crate) struct $name(String);
		impl $name {
			pub(crate) fn new(value: impl Into<String>) -> Result<Self, GitHubContractError> {
				let value = value.into();
				if !$validator(&value) { return Err(GitHubContractError::$error); }
				Ok(Self(value))
			}
			pub(crate) fn value(&self) -> &str { &self.0 }
		}
	};
}

fn valid_owner(value: &str) -> bool {
	!value.is_empty()
		&& value.len() <= 39
		&& value.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
		&& value.bytes().next().is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
		&& value.bytes().next_back().is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
		&& !value.contains("--")
}

fn valid_repository(value: &str) -> bool {
	!value.is_empty()
		&& value.len() <= MAX_IDENTITY_BYTES
		&& !matches!(value, "." | "..")
		&& value.bytes().all(|byte| {
			byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_' | b'.')
		})
}

fn valid_branch(value: &str) -> bool {
	!value.is_empty()
		&& value.len() <= MAX_IDENTITY_BYTES
		&& value.trim() == value
		&& !value.starts_with('/')
		&& !value.ends_with('/')
		&& !value.ends_with('.')
		&& !value.contains("..")
		&& !value.contains("@{")
		&& !value.contains("//")
		&& !value
			.bytes()
			.any(|byte| byte <= b' ' || matches!(byte, b'~' | b'^' | b':' | b'?' | b'*' | b'[' | b'\\'))
}

fn valid_revision(value: &str) -> bool {
	value.len() == 40
		&& value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

bounded_identity!(GitHubRepositoryOwner, valid_owner, InvalidRepository);
bounded_identity!(GitHubRepositoryName, valid_repository, InvalidRepository);
bounded_identity!(GitHubBranchName, valid_branch, InvalidBranch);
bounded_identity!(GitHubRevision, valid_revision, InvalidRevision);

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GitHubRepositoryBinding {
	provider: GitHubProviderIdentity,
	repository_id: GitHubRepositoryId,
	owner: GitHubRepositoryOwner,
	name: GitHubRepositoryName,
	installation_id: GitHubInstallationId,
	account_id: GitHubAccountId,
}
impl GitHubRepositoryBinding {
	pub(crate) fn new(
		provider: GitHubProviderIdentity,
		repository_id: GitHubRepositoryId,
		owner: GitHubRepositoryOwner,
		name: GitHubRepositoryName,
		installation_id: GitHubInstallationId,
		account_id: GitHubAccountId,
	) -> Self {
		Self { provider, repository_id, owner, name, installation_id, account_id }
	}
	pub(crate) const fn provider(&self) -> GitHubProviderIdentity { self.provider }
	pub(crate) const fn repository_id(&self) -> GitHubRepositoryId { self.repository_id }
	pub(crate) fn owner(&self) -> &str { self.owner.value() }
	pub(crate) fn name(&self) -> &str { self.name.value() }
	pub(crate) const fn installation_id(&self) -> GitHubInstallationId { self.installation_id }
	pub(crate) const fn account_id(&self) -> GitHubAccountId { self.account_id }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GitHubRevisionAuthority {
	base_branch: GitHubBranchName,
	base_revision: GitHubRevision,
	head_branch: GitHubBranchName,
	head_revision: GitHubRevision,
}
impl GitHubRevisionAuthority {
	pub(crate) fn new(
		base_branch: GitHubBranchName,
		base_revision: GitHubRevision,
		head_branch: GitHubBranchName,
		head_revision: GitHubRevision,
	) -> Result<Self, GitHubContractError> {
		if base_branch == head_branch { return Err(GitHubContractError::InvalidBranch); }
		Ok(Self { base_branch, base_revision, head_branch, head_revision })
	}
	pub(crate) fn base_branch(&self) -> &str { self.base_branch.value() }
	pub(crate) fn base_revision(&self) -> &str { self.base_revision.value() }
	pub(crate) fn head_branch(&self) -> &str { self.head_branch.value() }
	pub(crate) fn head_revision(&self) -> &str { self.head_revision.value() }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct GitHubOperationMarker(String);
impl GitHubOperationMarker {
	pub(crate) fn new(value: impl Into<String>) -> Result<Self, GitHubContractError> {
		let value = value.into();
		let Some(uuid) = value.strip_prefix("decodex/github-effect/1/") else {
			return Err(GitHubContractError::InvalidMarker);
		};
		if !is_canonical_uuid_v4(uuid) { return Err(GitHubContractError::InvalidMarker); }
		Ok(Self(value))
	}
	pub(crate) fn value(&self) -> &str { &self.0 }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct GitHubPullRequestIdentity {
	id: GitHubPullRequestId,
	number: u64,
}
impl GitHubPullRequestIdentity {
	pub(crate) fn new(id: GitHubPullRequestId, number: u64) -> Result<Self, GitHubContractError> {
		if number == 0 { return Err(GitHubContractError::InvalidIdentity); }
		Ok(Self { id, number })
	}
	pub(crate) const fn id(self) -> GitHubPullRequestId { self.id }
	pub(crate) const fn number(self) -> u64 { self.number }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GitHubPullRequestTarget {
	Unassigned,
	Exact(GitHubPullRequestIdentity),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct GitHubCheckIdentity {
	suite_id: GitHubCheckSuiteId,
	run_id: GitHubCheckRunId,
}
impl GitHubCheckIdentity {
	pub(crate) const fn new(suite_id: GitHubCheckSuiteId, run_id: GitHubCheckRunId) -> Self {
		Self { suite_id, run_id }
	}
	pub(crate) const fn suite_id(self) -> GitHubCheckSuiteId { self.suite_id }
	pub(crate) const fn run_id(self) -> GitHubCheckRunId { self.run_id }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GitHubCheckTarget {
	Unassigned,
	InSuite(GitHubCheckSuiteId),
	Exact(GitHubCheckIdentity),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GitHubPullRequestAuthority {
	repository: GitHubRepositoryBinding,
	revisions: GitHubRevisionAuthority,
	target: GitHubPullRequestTarget,
	marker: GitHubOperationMarker,
}
impl GitHubPullRequestAuthority {
	pub(crate) fn new(
		repository: GitHubRepositoryBinding,
		revisions: GitHubRevisionAuthority,
		target: GitHubPullRequestTarget,
		marker: GitHubOperationMarker,
	) -> Self {
		Self { repository, revisions, target, marker }
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GitHubCheckAuthority {
	repository: GitHubRepositoryBinding,
	revisions: GitHubRevisionAuthority,
	pull_request: GitHubPullRequestIdentity,
	target: GitHubCheckTarget,
	marker: GitHubOperationMarker,
}
impl GitHubCheckAuthority {
	pub(crate) fn new(
		repository: GitHubRepositoryBinding,
		revisions: GitHubRevisionAuthority,
		pull_request: GitHubPullRequestIdentity,
		target: GitHubCheckTarget,
		marker: GitHubOperationMarker,
	) -> Self {
		Self { repository, revisions, pull_request, target, marker }
	}
}

#[derive(Clone, Eq, PartialEq)]
pub(crate) struct GitHubPublicText(String);
impl GitHubPublicText {
	pub(crate) fn new(value: impl Into<String>, maximum: usize) -> Result<Self, GitHubContractError> {
		let value = value.into();
		if value.is_empty() || value.len() > maximum || value.chars().any(char::is_control) {
			return Err(GitHubContractError::InvalidText);
		}
		if contains_credential_material(&value) { return Err(GitHubContractError::CredentialRejected); }
		Ok(Self(value))
	}
	pub(crate) fn value(&self) -> &str { &self.0 }
}
impl Debug for GitHubPublicText {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str("GitHubPublicText(<redacted>)")
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GitHubPullRequestSpec {
	title: GitHubPublicText,
	body: GitHubPublicText,
	draft: bool,
}
impl GitHubPullRequestSpec {
	pub(crate) fn new(
		title: impl Into<String>,
		body: impl Into<String>,
		draft: bool,
	) -> Result<Self, GitHubContractError> {
		Ok(Self {
			title: GitHubPublicText::new(title, MAX_PULL_REQUEST_TITLE_BYTES)?,
			body: GitHubPublicText::new(body, MAX_PULL_REQUEST_BODY_BYTES)?,
			draft,
		})
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GitHubCheckStatus {
	Queued,
	InProgress,
	Completed,
	Waiting,
	Requested,
	Pending,
}
impl GitHubCheckStatus {
	pub(crate) fn from_provider(value: &str) -> Result<Self, GitHubContractError> {
		match value {
			"queued" => Ok(Self::Queued),
			"in_progress" => Ok(Self::InProgress),
			"completed" => Ok(Self::Completed),
			"waiting" => Ok(Self::Waiting),
			"requested" => Ok(Self::Requested),
			"pending" => Ok(Self::Pending),
			_ => Err(GitHubContractError::UnknownProviderValue),
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GitHubCheckConclusion {
	Neutral,
	Skipped,
	Cancelled,
	TimedOut,
	ActionRequired,
	Failure,
	Success,
	Stale,
	StartupFailure,
}
impl GitHubCheckConclusion {
	pub(crate) fn from_provider(value: &str) -> Result<Self, GitHubContractError> {
		match value {
			"neutral" => Ok(Self::Neutral),
			"skipped" => Ok(Self::Skipped),
			"cancelled" => Ok(Self::Cancelled),
			"timed_out" => Ok(Self::TimedOut),
			"action_required" => Ok(Self::ActionRequired),
			"failure" => Ok(Self::Failure),
			"success" => Ok(Self::Success),
			"stale" => Ok(Self::Stale),
			"startup_failure" => Ok(Self::StartupFailure),
			_ => Err(GitHubContractError::UnknownProviderValue),
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct GitHubCheckState {
	status: GitHubCheckStatus,
	conclusion: Option<GitHubCheckConclusion>,
}
impl GitHubCheckState {
	pub(crate) fn new(
		status: GitHubCheckStatus,
		conclusion: Option<GitHubCheckConclusion>,
	) -> Result<Self, GitHubContractError> {
		if matches!(status, GitHubCheckStatus::Completed) != conclusion.is_some() {
			return Err(GitHubContractError::ImpossibleProviderState);
		}
		Ok(Self { status, conclusion })
	}
	pub(crate) fn from_provider(
		status: &str,
		conclusion: Option<&str>,
	) -> Result<Self, GitHubContractError> {
		Self::new(
			GitHubCheckStatus::from_provider(status)?,
			conclusion.map(GitHubCheckConclusion::from_provider).transpose()?,
		)
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GitHubCheckSpec {
	name: GitHubPublicText,
	state: GitHubCheckState,
}
impl GitHubCheckSpec {
	pub(crate) fn new(
		name: impl Into<String>,
		state: GitHubCheckState,
	) -> Result<Self, GitHubContractError> {
		Ok(Self { name: GitHubPublicText::new(name, MAX_IDENTITY_BYTES)?, state })
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GitHubRequiredCheckRun {
	name: GitHubPublicText,
	run_id: Option<GitHubCheckRunId>,
}
impl GitHubRequiredCheckRun {
	pub(crate) fn new(
		name: impl Into<String>,
		run_id: Option<GitHubCheckRunId>,
	) -> Result<Self, GitHubContractError> {
		Ok(Self { name: GitHubPublicText::new(name, MAX_IDENTITY_BYTES)?, run_id })
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GitHubCheckSuiteContract {
	required_runs: Vec<GitHubRequiredCheckRun>,
}
impl GitHubCheckSuiteContract {
	pub(crate) fn new(required_runs: Vec<GitHubRequiredCheckRun>) -> Result<Self, GitHubContractError> {
		if required_runs.is_empty() || required_runs.len() > MAX_REQUIRED_CHECK_RUNS {
			return Err(GitHubContractError::InvalidCheckContract);
		}
		let mut prior_name = None;
		let mut run_ids = BTreeSet::new();
		for required in &required_runs {
			if prior_name.is_some_and(|prior: &str| prior >= required.name.value())
				|| required.run_id.is_some_and(|id| !run_ids.insert(id))
			{
				return Err(GitHubContractError::InvalidCheckContract);
			}
			prior_name = Some(required.name.value());
		}
		Ok(Self { required_runs })
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GitHubPullRequestState {
	Open,
	Closed,
	Merged,
}
impl GitHubPullRequestState {
	pub(crate) fn from_provider(value: &str) -> Result<Self, GitHubContractError> {
		match value {
			"open" => Ok(Self::Open),
			"closed" => Ok(Self::Closed),
			"merged" => Ok(Self::Merged),
			_ => Err(GitHubContractError::UnknownProviderValue),
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum GitHubProviderField<T> {
	Visible(T),
	Redacted,
}

#[derive(Clone, Eq, PartialEq)]
pub(crate) struct GitHubPullRequestObservation {
	identity: GitHubPullRequestIdentity,
	revisions: GitHubRevisionAuthority,
	marker: GitHubProviderField<Option<GitHubOperationMarker>>,
	state: GitHubPullRequestState,
	spec: GitHubProviderField<GitHubPullRequestSpec>,
}
impl GitHubPullRequestObservation {
	pub(crate) fn new(
		identity: GitHubPullRequestIdentity,
		revisions: GitHubRevisionAuthority,
		marker: GitHubProviderField<Option<GitHubOperationMarker>>,
		state: GitHubPullRequestState,
		spec: GitHubProviderField<GitHubPullRequestSpec>,
	) -> Self {
		Self { identity, revisions, marker, state, spec }
	}
}
impl Debug for GitHubPullRequestObservation {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("GitHubPullRequestObservation")
			.field("identity", &self.identity)
			.field("revisions", &self.revisions)
			.field("marker", &self.marker)
			.field("state", &self.state)
			.field("spec", &"<redacted>")
			.finish()
	}
}

#[derive(Clone, Eq, PartialEq)]
pub(crate) struct GitHubCheckObservation {
	identity: GitHubCheckIdentity,
	pull_request: GitHubPullRequestIdentity,
	revisions: GitHubRevisionAuthority,
	marker: GitHubProviderField<Option<GitHubOperationMarker>>,
	spec: GitHubProviderField<GitHubCheckSpec>,
}
impl GitHubCheckObservation {
	pub(crate) fn new(
		identity: GitHubCheckIdentity,
		pull_request: GitHubPullRequestIdentity,
		revisions: GitHubRevisionAuthority,
		marker: GitHubProviderField<Option<GitHubOperationMarker>>,
		spec: GitHubProviderField<GitHubCheckSpec>,
	) -> Self {
		Self { identity, pull_request, revisions, marker, spec }
	}
}
impl Debug for GitHubCheckObservation {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("GitHubCheckObservation")
			.field("identity", &self.identity)
			.field("pull_request", &self.pull_request)
			.field("revisions", &self.revisions)
			.field("marker", &self.marker)
			.field("spec", &"<redacted>")
			.finish()
	}
}

#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct GitHubCursor(String);
impl GitHubCursor {
	pub(crate) fn new(value: impl Into<String>) -> Result<Self, GitHubContractError> {
		Self::validated(value.into()).map(Self)
	}
	pub(crate) fn provider_value(&self) -> &str { &self.0 }
	fn validated(value: String) -> Result<String, GitHubContractError> {
		if !valid_opaque(&value) { return Err(GitHubContractError::InvalidPagination); }
		Ok(value)
	}
}
impl Debug for GitHubCursor {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str("GitHubCursor(<redacted>)")
	}
}

#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct GitHubSnapshot(String);
impl GitHubSnapshot {
	pub(crate) fn new(value: impl Into<String>) -> Result<Self, GitHubContractError> {
		let value = value.into();
		if !valid_opaque(&value) { return Err(GitHubContractError::InvalidPagination); }
		Ok(Self(value))
	}
	pub(crate) fn provider_value(&self) -> &str { &self.0 }
}
impl Debug for GitHubSnapshot {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str("GitHubSnapshot(<redacted>)")
	}
}

#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct GitHubPageIdentity(String);
impl GitHubPageIdentity {
	pub(crate) fn new(value: impl Into<String>) -> Result<Self, GitHubContractError> {
		let value = value.into();
		if !valid_opaque(&value) { return Err(GitHubContractError::InvalidPagination); }
		Ok(Self(value))
	}
}
impl Debug for GitHubPageIdentity {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str("GitHubPageIdentity(<redacted>)")
	}
}

fn valid_opaque(value: &str) -> bool {
	!value.is_empty()
		&& value.len() <= MAX_OPAQUE_BYTES
		&& value.trim() == value
		&& !value.chars().any(char::is_control)
		&& !contains_credential_material(value)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GitHubPaginationMetadata {
	snapshot: GitHubSnapshot,
	page_identity: GitHubPageIdentity,
	page_number: u16,
	requested_cursor: Option<GitHubCursor>,
	has_next_page: bool,
	next_cursor: Option<GitHubCursor>,
}
impl GitHubPaginationMetadata {
	pub(crate) fn new(
		snapshot: GitHubSnapshot,
		page_identity: GitHubPageIdentity,
		page_number: u16,
		requested_cursor: Option<GitHubCursor>,
		has_next_page: bool,
		next_cursor: Option<GitHubCursor>,
	) -> Result<Self, GitHubContractError> {
		if page_number == 0 { return Err(GitHubContractError::InvalidPagination); }
		Ok(Self {
			snapshot,
			page_identity,
			page_number,
			requested_cursor,
			has_next_page,
			next_cursor,
		})
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GitHubPage<T> {
	repository: GitHubRepositoryBinding,
	pagination: GitHubPaginationMetadata,
	objects: Vec<T>,
}
impl<T> GitHubPage<T> {
	pub(crate) fn new(
		repository: GitHubRepositoryBinding,
		pagination: GitHubPaginationMetadata,
		objects: Vec<T>,
	) -> Result<Self, GitHubContractError> {
		if objects.len() > MAX_OBJECTS_PER_PAGE {
			return Err(GitHubContractError::InvalidPagination);
		}
		Ok(Self { repository, pagination, objects })
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GitHubReadFailure {
	TemporarilyUnavailable,
	Unauthorized,
	Forbidden,
	NotFound,
	RateLimited,
	ProviderRedacted,
	IncompleteChecks,
	MalformedResponse,
	UnknownProviderValue,
	ImpossibleProviderState,
}

pub(crate) struct GitHubPullRequestMutation<'a> {
	authority: &'a GitHubPullRequestAuthority,
	spec: &'a GitHubPullRequestSpec,
}
impl<'a> GitHubPullRequestMutation<'a> {
	pub(crate) fn authority(&self) -> &GitHubPullRequestAuthority { self.authority }
	pub(crate) fn title(&self) -> &str { self.spec.title.value() }
	pub(crate) fn body(&self) -> &str { self.spec.body.value() }
	pub(crate) const fn draft(&self) -> bool { self.spec.draft }
}

pub(crate) struct GitHubCheckMutation<'a> {
	authority: &'a GitHubCheckAuthority,
	spec: &'a GitHubCheckSpec,
}
impl<'a> GitHubCheckMutation<'a> {
	pub(crate) fn authority(&self) -> &GitHubCheckAuthority { self.authority }
	pub(crate) fn name(&self) -> &str { self.spec.name.value() }
	pub(crate) const fn state(&self) -> GitHubCheckState { self.spec.state }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GitHubPullRequestMutationOutcome {
	Accepted(GitHubPullRequestIdentity),
	LostResponse,
	DuplicateOrAlreadyExists,
	DefinitelyNotSent,
	StaleBase,
	StaleHead,
	ConflictingMarker,
	ProviderRedacted,
	ImpossibleState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GitHubCheckMutationOutcome {
	Accepted(GitHubCheckIdentity),
	LostResponse,
	DuplicateOrAlreadyExists,
	DefinitelyNotSent,
	StaleBase,
	StaleHead,
	ConflictingMarker,
	ProviderRedacted,
	ImpossibleState,
}

pub(crate) trait GitHubEffectProvider {
	fn begin_pull_request_snapshot(
		&self,
		authority: &GitHubPullRequestAuthority,
	) -> Result<GitHubSnapshot, GitHubReadFailure>;
	fn pull_request_page(
		&self,
		authority: &GitHubPullRequestAuthority,
		snapshot: &GitHubSnapshot,
		cursor: Option<&GitHubCursor>,
	) -> Result<GitHubPage<GitHubPullRequestObservation>, GitHubReadFailure>;
	fn end_pull_request_snapshot(
		&self,
		authority: &GitHubPullRequestAuthority,
		start: &GitHubSnapshot,
	) -> Result<GitHubSnapshot, GitHubReadFailure>;
	fn apply_pull_request(
		&self,
		mutation: GitHubPullRequestMutation<'_>,
	) -> GitHubPullRequestMutationOutcome;

	fn begin_check_snapshot(
		&self,
		authority: &GitHubCheckAuthority,
	) -> Result<GitHubSnapshot, GitHubReadFailure>;
	fn check_page(
		&self,
		authority: &GitHubCheckAuthority,
		snapshot: &GitHubSnapshot,
		cursor: Option<&GitHubCursor>,
	) -> Result<GitHubPage<GitHubCheckObservation>, GitHubReadFailure>;
	fn end_check_snapshot(
		&self,
		authority: &GitHubCheckAuthority,
		start: &GitHubSnapshot,
	) -> Result<GitHubSnapshot, GitHubReadFailure>;
	fn apply_check(&self, mutation: GitHubCheckMutation<'_>) -> GitHubCheckMutationOutcome;
}

/// Affine authority issued only after accepted persistence acknowledges a fresh dispatch.
/// There is intentionally no constructor in this provider slice.
pub(crate) struct GitHubPullRequestDispatchReceipt {
	authority: GitHubPullRequestAuthority,
	spec: GitHubPullRequestSpec,
}
impl Debug for GitHubPullRequestDispatchReceipt {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str("GitHubPullRequestDispatchReceipt(<affine>)")
	}
}

/// Affine authority issued only after accepted persistence acknowledges a fresh dispatch.
/// There is intentionally no constructor in this provider slice.
pub(crate) struct GitHubCheckDispatchReceipt {
	authority: GitHubCheckAuthority,
	spec: GitHubCheckSpec,
	suite_contract: GitHubCheckSuiteContract,
}
impl Debug for GitHubCheckDispatchReceipt {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str("GitHubCheckDispatchReceipt(<affine>)")
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GitHubReadbackReason {
	AcceptedNeedsVerification,
	LostResponse,
	DuplicateOrAlreadyExists,
	TemporarilyUnavailable,
}

pub(crate) struct GitHubPullRequestContinuation {
	authority: GitHubPullRequestAuthority,
	spec: GitHubPullRequestSpec,
	response_identity: Option<GitHubPullRequestIdentity>,
	reason: GitHubReadbackReason,
}
impl Debug for GitHubPullRequestContinuation {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("GitHubPullRequestContinuation")
			.field("authority", &self.authority)
			.field("spec", &"<redacted>")
			.field("response_identity", &self.response_identity)
			.field("reason", &self.reason)
			.finish()
	}
}

pub(crate) struct GitHubCheckContinuation {
	authority: GitHubCheckAuthority,
	spec: GitHubCheckSpec,
	suite_contract: GitHubCheckSuiteContract,
	response_identity: Option<GitHubCheckIdentity>,
	reason: GitHubReadbackReason,
}
impl Debug for GitHubCheckContinuation {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("GitHubCheckContinuation")
			.field("authority", &self.authority)
			.field("spec", &"<redacted>")
			.field("suite_contract", &"<bounded>")
			.field("response_identity", &self.response_identity)
			.field("reason", &self.reason)
			.finish()
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct GitHubObservationSummary {
	pages: u16,
	objects: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GitHubNoEffectReason {
	CompletelyObservedAbsent,
	ProviderProvedRequestNotSent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GitHubStaleReason {
	BaseRevisionChanged,
	HeadRevisionChanged,
	BaseAndHeadChanged,
	PullRequestNotOpen,
	PullRequestIdentityChanged,
	CheckIdentityChanged,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GitHubAmbiguity {
	MissingNextCursor,
	UnexpectedNextCursor,
	CursorCycle,
	PageCycle,
	RepeatedPage,
	PaginationLimit,
	PageSnapshotChanged,
	EndSnapshotChanged,
	RepositoryIdentityChanged,
	DuplicateObjectIdentity,
	DurableMarkerConflict,
	ProviderResponseIdentityConflict,
	ProviderRedacted,
	ExternallyChangedFields,
	IncompleteChecks,
	ConflictingCheckResults,
	ImpossibleProviderState,
	UnknownProviderValue,
	Unauthorized,
	Forbidden,
	ProviderObjectNotFound,
	MalformedProviderResponse,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GitHubPullRequestCompletion {
	observation: GitHubPullRequestObservation,
	summary: GitHubObservationSummary,
}
impl GitHubPullRequestCompletion {
	pub(crate) fn observation(&self) -> &GitHubPullRequestObservation { &self.observation }
	pub(crate) const fn summary(&self) -> GitHubObservationSummary { self.summary }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GitHubCheckCompletion {
	observation: GitHubCheckObservation,
	required_runs: Vec<GitHubCheckObservation>,
	summary: GitHubObservationSummary,
}
impl GitHubCheckCompletion {
	pub(crate) fn observation(&self) -> &GitHubCheckObservation { &self.observation }
	pub(crate) fn required_runs(&self) -> &[GitHubCheckObservation] { &self.required_runs }
	pub(crate) const fn summary(&self) -> GitHubObservationSummary { self.summary }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct GitHubNoEffect {
	reason: GitHubNoEffectReason,
	summary: Option<GitHubObservationSummary>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct GitHubStale {
	reason: GitHubStaleReason,
	summary: Option<GitHubObservationSummary>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct GitHubTerminalAmbiguity {
	reason: GitHubAmbiguity,
	summary: Option<GitHubObservationSummary>,
}

pub(crate) enum GitHubPullRequestDispatchResolution {
	Deferred(GitHubPullRequestDispatchReceipt),
	ReadbackRequired(GitHubPullRequestContinuation),
	Completed(GitHubPullRequestCompletion),
	NoEffect(GitHubNoEffect),
	Stale(GitHubStale),
	Ambiguous(GitHubTerminalAmbiguity),
}

pub(crate) enum GitHubPullRequestReadbackResolution {
	ReadbackRequired(GitHubPullRequestContinuation),
	Completed(GitHubPullRequestCompletion),
	NoEffect(GitHubNoEffect),
	Stale(GitHubStale),
	Ambiguous(GitHubTerminalAmbiguity),
}

pub(crate) enum GitHubCheckDispatchResolution {
	Deferred(GitHubCheckDispatchReceipt),
	ReadbackRequired(GitHubCheckContinuation),
	Completed(GitHubCheckCompletion),
	NoEffect(GitHubNoEffect),
	Stale(GitHubStale),
	Ambiguous(GitHubTerminalAmbiguity),
}

pub(crate) enum GitHubCheckReadbackResolution {
	ReadbackRequired(GitHubCheckContinuation),
	Completed(GitHubCheckCompletion),
	NoEffect(GitHubNoEffect),
	Stale(GitHubStale),
	Ambiguous(GitHubTerminalAmbiguity),
}

#[derive(Debug)]
struct CompleteInventory<T> {
	objects: Vec<T>,
	summary: GitHubObservationSummary,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CollectionFailure {
	Read(GitHubReadFailure),
	Ambiguous(GitHubAmbiguity),
}

pub(crate) fn reconcile_pull_request_dispatch<P: GitHubEffectProvider + ?Sized>(
	provider: &P,
	receipt: GitHubPullRequestDispatchReceipt,
) -> GitHubPullRequestDispatchResolution {
	let GitHubPullRequestDispatchReceipt { authority, spec } = receipt;
	let inventory = match collect_pull_requests(provider, &authority) {
		Ok(inventory) => inventory,
		Err(CollectionFailure::Read(
			GitHubReadFailure::TemporarilyUnavailable | GitHubReadFailure::RateLimited,
		)) => {
			return GitHubPullRequestDispatchResolution::Deferred(
				GitHubPullRequestDispatchReceipt { authority, spec },
			);
		},
		Err(failure) => return GitHubPullRequestDispatchResolution::Ambiguous(collection_ambiguity(failure)),
	};
	match reconcile_pull_request_present(&authority, &spec, None, inventory) {
		PullRequestPresence::Terminal(terminal) => return terminal.into_dispatch(),
		PullRequestPresence::Absent(_) => {},
	}

	let outcome = provider.apply_pull_request(GitHubPullRequestMutation { authority: &authority, spec: &spec });
	after_pull_request_mutation(authority, spec, outcome)
}

pub(crate) fn reconcile_pull_request_readback<P: GitHubEffectProvider + ?Sized>(
	provider: &P,
	continuation: GitHubPullRequestContinuation,
) -> GitHubPullRequestReadbackResolution {
	let GitHubPullRequestContinuation { authority, spec, response_identity, reason } = continuation;
	let inventory = match collect_pull_requests(provider, &authority) {
		Ok(inventory) => inventory,
		Err(CollectionFailure::Read(
			GitHubReadFailure::TemporarilyUnavailable | GitHubReadFailure::RateLimited,
		)) => {
			return GitHubPullRequestReadbackResolution::ReadbackRequired(
				GitHubPullRequestContinuation { authority, spec, response_identity, reason },
			);
		},
		Err(failure) => return GitHubPullRequestReadbackResolution::Ambiguous(collection_ambiguity(failure)),
	};
	match reconcile_pull_request_present(&authority, &spec, response_identity, inventory) {
		PullRequestPresence::Terminal(terminal) => terminal,
		PullRequestPresence::Absent(summary) => PullRequestTerminal::NoEffect(GitHubNoEffect {
			reason: GitHubNoEffectReason::CompletelyObservedAbsent,
			summary: Some(summary),
		}),
	}
	.into_readback()
}

pub(crate) fn reconcile_check_dispatch<P: GitHubEffectProvider + ?Sized>(
	provider: &P,
	receipt: GitHubCheckDispatchReceipt,
) -> GitHubCheckDispatchResolution {
	let GitHubCheckDispatchReceipt { authority, spec, suite_contract } = receipt;
	let inventory = match collect_checks(provider, &authority) {
		Ok(inventory) => inventory,
		Err(CollectionFailure::Read(
			GitHubReadFailure::TemporarilyUnavailable | GitHubReadFailure::RateLimited,
		)) => {
			return GitHubCheckDispatchResolution::Deferred(GitHubCheckDispatchReceipt {
				authority,
				spec,
				suite_contract,
			});
		},
		Err(failure) => return GitHubCheckDispatchResolution::Ambiguous(collection_ambiguity(failure)),
	};
	match reconcile_check_present(
		&authority,
		&spec,
		&suite_contract,
		None,
		inventory,
	) {
		CheckPresence::Terminal(terminal) => return terminal.into_dispatch(),
		CheckPresence::Absent(_) => {},
	}

	let outcome = provider.apply_check(GitHubCheckMutation { authority: &authority, spec: &spec });
	after_check_mutation(authority, spec, suite_contract, outcome)
}

pub(crate) fn reconcile_check_readback<P: GitHubEffectProvider + ?Sized>(
	provider: &P,
	continuation: GitHubCheckContinuation,
) -> GitHubCheckReadbackResolution {
	let GitHubCheckContinuation {
		authority,
		spec,
		suite_contract,
		response_identity,
		reason,
	} = continuation;
	let inventory = match collect_checks(provider, &authority) {
		Ok(inventory) => inventory,
		Err(CollectionFailure::Read(
			GitHubReadFailure::TemporarilyUnavailable | GitHubReadFailure::RateLimited,
		)) => {
			return GitHubCheckReadbackResolution::ReadbackRequired(GitHubCheckContinuation {
				authority,
				spec,
				suite_contract,
				response_identity,
				reason,
			});
		},
		Err(failure) => return GitHubCheckReadbackResolution::Ambiguous(collection_ambiguity(failure)),
	};
	match reconcile_check_present(
		&authority,
		&spec,
		&suite_contract,
		response_identity,
		inventory,
	) {
		CheckPresence::Terminal(terminal) => terminal,
		CheckPresence::Absent(summary) => CheckTerminal::NoEffect(GitHubNoEffect {
			reason: GitHubNoEffectReason::CompletelyObservedAbsent,
			summary: Some(summary),
		}),
	}
	.into_readback()
}

fn after_pull_request_mutation(
	authority: GitHubPullRequestAuthority,
	spec: GitHubPullRequestSpec,
	outcome: GitHubPullRequestMutationOutcome,
) -> GitHubPullRequestDispatchResolution {
	match outcome {
		GitHubPullRequestMutationOutcome::Accepted(identity) => {
			if !pull_request_response_matches(authority.target, identity) {
				return GitHubPullRequestDispatchResolution::Ambiguous(ambiguity(
					GitHubAmbiguity::ProviderResponseIdentityConflict,
					None,
				));
			}
			GitHubPullRequestDispatchResolution::ReadbackRequired(GitHubPullRequestContinuation {
				authority,
				spec,
				response_identity: Some(identity),
				reason: GitHubReadbackReason::AcceptedNeedsVerification,
			})
		},
		GitHubPullRequestMutationOutcome::LostResponse =>
			GitHubPullRequestDispatchResolution::ReadbackRequired(GitHubPullRequestContinuation {
				authority,
				spec,
				response_identity: None,
				reason: GitHubReadbackReason::LostResponse,
			}),
		GitHubPullRequestMutationOutcome::DuplicateOrAlreadyExists =>
			GitHubPullRequestDispatchResolution::ReadbackRequired(GitHubPullRequestContinuation {
				authority,
				spec,
				response_identity: None,
				reason: GitHubReadbackReason::DuplicateOrAlreadyExists,
			}),
		GitHubPullRequestMutationOutcome::DefinitelyNotSent =>
			GitHubPullRequestDispatchResolution::NoEffect(GitHubNoEffect {
				reason: GitHubNoEffectReason::ProviderProvedRequestNotSent,
				summary: None,
			}),
		GitHubPullRequestMutationOutcome::StaleBase =>
			GitHubPullRequestDispatchResolution::Stale(stale(GitHubStaleReason::BaseRevisionChanged, None)),
		GitHubPullRequestMutationOutcome::StaleHead =>
			GitHubPullRequestDispatchResolution::Stale(stale(GitHubStaleReason::HeadRevisionChanged, None)),
		GitHubPullRequestMutationOutcome::ConflictingMarker =>
			GitHubPullRequestDispatchResolution::Ambiguous(ambiguity(GitHubAmbiguity::DurableMarkerConflict, None)),
		GitHubPullRequestMutationOutcome::ProviderRedacted =>
			GitHubPullRequestDispatchResolution::Ambiguous(ambiguity(GitHubAmbiguity::ProviderRedacted, None)),
		GitHubPullRequestMutationOutcome::ImpossibleState =>
			GitHubPullRequestDispatchResolution::Ambiguous(ambiguity(GitHubAmbiguity::ImpossibleProviderState, None)),
	}
}

fn after_check_mutation(
	authority: GitHubCheckAuthority,
	spec: GitHubCheckSpec,
	suite_contract: GitHubCheckSuiteContract,
	outcome: GitHubCheckMutationOutcome,
) -> GitHubCheckDispatchResolution {
	match outcome {
		GitHubCheckMutationOutcome::Accepted(identity) => {
			if !check_response_matches(authority.target, identity) {
				return GitHubCheckDispatchResolution::Ambiguous(ambiguity(
					GitHubAmbiguity::ProviderResponseIdentityConflict,
					None,
				));
			}
			GitHubCheckDispatchResolution::ReadbackRequired(GitHubCheckContinuation {
				authority,
				spec,
				suite_contract,
				response_identity: Some(identity),
				reason: GitHubReadbackReason::AcceptedNeedsVerification,
			})
		},
		GitHubCheckMutationOutcome::LostResponse =>
			GitHubCheckDispatchResolution::ReadbackRequired(GitHubCheckContinuation {
				authority,
				spec,
				suite_contract,
				response_identity: None,
				reason: GitHubReadbackReason::LostResponse,
			}),
		GitHubCheckMutationOutcome::DuplicateOrAlreadyExists =>
			GitHubCheckDispatchResolution::ReadbackRequired(GitHubCheckContinuation {
				authority,
				spec,
				suite_contract,
				response_identity: None,
				reason: GitHubReadbackReason::DuplicateOrAlreadyExists,
			}),
		GitHubCheckMutationOutcome::DefinitelyNotSent =>
			GitHubCheckDispatchResolution::NoEffect(GitHubNoEffect {
				reason: GitHubNoEffectReason::ProviderProvedRequestNotSent,
				summary: None,
			}),
		GitHubCheckMutationOutcome::StaleBase =>
			GitHubCheckDispatchResolution::Stale(stale(GitHubStaleReason::BaseRevisionChanged, None)),
		GitHubCheckMutationOutcome::StaleHead =>
			GitHubCheckDispatchResolution::Stale(stale(GitHubStaleReason::HeadRevisionChanged, None)),
		GitHubCheckMutationOutcome::ConflictingMarker =>
			GitHubCheckDispatchResolution::Ambiguous(ambiguity(GitHubAmbiguity::DurableMarkerConflict, None)),
		GitHubCheckMutationOutcome::ProviderRedacted =>
			GitHubCheckDispatchResolution::Ambiguous(ambiguity(GitHubAmbiguity::ProviderRedacted, None)),
		GitHubCheckMutationOutcome::ImpossibleState =>
			GitHubCheckDispatchResolution::Ambiguous(ambiguity(GitHubAmbiguity::ImpossibleProviderState, None)),
	}
}

fn collect_pull_requests<P: GitHubEffectProvider + ?Sized>(
	provider: &P,
	authority: &GitHubPullRequestAuthority,
) -> Result<CompleteInventory<GitHubPullRequestObservation>, CollectionFailure> {
	let start = provider.begin_pull_request_snapshot(authority).map_err(CollectionFailure::Read)?;
	let inventory = collect_pages(
		&authority.repository,
		&start,
		|cursor| provider.pull_request_page(authority, &start, cursor),
		|observation| (observation.identity.id.get(), Some(observation.identity.number)),
	)?;
	let end = provider.end_pull_request_snapshot(authority, &start).map_err(CollectionFailure::Read)?;
	if end != start { return Err(CollectionFailure::Ambiguous(GitHubAmbiguity::EndSnapshotChanged)); }
	Ok(inventory)
}

fn collect_checks<P: GitHubEffectProvider + ?Sized>(
	provider: &P,
	authority: &GitHubCheckAuthority,
) -> Result<CompleteInventory<GitHubCheckObservation>, CollectionFailure> {
	let start = provider.begin_check_snapshot(authority).map_err(CollectionFailure::Read)?;
	let inventory = collect_pages(
		&authority.repository,
		&start,
		|cursor| provider.check_page(authority, &start, cursor),
		|observation| (observation.identity.run_id.get(), None),
	)?;
	let end = provider.end_check_snapshot(authority, &start).map_err(CollectionFailure::Read)?;
	if end != start { return Err(CollectionFailure::Ambiguous(GitHubAmbiguity::EndSnapshotChanged)); }
	Ok(inventory)
}

fn collect_pages<T, F, K>(
	expected_repository: &GitHubRepositoryBinding,
	start_snapshot: &GitHubSnapshot,
	mut fetch: F,
	key: K,
) -> Result<CompleteInventory<T>, CollectionFailure>
where
	F: FnMut(Option<&GitHubCursor>) -> Result<GitHubPage<T>, GitHubReadFailure>,
	K: Fn(&T) -> (u64, Option<u64>),
{
	let mut cursor = None;
	let mut seen_cursors = BTreeSet::new();
	let mut seen_pages = BTreeSet::new();
	let mut seen_primary = BTreeSet::new();
	let mut seen_secondary = BTreeSet::new();
	let mut objects = Vec::new();

	for expected_page in 1..=MAX_PAGES {
		let page = fetch(cursor.as_ref()).map_err(CollectionFailure::Read)?;
		let metadata = &page.pagination;
		if page.repository != *expected_repository {
			return Err(CollectionFailure::Ambiguous(GitHubAmbiguity::RepositoryIdentityChanged));
		}
		if metadata.snapshot != *start_snapshot {
			return Err(CollectionFailure::Ambiguous(GitHubAmbiguity::PageSnapshotChanged));
		}
		if usize::from(metadata.page_number) != expected_page {
			return Err(CollectionFailure::Ambiguous(GitHubAmbiguity::PageCycle));
		}
		if metadata.requested_cursor != cursor {
			return Err(CollectionFailure::Ambiguous(GitHubAmbiguity::CursorCycle));
		}
		if !seen_pages.insert(metadata.page_identity.clone()) {
			return Err(CollectionFailure::Ambiguous(GitHubAmbiguity::RepeatedPage));
		}

		for object in page.objects {
			let (primary, secondary) = key(&object);
			if !seen_primary.insert(primary)
				|| secondary.is_some_and(|identity| !seen_secondary.insert(identity))
			{
				return Err(CollectionFailure::Ambiguous(GitHubAmbiguity::DuplicateObjectIdentity));
			}
			objects.push(object);
		}

		match (metadata.has_next_page, metadata.next_cursor.clone()) {
			(true, None) => return Err(CollectionFailure::Ambiguous(GitHubAmbiguity::MissingNextCursor)),
			(false, Some(_)) => return Err(CollectionFailure::Ambiguous(GitHubAmbiguity::UnexpectedNextCursor)),
			(true, Some(next)) => {
				if cursor.as_ref() == Some(&next) || !seen_cursors.insert(next.clone()) {
					return Err(CollectionFailure::Ambiguous(GitHubAmbiguity::CursorCycle));
				}
				cursor = Some(next);
			},
			(false, None) => {
				let pages = u16::try_from(expected_page)
					.map_err(|_| CollectionFailure::Ambiguous(GitHubAmbiguity::PaginationLimit))?;
				let object_count = u32::try_from(objects.len())
					.map_err(|_| CollectionFailure::Ambiguous(GitHubAmbiguity::PaginationLimit))?;
				return Ok(CompleteInventory {
					objects,
					summary: GitHubObservationSummary { pages, objects: object_count },
				});
			},
		}
	}

	Err(CollectionFailure::Ambiguous(GitHubAmbiguity::PaginationLimit))
}

enum PullRequestTerminal {
	Completed(GitHubPullRequestCompletion),
	Stale(GitHubStale),
	Ambiguous(GitHubTerminalAmbiguity),
	NoEffect(GitHubNoEffect),
}

enum PullRequestPresence {
	Absent(GitHubObservationSummary),
	Terminal(PullRequestTerminal),
}
impl PullRequestTerminal {
	fn into_dispatch(self) -> GitHubPullRequestDispatchResolution {
		match self {
			Self::Completed(value) => GitHubPullRequestDispatchResolution::Completed(value),
			Self::Stale(value) => GitHubPullRequestDispatchResolution::Stale(value),
			Self::Ambiguous(value) => GitHubPullRequestDispatchResolution::Ambiguous(value),
			Self::NoEffect(value) => GitHubPullRequestDispatchResolution::NoEffect(value),
		}
	}
	fn into_readback(self) -> GitHubPullRequestReadbackResolution {
		match self {
			Self::Completed(value) => GitHubPullRequestReadbackResolution::Completed(value),
			Self::Stale(value) => GitHubPullRequestReadbackResolution::Stale(value),
			Self::Ambiguous(value) => GitHubPullRequestReadbackResolution::Ambiguous(value),
			Self::NoEffect(value) => GitHubPullRequestReadbackResolution::NoEffect(value),
		}
	}
}

enum CheckTerminal {
	Completed(GitHubCheckCompletion),
	Stale(GitHubStale),
	Ambiguous(GitHubTerminalAmbiguity),
	NoEffect(GitHubNoEffect),
}

enum CheckPresence {
	Absent(GitHubObservationSummary),
	Terminal(CheckTerminal),
}
impl CheckTerminal {
	fn into_dispatch(self) -> GitHubCheckDispatchResolution {
		match self {
			Self::Completed(value) => GitHubCheckDispatchResolution::Completed(value),
			Self::Stale(value) => GitHubCheckDispatchResolution::Stale(value),
			Self::Ambiguous(value) => GitHubCheckDispatchResolution::Ambiguous(value),
			Self::NoEffect(value) => GitHubCheckDispatchResolution::NoEffect(value),
		}
	}
	fn into_readback(self) -> GitHubCheckReadbackResolution {
		match self {
			Self::Completed(value) => GitHubCheckReadbackResolution::Completed(value),
			Self::Stale(value) => GitHubCheckReadbackResolution::Stale(value),
			Self::Ambiguous(value) => GitHubCheckReadbackResolution::Ambiguous(value),
			Self::NoEffect(value) => GitHubCheckReadbackResolution::NoEffect(value),
		}
	}
}

fn reconcile_pull_request_present(
	authority: &GitHubPullRequestAuthority,
	spec: &GitHubPullRequestSpec,
	response_identity: Option<GitHubPullRequestIdentity>,
	inventory: CompleteInventory<GitHubPullRequestObservation>,
) -> PullRequestPresence {
	let summary = inventory.summary;
	let mut candidate = None;
	for observation in inventory.objects {
		let marker = match &observation.marker {
			GitHubProviderField::Redacted =>
				return PullRequestPresence::Terminal(PullRequestTerminal::Ambiguous(ambiguity(
					GitHubAmbiguity::ProviderRedacted,
					Some(summary),
				))),
			GitHubProviderField::Visible(marker) => marker,
		};
		let marker_matches = marker.as_ref() == Some(&authority.marker);
		let target_matches = matches!(authority.target, GitHubPullRequestTarget::Exact(expected) if expected == observation.identity);
		let same_head = observation.revisions.head_branch == authority.revisions.head_branch;
		if marker_matches || target_matches || same_head {
			if candidate.is_some() {
				return PullRequestPresence::Terminal(PullRequestTerminal::Ambiguous(ambiguity(
					GitHubAmbiguity::DuplicateObjectIdentity,
					Some(summary),
				)));
			}
			if !marker_matches {
				return PullRequestPresence::Terminal(PullRequestTerminal::Ambiguous(ambiguity(
					GitHubAmbiguity::DurableMarkerConflict,
					Some(summary),
				)));
			}
			candidate = Some(observation);
		}
	}
	let Some(observation) = candidate else { return PullRequestPresence::Absent(summary); };
	if !pull_request_target_matches(authority.target, observation.identity) {
		return PullRequestPresence::Terminal(PullRequestTerminal::Stale(stale(
			GitHubStaleReason::PullRequestIdentityChanged,
			Some(summary),
		)));
	}
	if response_identity.is_some_and(|identity| identity != observation.identity) {
		return PullRequestPresence::Terminal(PullRequestTerminal::Ambiguous(ambiguity(
			GitHubAmbiguity::ProviderResponseIdentityConflict,
			Some(summary),
		)));
	}
	if let Some(reason) = stale_revisions(&authority.revisions, &observation.revisions) {
		return PullRequestPresence::Terminal(PullRequestTerminal::Stale(stale(reason, Some(summary))));
	}
	if observation.state != GitHubPullRequestState::Open {
		return PullRequestPresence::Terminal(PullRequestTerminal::Stale(stale(
			GitHubStaleReason::PullRequestNotOpen,
			Some(summary),
		)));
	}
	PullRequestPresence::Terminal(match &observation.spec {
		GitHubProviderField::Redacted => PullRequestTerminal::Ambiguous(ambiguity(
			GitHubAmbiguity::ProviderRedacted,
			Some(summary),
		)),
		GitHubProviderField::Visible(observed) if observed != spec =>
			PullRequestTerminal::Ambiguous(ambiguity(
				GitHubAmbiguity::ExternallyChangedFields,
				Some(summary),
			)),
		GitHubProviderField::Visible(_) => PullRequestTerminal::Completed(
			GitHubPullRequestCompletion { observation, summary },
		),
	})
}

fn reconcile_check_present(
	authority: &GitHubCheckAuthority,
	spec: &GitHubCheckSpec,
	suite_contract: &GitHubCheckSuiteContract,
	response_identity: Option<GitHubCheckIdentity>,
	inventory: CompleteInventory<GitHubCheckObservation>,
) -> CheckPresence {
	let summary = inventory.summary;
	let mut candidate = None;
	let mut all = Vec::with_capacity(inventory.objects.len());
	for observation in inventory.objects {
		let marker = match &observation.marker {
			GitHubProviderField::Redacted =>
				return CheckPresence::Terminal(CheckTerminal::Ambiguous(ambiguity(
					GitHubAmbiguity::ProviderRedacted,
					Some(summary),
				))),
			GitHubProviderField::Visible(marker) => marker,
		};
		if matches!(&observation.spec, GitHubProviderField::Redacted) {
			return CheckPresence::Terminal(CheckTerminal::Ambiguous(ambiguity(
				GitHubAmbiguity::ProviderRedacted,
				Some(summary),
			)));
		}
		let marker_matches = marker.as_ref() == Some(&authority.marker);
		let target_matches = matches!(authority.target, GitHubCheckTarget::Exact(expected) if expected == observation.identity);
		let same_named_head = observation.revisions.head_branch == authority.revisions.head_branch
			&& matches!(&observation.spec, GitHubProviderField::Visible(observed) if observed.name == spec.name);
		if marker_matches || target_matches || same_named_head {
			if candidate.is_some() {
				return CheckPresence::Terminal(CheckTerminal::Ambiguous(ambiguity(
					GitHubAmbiguity::ConflictingCheckResults,
					Some(summary),
				)));
			}
			if !marker_matches {
				return CheckPresence::Terminal(CheckTerminal::Ambiguous(ambiguity(
					GitHubAmbiguity::DurableMarkerConflict,
					Some(summary),
				)));
			}
			candidate = Some(observation.clone());
		}
		all.push(observation);
	}
	let Some(observation) = candidate else { return CheckPresence::Absent(summary); };
	if observation.pull_request != authority.pull_request {
		return CheckPresence::Terminal(CheckTerminal::Ambiguous(ambiguity(
			GitHubAmbiguity::DurableMarkerConflict,
			Some(summary),
		)));
	}
	if !check_target_matches(authority.target, observation.identity) {
		return CheckPresence::Terminal(CheckTerminal::Stale(stale(
			GitHubStaleReason::CheckIdentityChanged,
			Some(summary),
		)));
	}
	if response_identity.is_some_and(|identity| identity != observation.identity) {
		return CheckPresence::Terminal(CheckTerminal::Ambiguous(ambiguity(
			GitHubAmbiguity::ProviderResponseIdentityConflict,
			Some(summary),
		)));
	}
	if let Some(reason) = stale_revisions(&authority.revisions, &observation.revisions) {
		return CheckPresence::Terminal(CheckTerminal::Stale(stale(reason, Some(summary))));
	}
	match &observation.spec {
		GitHubProviderField::Visible(observed) if observed == spec => {},
		GitHubProviderField::Visible(_) => {
			return CheckPresence::Terminal(CheckTerminal::Ambiguous(ambiguity(
				GitHubAmbiguity::ExternallyChangedFields,
				Some(summary),
			)));
		},
		GitHubProviderField::Redacted => unreachable!("redaction was rejected above"),
	}

	if !suite_contract.required_runs.iter().any(|required| required.name == spec.name) {
		return CheckPresence::Terminal(CheckTerminal::Ambiguous(ambiguity(
			GitHubAmbiguity::IncompleteChecks,
			Some(summary),
		)));
	}
	let required_runs = match complete_required_runs(suite_contract, observation.identity.suite_id, &all) {
		Ok(required) => required,
		Err(reason) =>
			return CheckPresence::Terminal(CheckTerminal::Ambiguous(ambiguity(reason, Some(summary)))),
	};
	CheckPresence::Terminal(CheckTerminal::Completed(GitHubCheckCompletion {
		observation,
		required_runs,
		summary,
	}))
}

fn complete_required_runs(
	contract: &GitHubCheckSuiteContract,
	suite_id: GitHubCheckSuiteId,
	observations: &[GitHubCheckObservation],
) -> Result<Vec<GitHubCheckObservation>, GitHubAmbiguity> {
	let mut by_name = BTreeMap::<&str, &GitHubCheckObservation>::new();
	for observation in observations.iter().filter(|item| item.identity.suite_id == suite_id) {
		let GitHubProviderField::Visible(spec) = &observation.spec else {
			return Err(GitHubAmbiguity::ProviderRedacted);
		};
		if by_name.insert(spec.name.value(), observation).is_some() {
			return Err(GitHubAmbiguity::ConflictingCheckResults);
		}
	}

	let mut complete = Vec::with_capacity(contract.required_runs.len());
	for required in &contract.required_runs {
		let Some(observation) = by_name.get(required.name.value()).copied() else {
			return Err(GitHubAmbiguity::IncompleteChecks);
		};
		if required.run_id.is_some_and(|run_id| run_id != observation.identity.run_id) {
			return Err(GitHubAmbiguity::ConflictingCheckResults);
		}
		complete.push(observation.clone());
	}
	Ok(complete)
}

fn pull_request_response_matches(
	target: GitHubPullRequestTarget,
	response: GitHubPullRequestIdentity,
) -> bool {
	pull_request_target_matches(target, response)
}

fn pull_request_target_matches(
	target: GitHubPullRequestTarget,
	observed: GitHubPullRequestIdentity,
) -> bool {
	match target {
		GitHubPullRequestTarget::Unassigned => true,
		GitHubPullRequestTarget::Exact(expected) => expected == observed,
	}
}

fn check_response_matches(target: GitHubCheckTarget, response: GitHubCheckIdentity) -> bool {
	check_target_matches(target, response)
}

fn check_target_matches(target: GitHubCheckTarget, observed: GitHubCheckIdentity) -> bool {
	match target {
		GitHubCheckTarget::Unassigned => true,
		GitHubCheckTarget::InSuite(expected) => expected == observed.suite_id,
		GitHubCheckTarget::Exact(expected) => expected == observed,
	}
}

fn stale_revisions(
	expected: &GitHubRevisionAuthority,
	observed: &GitHubRevisionAuthority,
) -> Option<GitHubStaleReason> {
	let base = expected.base_branch != observed.base_branch || expected.base_revision != observed.base_revision;
	let head = expected.head_branch != observed.head_branch || expected.head_revision != observed.head_revision;
	match (base, head) {
		(false, false) => None,
		(true, false) => Some(GitHubStaleReason::BaseRevisionChanged),
		(false, true) => Some(GitHubStaleReason::HeadRevisionChanged),
		(true, true) => Some(GitHubStaleReason::BaseAndHeadChanged),
	}
}

fn collection_ambiguity(failure: CollectionFailure) -> GitHubTerminalAmbiguity {
	match failure {
		CollectionFailure::Read(failure) => ambiguity(read_failure_ambiguity(failure), None),
		CollectionFailure::Ambiguous(reason) => ambiguity(reason, None),
	}
}

fn read_failure_ambiguity(failure: GitHubReadFailure) -> GitHubAmbiguity {
	match failure {
		GitHubReadFailure::Unauthorized => GitHubAmbiguity::Unauthorized,
		GitHubReadFailure::Forbidden => GitHubAmbiguity::Forbidden,
		GitHubReadFailure::NotFound => GitHubAmbiguity::ProviderObjectNotFound,
		GitHubReadFailure::ProviderRedacted => GitHubAmbiguity::ProviderRedacted,
		GitHubReadFailure::IncompleteChecks => GitHubAmbiguity::IncompleteChecks,
		GitHubReadFailure::MalformedResponse => GitHubAmbiguity::MalformedProviderResponse,
		GitHubReadFailure::UnknownProviderValue => GitHubAmbiguity::UnknownProviderValue,
		GitHubReadFailure::ImpossibleProviderState => GitHubAmbiguity::ImpossibleProviderState,
		GitHubReadFailure::TemporarilyUnavailable | GitHubReadFailure::RateLimited =>
			GitHubAmbiguity::MalformedProviderResponse,
	}
}

fn ambiguity(
	reason: GitHubAmbiguity,
	summary: Option<GitHubObservationSummary>,
) -> GitHubTerminalAmbiguity {
	GitHubTerminalAmbiguity { reason, summary }
}

fn stale(reason: GitHubStaleReason, summary: Option<GitHubObservationSummary>) -> GitHubStale {
	GitHubStale { reason, summary }
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
				|| (b'a'..=b'f').contains(byte)
		})
}
