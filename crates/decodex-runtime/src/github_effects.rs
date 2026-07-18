//! Typed GitHub pull-request and check-run effects with positive readback.
//!
//! This module owns provider-specific mechanics only. Its values are explicit, forgeable inputs;
//! they do not replace PostgreSQL operation authority or grant dispatch. In particular, no value is
//! derived from the current directory, a checkout, Git configuration, a remote URL, or process
//! configuration. After a mutation may have reached GitHub, callers may only retain
//! [`GitHubEffectResolution::ReadbackRequired`] and reconcile provider facts. They must never turn
//! that result back into a fresh mutation attempt.
//!
//! Frozen integration-gate cases for XY-1353 are: all-page absence versus cursor cycle/truncation
//! and snapshot drift; accepted mutation with a lost response followed only by readback; duplicate
//! or already-existing objects with exact completion versus marker/object conflict; base or head
//! movement producing stale authority; and unknown check status/conclusion producing terminal
//! ambiguity. This slice deliberately leaves those cases unexecuted until the integrated freeze.

use std::{
	collections::{BTreeMap, BTreeSet},
	error::Error,
	fmt::{Debug, Display, Formatter},
};

use decodex_core::contains_credential_material;

/// Maximum cursor pages accepted for one authoritative inventory.
pub const MAX_GITHUB_PAGES: usize = 256;
/// Maximum provider objects accepted on one page.
pub const MAX_GITHUB_OBJECTS_PER_PAGE: usize = 100;
/// Maximum bytes in a provider cursor or snapshot identity.
pub const MAX_GITHUB_OPAQUE_VALUE_BYTES: usize = 512;
/// Maximum bytes in an owner, repository, branch, check name, or revision.
pub const MAX_GITHUB_IDENTITY_VALUE_BYTES: usize = 256;
/// Maximum bytes in a pull-request title.
pub const MAX_GITHUB_PULL_REQUEST_TITLE_BYTES: usize = 256;
/// Maximum bytes in a pull-request body retained at the provider boundary.
pub const MAX_GITHUB_PULL_REQUEST_BODY_BYTES: usize = 65_536;

/// Closed construction failure. It contains no caller or provider text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GitHubContractError {
	InvalidIdentity,
	InvalidRepository,
	InvalidBranch,
	InvalidRevision,
	InvalidMarker,
	InvalidText,
	CredentialRejected,
	InvalidPage,
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
			Self::InvalidMarker => "invalid GitHub durable operation marker",
			Self::InvalidText => "invalid bounded GitHub public text",
			Self::CredentialRejected => "credential-bearing GitHub public text rejected",
			Self::InvalidPage => "invalid GitHub provider page",
			Self::UnknownProviderValue => "unknown GitHub provider value",
			Self::ImpossibleProviderState => "impossible GitHub provider state",
		})
	}
}
impl Error for GitHubContractError {}

macro_rules! positive_id {
	($(#[$meta:meta])* $name:ident) => {
		$(#[$meta])*
		#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
		pub struct $name(u64);
		impl $name {
			pub fn new(value: u64) -> Result<Self, GitHubContractError> {
				if value == 0 { Err(GitHubContractError::InvalidIdentity) } else { Ok(Self(value)) }
			}
			pub const fn get(self) -> u64 { self.0 }
		}
	};
}

positive_id!(/// Immutable GitHub repository database identity.
	GitHubRepositoryId);
positive_id!(/// GitHub App installation identity selected by trusted account routing.
	GitHubInstallationId);
positive_id!(/// GitHub account database identity selected by trusted account routing.
	GitHubAccountId);
positive_id!(/// Immutable GitHub pull-request database identity.
	GitHubPullRequestId);
positive_id!(/// GitHub check-suite database identity.
	GitHubCheckSuiteId);
positive_id!(/// GitHub check-run database identity.
	GitHubCheckRunId);

/// Canonical provider identity. No hostname or API origin is accepted from ambient configuration.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum GitHubProviderIdentity {
	GitHubDotCom,
}

macro_rules! bounded_identity {
	($(#[$meta:meta])* $name:ident, $validator:ident, $error:ident) => {
		$(#[$meta])*
		#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
		pub struct $name(String);
		impl $name {
			pub fn new(value: impl Into<String>) -> Result<Self, GitHubContractError> {
				let value = value.into();
				if !$validator(&value) { return Err(GitHubContractError::$error); }
				Ok(Self(value))
			}
			pub fn as_str(&self) -> &str { &self.0 }
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

fn valid_repository_name(value: &str) -> bool {
	!value.is_empty()
		&& value.len() <= MAX_GITHUB_IDENTITY_VALUE_BYTES
		&& value.trim() == value
		&& !matches!(value, "." | "..")
		&& value
			.bytes()
			.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_' | b'.'))
}

fn valid_branch(value: &str) -> bool {
	!value.is_empty()
		&& value.len() <= MAX_GITHUB_IDENTITY_VALUE_BYTES
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
	value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

bounded_identity!(/// Canonical GitHub repository owner login.
	GitHubRepositoryOwner, valid_owner, InvalidRepository);
bounded_identity!(/// Canonical GitHub repository name.
	GitHubRepositoryName, valid_repository_name, InvalidRepository);
bounded_identity!(/// Exact provider branch name.
	GitHubBranchName, valid_branch, InvalidBranch);
bounded_identity!(/// Exact lowercase forty-hex Git revision.
	GitHubRevision, valid_revision, InvalidRevision);

/// Exact repository and installation/account binding supplied by trusted composition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitHubRepositoryBinding {
	provider: GitHubProviderIdentity,
	repository_id: GitHubRepositoryId,
	owner: GitHubRepositoryOwner,
	name: GitHubRepositoryName,
	installation_id: GitHubInstallationId,
	account_id: GitHubAccountId,
}
impl GitHubRepositoryBinding {
	pub fn new(
		provider: GitHubProviderIdentity,
		repository_id: GitHubRepositoryId,
		owner: GitHubRepositoryOwner,
		name: GitHubRepositoryName,
		installation_id: GitHubInstallationId,
		account_id: GitHubAccountId,
	) -> Self {
		Self { provider, repository_id, owner, name, installation_id, account_id }
	}
	pub const fn provider(&self) -> GitHubProviderIdentity { self.provider }
	pub const fn repository_id(&self) -> GitHubRepositoryId { self.repository_id }
	pub fn owner(&self) -> &GitHubRepositoryOwner { &self.owner }
	pub fn name(&self) -> &GitHubRepositoryName { &self.name }
	pub const fn installation_id(&self) -> GitHubInstallationId { self.installation_id }
	pub const fn account_id(&self) -> GitHubAccountId { self.account_id }
}

/// Exact base/head authority. A branch name never substitutes for its pinned revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitHubRevisionAuthority {
	base_branch: GitHubBranchName,
	base_revision: GitHubRevision,
	head_branch: GitHubBranchName,
	head_revision: GitHubRevision,
}
impl GitHubRevisionAuthority {
	pub fn new(
		base_branch: GitHubBranchName,
		base_revision: GitHubRevision,
		head_branch: GitHubBranchName,
		head_revision: GitHubRevision,
	) -> Result<Self, GitHubContractError> {
		if base_branch == head_branch {
			return Err(GitHubContractError::InvalidBranch);
		}
		Ok(Self { base_branch, base_revision, head_branch, head_revision })
	}
	pub fn base_branch(&self) -> &GitHubBranchName { &self.base_branch }
	pub fn base_revision(&self) -> &GitHubRevision { &self.base_revision }
	pub fn head_branch(&self) -> &GitHubBranchName { &self.head_branch }
	pub fn head_revision(&self) -> &GitHubRevision { &self.head_revision }
}

/// One exact durable Decodex operation marker. It is identity, never title/body matching.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct GitHubOperationMarker(String);
impl GitHubOperationMarker {
	pub fn new(value: impl Into<String>) -> Result<Self, GitHubContractError> {
		let value = value.into();
		let Some(uuid) = value.strip_prefix("decodex/github-effect/1/") else {
			return Err(GitHubContractError::InvalidMarker);
		};
		if !is_canonical_uuid_v4(uuid) {
			return Err(GitHubContractError::InvalidMarker);
		}
		Ok(Self(value))
	}
	pub fn as_str(&self) -> &str { &self.0 }
}

/// Provider-assigned identity expectation, explicit even before GitHub assigns an object.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GitHubPullRequestTarget {
	Unassigned,
	Exact { id: GitHubPullRequestId, number: u64 },
}
impl GitHubPullRequestTarget {
	pub fn exact(id: GitHubPullRequestId, number: u64) -> Result<Self, GitHubContractError> {
		if number == 0 { Err(GitHubContractError::InvalidIdentity) } else { Ok(Self::Exact { id, number }) }
	}
}

/// Complete authority tuple required by every pull-request provider call.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitHubPullRequestAuthority {
	repository: GitHubRepositoryBinding,
	revisions: GitHubRevisionAuthority,
	target: GitHubPullRequestTarget,
	marker: GitHubOperationMarker,
}
impl GitHubPullRequestAuthority {
	pub fn new(
		repository: GitHubRepositoryBinding,
		revisions: GitHubRevisionAuthority,
		target: GitHubPullRequestTarget,
		marker: GitHubOperationMarker,
	) -> Self {
		Self { repository, revisions, target, marker }
	}
	pub fn repository(&self) -> &GitHubRepositoryBinding { &self.repository }
	pub fn revisions(&self) -> &GitHubRevisionAuthority { &self.revisions }
	pub const fn target(&self) -> GitHubPullRequestTarget { self.target }
	pub fn marker(&self) -> &GitHubOperationMarker { &self.marker }
}

/// Explicit check-suite/run target, including pre-assignment state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GitHubCheckTarget {
	suite_id: Option<GitHubCheckSuiteId>,
	run_id: Option<GitHubCheckRunId>,
}
impl GitHubCheckTarget {
	pub const fn unassigned() -> Self { Self { suite_id: None, run_id: None } }
	pub const fn in_suite(suite_id: GitHubCheckSuiteId) -> Self {
		Self { suite_id: Some(suite_id), run_id: None }
	}
	pub const fn exact(suite_id: GitHubCheckSuiteId, run_id: GitHubCheckRunId) -> Self {
		Self { suite_id: Some(suite_id), run_id: Some(run_id) }
	}
	pub const fn suite_id(self) -> Option<GitHubCheckSuiteId> { self.suite_id }
	pub const fn run_id(self) -> Option<GitHubCheckRunId> { self.run_id }
}

/// Complete authority tuple required by every check provider call.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitHubCheckAuthority {
	repository: GitHubRepositoryBinding,
	revisions: GitHubRevisionAuthority,
	pull_request: (GitHubPullRequestId, u64),
	target: GitHubCheckTarget,
	marker: GitHubOperationMarker,
}
impl GitHubCheckAuthority {
	pub fn new(
		repository: GitHubRepositoryBinding,
		revisions: GitHubRevisionAuthority,
		pull_request_id: GitHubPullRequestId,
		pull_request_number: u64,
		target: GitHubCheckTarget,
		marker: GitHubOperationMarker,
	) -> Result<Self, GitHubContractError> {
		if pull_request_number == 0 { return Err(GitHubContractError::InvalidIdentity); }
		Ok(Self {
			repository,
			revisions,
			pull_request: (pull_request_id, pull_request_number),
			target,
			marker,
		})
	}
	pub fn repository(&self) -> &GitHubRepositoryBinding { &self.repository }
	pub fn revisions(&self) -> &GitHubRevisionAuthority { &self.revisions }
	pub const fn pull_request(&self) -> (GitHubPullRequestId, u64) { self.pull_request }
	pub const fn target(&self) -> GitHubCheckTarget { self.target }
	pub fn marker(&self) -> &GitHubOperationMarker { &self.marker }
}

/// Bounded credential-negative public text. Debug output is always redacted.
#[derive(Clone, Eq, PartialEq)]
pub struct GitHubPublicText(String);
impl GitHubPublicText {
	pub fn new(value: impl Into<String>, maximum: usize) -> Result<Self, GitHubContractError> {
		let value = value.into();
		if value.is_empty() || value.len() > maximum || value.chars().any(char::is_control) {
			return Err(GitHubContractError::InvalidText);
		}
		if contains_credential_material(&value) {
			return Err(GitHubContractError::CredentialRejected);
		}
		Ok(Self(value))
	}
	pub fn as_str(&self) -> &str { &self.0 }
}
impl Debug for GitHubPublicText {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str("GitHubPublicText(<redacted>)")
	}
}

/// Exact desired pull-request fields.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitHubPullRequestSpec {
	title: GitHubPublicText,
	body: GitHubPublicText,
	draft: bool,
}
impl GitHubPullRequestSpec {
	pub fn new(title: impl Into<String>, body: impl Into<String>, draft: bool) -> Result<Self, GitHubContractError> {
		Ok(Self {
			title: GitHubPublicText::new(title, MAX_GITHUB_PULL_REQUEST_TITLE_BYTES)?,
			body: GitHubPublicText::new(body, MAX_GITHUB_PULL_REQUEST_BODY_BYTES)?,
			draft,
		})
	}
	pub fn title(&self) -> &str { self.title.as_str() }
	pub fn body(&self) -> &str { self.body.as_str() }
	pub const fn draft(&self) -> bool { self.draft }
}

/// Closed provider pull-request lifecycle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GitHubPullRequestState { Open, Closed, Merged }
impl GitHubPullRequestState {
	pub fn from_provider(value: &str) -> Result<Self, GitHubContractError> {
		match value {
			"open" => Ok(Self::Open),
			"closed" => Ok(Self::Closed),
			"merged" => Ok(Self::Merged),
			_ => Err(GitHubContractError::UnknownProviderValue),
		}
	}
}

/// Visible provider value versus explicit provider redaction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GitHubProviderField<T> { Visible(T), Redacted }

/// Trusted provider facts for one pull request. They remain observations, not local authority.
#[derive(Clone, Eq, PartialEq)]
pub struct GitHubPullRequestObservation {
	id: GitHubPullRequestId,
	number: u64,
	revisions: GitHubRevisionAuthority,
	marker: GitHubProviderField<Option<GitHubOperationMarker>>,
	state: GitHubPullRequestState,
	spec: GitHubProviderField<GitHubPullRequestSpec>,
}
impl GitHubPullRequestObservation {
	pub fn new(
		id: GitHubPullRequestId,
		number: u64,
		revisions: GitHubRevisionAuthority,
		marker: GitHubProviderField<Option<GitHubOperationMarker>>,
		state: GitHubPullRequestState,
		spec: GitHubProviderField<GitHubPullRequestSpec>,
	) -> Result<Self, GitHubContractError> {
		if number == 0 { return Err(GitHubContractError::InvalidIdentity); }
		Ok(Self { id, number, revisions, marker, state, spec })
	}
	pub const fn id(&self) -> GitHubPullRequestId { self.id }
	pub const fn number(&self) -> u64 { self.number }
	pub fn revisions(&self) -> &GitHubRevisionAuthority { &self.revisions }
	pub fn marker(&self) -> &GitHubProviderField<Option<GitHubOperationMarker>> { &self.marker }
	pub const fn state(&self) -> GitHubPullRequestState { self.state }
	pub fn spec(&self) -> &GitHubProviderField<GitHubPullRequestSpec> { &self.spec }
}
impl Debug for GitHubPullRequestObservation {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("GitHubPullRequestObservation")
			.field("id", &self.id)
			.field("number", &self.number)
			.field("revisions", &self.revisions)
			.field("marker", &self.marker)
			.field("state", &self.state)
			.field("spec", &"<redacted>")
			.finish()
	}
}

/// GitHub check-run execution status.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GitHubCheckStatus { Queued, InProgress, Completed }
impl GitHubCheckStatus {
	pub fn from_provider(value: &str) -> Result<Self, GitHubContractError> {
		match value {
			"queued" => Ok(Self::Queued),
			"in_progress" => Ok(Self::InProgress),
			"completed" => Ok(Self::Completed),
			_ => Err(GitHubContractError::UnknownProviderValue),
		}
	}
}

/// Every currently supported terminal GitHub check conclusion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GitHubCheckConclusion {
	Neutral,
	Skipped,
	Cancelled,
	TimedOut,
	ActionRequired,
	Failure,
	Success,
}
impl GitHubCheckConclusion {
	pub fn from_provider(value: &str) -> Result<Self, GitHubContractError> {
		match value {
			"neutral" => Ok(Self::Neutral),
			"skipped" => Ok(Self::Skipped),
			"cancelled" => Ok(Self::Cancelled),
			"timed_out" => Ok(Self::TimedOut),
			"action_required" => Ok(Self::ActionRequired),
			"failure" => Ok(Self::Failure),
			"success" => Ok(Self::Success),
			_ => Err(GitHubContractError::UnknownProviderValue),
		}
	}
}

/// Valid check state. Non-completed runs cannot have a conclusion; completed runs must have one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GitHubCheckState {
	status: GitHubCheckStatus,
	conclusion: Option<GitHubCheckConclusion>,
}
impl GitHubCheckState {
	pub fn new(
		status: GitHubCheckStatus,
		conclusion: Option<GitHubCheckConclusion>,
	) -> Result<Self, GitHubContractError> {
		if matches!(status, GitHubCheckStatus::Completed) != conclusion.is_some() {
			return Err(GitHubContractError::ImpossibleProviderState);
		}
		Ok(Self { status, conclusion })
	}
	pub fn from_provider(status: &str, conclusion: Option<&str>) -> Result<Self, GitHubContractError> {
		Self::new(
			GitHubCheckStatus::from_provider(status)?,
			conclusion.map(GitHubCheckConclusion::from_provider).transpose()?,
		)
	}
	pub const fn status(self) -> GitHubCheckStatus { self.status }
	pub const fn conclusion(self) -> Option<GitHubCheckConclusion> { self.conclusion }
}

/// Exact desired check-run fields.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitHubCheckSpec {
	name: GitHubPublicText,
	state: GitHubCheckState,
}
impl GitHubCheckSpec {
	pub fn new(name: impl Into<String>, state: GitHubCheckState) -> Result<Self, GitHubContractError> {
		Ok(Self { name: GitHubPublicText::new(name, MAX_GITHUB_IDENTITY_VALUE_BYTES)?, state })
	}
	pub fn name(&self) -> &str { self.name.as_str() }
	pub const fn state(&self) -> GitHubCheckState { self.state }
}

/// Trusted provider facts for one check run.
#[derive(Clone, Eq, PartialEq)]
pub struct GitHubCheckObservation {
	suite_id: GitHubCheckSuiteId,
	run_id: GitHubCheckRunId,
	pull_request: (GitHubPullRequestId, u64),
	revisions: GitHubRevisionAuthority,
	marker: GitHubProviderField<Option<GitHubOperationMarker>>,
	spec: GitHubProviderField<GitHubCheckSpec>,
}
impl GitHubCheckObservation {
	pub fn new(
		suite_id: GitHubCheckSuiteId,
		run_id: GitHubCheckRunId,
		pull_request_id: GitHubPullRequestId,
		pull_request_number: u64,
		revisions: GitHubRevisionAuthority,
		marker: GitHubProviderField<Option<GitHubOperationMarker>>,
		spec: GitHubProviderField<GitHubCheckSpec>,
	) -> Result<Self, GitHubContractError> {
		if pull_request_number == 0 { return Err(GitHubContractError::InvalidIdentity); }
		Ok(Self {
			suite_id,
			run_id,
			pull_request: (pull_request_id, pull_request_number),
			revisions,
			marker,
			spec,
		})
	}
	pub const fn suite_id(&self) -> GitHubCheckSuiteId { self.suite_id }
	pub const fn run_id(&self) -> GitHubCheckRunId { self.run_id }
	pub const fn pull_request(&self) -> (GitHubPullRequestId, u64) { self.pull_request }
	pub fn revisions(&self) -> &GitHubRevisionAuthority { &self.revisions }
	pub fn marker(&self) -> &GitHubProviderField<Option<GitHubOperationMarker>> { &self.marker }
	pub fn spec(&self) -> &GitHubProviderField<GitHubCheckSpec> { &self.spec }
}
impl Debug for GitHubCheckObservation {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("GitHubCheckObservation")
			.field("suite_id", &self.suite_id)
			.field("run_id", &self.run_id)
			.field("pull_request", &self.pull_request)
			.field("revisions", &self.revisions)
			.field("marker", &self.marker)
			.field("spec", &"<redacted>")
			.finish()
	}
}

/// Opaque provider cursor. Debug output cannot expose its bytes.
#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct GitHubCursor(String);
impl GitHubCursor {
	pub fn new(value: impl Into<String>) -> Result<Self, GitHubContractError> {
		let value = value.into();
		if !valid_opaque(&value) { return Err(GitHubContractError::InvalidPage); }
		Ok(Self(value))
	}
	pub fn as_str(&self) -> &str { &self.0 }
}
impl Debug for GitHubCursor {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str("GitHubCursor(<redacted>)")
	}
}

/// Provider snapshot/version identity used to reject stale mixed-page inventories.
#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct GitHubSnapshotId(String);
impl GitHubSnapshotId {
	pub fn new(value: impl Into<String>) -> Result<Self, GitHubContractError> {
		let value = value.into();
		if !valid_opaque(&value) { return Err(GitHubContractError::InvalidPage); }
		Ok(Self(value))
	}
	pub fn as_str(&self) -> &str { &self.0 }
}
impl Debug for GitHubSnapshotId {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str("GitHubSnapshotId(<redacted>)")
	}
}

fn valid_opaque(value: &str) -> bool {
	!value.is_empty()
		&& value.len() <= MAX_GITHUB_OPAQUE_VALUE_BYTES
		&& value.trim() == value
		&& !value.chars().any(char::is_control)
}

/// Positive page continuation fact. `Truncated` is never authoritative absence evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GitHubPageContinuation { End, Next(GitHubCursor), Truncated }

/// One provider page bound to exact repository/account authority and one stable snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitHubPage<T> {
	repository: GitHubRepositoryBinding,
	snapshot: GitHubSnapshotId,
	requested_cursor: Option<GitHubCursor>,
	continuation: GitHubPageContinuation,
	objects: Vec<T>,
}
impl<T> GitHubPage<T> {
	pub fn new(
		repository: GitHubRepositoryBinding,
		snapshot: GitHubSnapshotId,
		requested_cursor: Option<GitHubCursor>,
		continuation: GitHubPageContinuation,
		objects: Vec<T>,
	) -> Result<Self, GitHubContractError> {
		if objects.len() > MAX_GITHUB_OBJECTS_PER_PAGE {
			return Err(GitHubContractError::InvalidPage);
		}
		Ok(Self { repository, snapshot, requested_cursor, continuation, objects })
	}
	pub fn repository(&self) -> &GitHubRepositoryBinding { &self.repository }
	pub fn snapshot(&self) -> &GitHubSnapshotId { &self.snapshot }
	pub fn requested_cursor(&self) -> Option<&GitHubCursor> { self.requested_cursor.as_ref() }
	pub fn continuation(&self) -> &GitHubPageContinuation { &self.continuation }
	pub fn objects(&self) -> &[T] { &self.objects }
}

/// Read-only provider failure. It never contains raw provider data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GitHubReadFailure {
	TemporarilyUnavailable,
	Unauthorized,
	Forbidden,
	NotFound,
	RateLimited,
	ProviderRedacted,
	IncompleteChecks,
	MalformedResponse,
	UnknownProviderValue,
}

/// Narrow provider-specific port. Every method receives the complete explicit authority tuple.
/// Implementations must not consult repository discovery, ambient configuration, or process state.
pub trait GitHubEffectProvider {
	fn pull_request_page(
		&self,
		authority: &GitHubPullRequestAuthority,
		cursor: Option<&GitHubCursor>,
	) -> Result<GitHubPage<GitHubPullRequestObservation>, GitHubReadFailure>;

	fn apply_pull_request(
		&self,
		permit: GitHubPullRequestMutationPermit,
	) -> GitHubPullRequestMutationOutcome;

	fn check_run_page(
		&self,
		authority: &GitHubCheckAuthority,
		cursor: Option<&GitHubCursor>,
	) -> Result<GitHubPage<GitHubCheckObservation>, GitHubReadFailure>;

	fn apply_check_run(&self, permit: GitHubCheckMutationPermit) -> GitHubCheckMutationOutcome;
}

/// Whether the durable caller is before its sole attempt or reconciliation-only.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GitHubEffectPhase {
	Fresh,
	ReadbackOnly,
}

/// Why a mutation response can only be followed by readback.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GitHubReadbackReason {
	AcceptedNeedsVerification,
	LostResponse,
	DuplicateOrAlreadyExists,
	TemporarilyUnavailable,
}

/// Positive no-effect classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GitHubNoEffectReason {
	CompletelyObservedAbsentAfterPossibleMutation,
	ProviderProvedRequestNotSent,
}

/// Exact stale-authority classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GitHubStaleReason {
	BaseRevisionChanged,
	HeadRevisionChanged,
	BaseAndHeadChanged,
	PullRequestNotOpen,
	PullRequestIdentityChanged,
	CheckIdentityChanged,
}

/// Terminal provider ambiguity. No variant authorizes another mutation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GitHubAmbiguity {
	CursorCycle,
	PaginationTruncated,
	PageSnapshotChanged,
	RepositoryIdentityChanged,
	DuplicateConflictingObject,
	DurableMarkerConflict,
	ProviderRedacted,
	ExternallyChangedFields,
	IncompleteChecks,
	ImpossibleProviderState,
	UnknownProviderValue,
	Unauthorized,
	Forbidden,
	ProviderObjectNotFound,
	MalformedProviderResponse,
}

/// Deterministic effect decision. Mutation permits are affine and created only from complete fresh
/// absence; accepted/unknown outcomes return only `ReadbackRequired`.
#[derive(Debug)]
pub enum GitHubEffectResolution<C, M> {
	MutationRequired(M),
	ReadbackRequired(GitHubReadbackReason),
	Completed(C),
	NoEffect(GitHubNoEffectEvidence),
	Stale(GitHubStaleEvidence),
	Ambiguous(GitHubAmbiguityEvidence),
}

/// Positive provider evidence retained for a terminal no-effect decision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitHubNoEffectEvidence {
	pub reason: GitHubNoEffectReason,
	pub snapshot: Option<GitHubSnapshotId>,
}

/// Provider evidence retained for stale authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitHubStaleEvidence {
	pub reason: GitHubStaleReason,
	pub snapshot: Option<GitHubSnapshotId>,
}

/// Evidence retained for a terminal ambiguity. Details are typed and credential-negative.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitHubAmbiguityEvidence {
	pub reason: GitHubAmbiguity,
	pub snapshot: Option<GitHubSnapshotId>,
}

/// Exact positive pull-request completion evidence for later saga/composition persistence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitHubPullRequestCompletion {
	pub snapshot: GitHubSnapshotId,
	pub pull_request: GitHubPullRequestObservation,
}

/// Exact positive check completion evidence for later saga/composition persistence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitHubCheckCompletion {
	pub snapshot: GitHubSnapshotId,
	pub check: GitHubCheckObservation,
}

/// Fresh one-use pull-request mutation request. It is intentionally not Clone or Copy.
pub struct GitHubPullRequestMutationPermit {
	authority: GitHubPullRequestAuthority,
	spec: GitHubPullRequestSpec,
}
impl GitHubPullRequestMutationPermit {
	pub fn authority(&self) -> &GitHubPullRequestAuthority { &self.authority }
	pub fn spec(&self) -> &GitHubPullRequestSpec { &self.spec }
}
impl Debug for GitHubPullRequestMutationPermit {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("GitHubPullRequestMutationPermit")
			.field("authority", &self.authority)
			.field("spec", &"<redacted>")
			.finish()
	}
}

/// Fresh one-use check mutation request. It is intentionally not Clone or Copy.
pub struct GitHubCheckMutationPermit {
	authority: GitHubCheckAuthority,
	spec: GitHubCheckSpec,
}
impl GitHubCheckMutationPermit {
	pub fn authority(&self) -> &GitHubCheckAuthority { &self.authority }
	pub fn spec(&self) -> &GitHubCheckSpec { &self.spec }
}
impl Debug for GitHubCheckMutationPermit {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("GitHubCheckMutationPermit")
			.field("authority", &self.authority)
			.field("spec", &"<redacted>")
			.finish()
	}
}

/// Mutation transport result. Success receipts remain observations and always require readback.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GitHubPullRequestMutationOutcome {
	Accepted { id: GitHubPullRequestId, number: u64 },
	LostResponse,
	DuplicateOrAlreadyExists,
	DefinitelyNotSent,
	StaleBase,
	StaleHead,
	ConflictingMarker,
	ProviderRedacted,
	ImpossibleState,
}

/// Mutation transport result. No success receipt completes a check without exact readback.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GitHubCheckMutationOutcome {
	Accepted { suite_id: GitHubCheckSuiteId, run_id: GitHubCheckRunId },
	LostResponse,
	DuplicateOrAlreadyExists,
	DefinitelyNotSent,
	StaleBase,
	StaleHead,
	ConflictingMarker,
	ProviderRedacted,
	ImpossibleState,
}

/// Map a pull-request mutation result without ever producing another mutation permit.
pub fn after_pull_request_mutation(
	outcome: GitHubPullRequestMutationOutcome,
) -> GitHubEffectResolution<GitHubPullRequestCompletion, GitHubPullRequestMutationPermit> {
	match outcome {
		GitHubPullRequestMutationOutcome::Accepted { .. } =>
			GitHubEffectResolution::ReadbackRequired(GitHubReadbackReason::AcceptedNeedsVerification),
		GitHubPullRequestMutationOutcome::LostResponse =>
			GitHubEffectResolution::ReadbackRequired(GitHubReadbackReason::LostResponse),
		GitHubPullRequestMutationOutcome::DuplicateOrAlreadyExists =>
			GitHubEffectResolution::ReadbackRequired(GitHubReadbackReason::DuplicateOrAlreadyExists),
		GitHubPullRequestMutationOutcome::DefinitelyNotSent =>
			GitHubEffectResolution::NoEffect(GitHubNoEffectEvidence {
				reason: GitHubNoEffectReason::ProviderProvedRequestNotSent,
				snapshot: None,
			}),
		GitHubPullRequestMutationOutcome::StaleBase => stale_without_snapshot(GitHubStaleReason::BaseRevisionChanged),
		GitHubPullRequestMutationOutcome::StaleHead => stale_without_snapshot(GitHubStaleReason::HeadRevisionChanged),
		GitHubPullRequestMutationOutcome::ConflictingMarker => ambiguous_without_snapshot(GitHubAmbiguity::DurableMarkerConflict),
		GitHubPullRequestMutationOutcome::ProviderRedacted => ambiguous_without_snapshot(GitHubAmbiguity::ProviderRedacted),
		GitHubPullRequestMutationOutcome::ImpossibleState => ambiguous_without_snapshot(GitHubAmbiguity::ImpossibleProviderState),
	}
}

/// Map a check mutation result without ever producing another mutation permit.
pub fn after_check_mutation(
	outcome: GitHubCheckMutationOutcome,
) -> GitHubEffectResolution<GitHubCheckCompletion, GitHubCheckMutationPermit> {
	match outcome {
		GitHubCheckMutationOutcome::Accepted { .. } =>
			GitHubEffectResolution::ReadbackRequired(GitHubReadbackReason::AcceptedNeedsVerification),
		GitHubCheckMutationOutcome::LostResponse =>
			GitHubEffectResolution::ReadbackRequired(GitHubReadbackReason::LostResponse),
		GitHubCheckMutationOutcome::DuplicateOrAlreadyExists =>
			GitHubEffectResolution::ReadbackRequired(GitHubReadbackReason::DuplicateOrAlreadyExists),
		GitHubCheckMutationOutcome::DefinitelyNotSent =>
			GitHubEffectResolution::NoEffect(GitHubNoEffectEvidence {
				reason: GitHubNoEffectReason::ProviderProvedRequestNotSent,
				snapshot: None,
			}),
		GitHubCheckMutationOutcome::StaleBase => stale_without_snapshot(GitHubStaleReason::BaseRevisionChanged),
		GitHubCheckMutationOutcome::StaleHead => stale_without_snapshot(GitHubStaleReason::HeadRevisionChanged),
		GitHubCheckMutationOutcome::ConflictingMarker => ambiguous_without_snapshot(GitHubAmbiguity::DurableMarkerConflict),
		GitHubCheckMutationOutcome::ProviderRedacted => ambiguous_without_snapshot(GitHubAmbiguity::ProviderRedacted),
		GitHubCheckMutationOutcome::ImpossibleState => ambiguous_without_snapshot(GitHubAmbiguity::ImpossibleProviderState),
	}
}

/// Read every pull-request page and reconcile exact provider facts.
pub fn reconcile_pull_request<P: GitHubEffectProvider>(
	provider: &P,
	phase: GitHubEffectPhase,
	authority: GitHubPullRequestAuthority,
	spec: GitHubPullRequestSpec,
) -> GitHubEffectResolution<GitHubPullRequestCompletion, GitHubPullRequestMutationPermit> {
	let inventory = match collect_pull_requests(provider, &authority) {
		Ok(inventory) => inventory,
		Err(failure) => return pull_request_collection_failure(failure),
	};
	reconcile_pull_request_inventory(phase, authority, spec, inventory)
}

/// Read every check page and reconcile exact provider facts.
pub fn reconcile_check_run<P: GitHubEffectProvider>(
	provider: &P,
	phase: GitHubEffectPhase,
	authority: GitHubCheckAuthority,
	spec: GitHubCheckSpec,
) -> GitHubEffectResolution<GitHubCheckCompletion, GitHubCheckMutationPermit> {
	let inventory = match collect_checks(provider, &authority) {
		Ok(inventory) => inventory,
		Err(failure) => return check_collection_failure(failure),
	};
	reconcile_check_inventory(phase, authority, spec, inventory)
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CompleteInventory<T> {
	snapshot: GitHubSnapshotId,
	objects: Vec<T>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CollectionFailure {
	Read(GitHubReadFailure),
	Ambiguous(GitHubAmbiguity),
}

fn collect_pull_requests<P: GitHubEffectProvider>(
	provider: &P,
	authority: &GitHubPullRequestAuthority,
) -> Result<CompleteInventory<GitHubPullRequestObservation>, CollectionFailure> {
	let inventory = collect_pages(
		authority.repository(),
		|cursor| provider.pull_request_page(authority, cursor),
		pull_request_key,
	)?;
	let mut identities_by_number = BTreeMap::new();
	for observation in &inventory.objects {
		if let Some(previous) = identities_by_number.insert(observation.number, observation.id)
			&& previous != observation.id
		{
			return Err(CollectionFailure::Ambiguous(GitHubAmbiguity::DuplicateConflictingObject));
		}
	}
	Ok(inventory)
}

fn collect_checks<P: GitHubEffectProvider>(
	provider: &P,
	authority: &GitHubCheckAuthority,
) -> Result<CompleteInventory<GitHubCheckObservation>, CollectionFailure> {
	collect_pages(authority.repository(), |cursor| provider.check_run_page(authority, cursor), check_key)
}

fn collect_pages<T: Clone + Eq, F, K>(
	expected_repository: &GitHubRepositoryBinding,
	mut fetch: F,
	key: K,
) -> Result<CompleteInventory<T>, CollectionFailure>
where
	F: FnMut(Option<&GitHubCursor>) -> Result<GitHubPage<T>, GitHubReadFailure>,
	K: Fn(&T) -> (u64, u64),
{
	let mut requested = None;
	let mut seen_cursors = BTreeSet::new();
	let mut snapshot = None;
	let mut objects = BTreeMap::<(u64, u64), T>::new();
	let mut first_by_primary = BTreeMap::<u64, (u64, u64)>::new();

	for _ in 0..MAX_GITHUB_PAGES {
		let page = fetch(requested.as_ref()).map_err(CollectionFailure::Read)?;
		if page.repository != *expected_repository {
			return Err(CollectionFailure::Ambiguous(GitHubAmbiguity::RepositoryIdentityChanged));
		}
		if page.requested_cursor != requested {
			return Err(CollectionFailure::Ambiguous(GitHubAmbiguity::PaginationTruncated));
		}
		match &snapshot {
			None => snapshot = Some(page.snapshot.clone()),
			Some(expected) if expected != &page.snapshot =>
				return Err(CollectionFailure::Ambiguous(GitHubAmbiguity::PageSnapshotChanged)),
			Some(_) => {},
		}

		for object in page.objects {
			let object_key = key(&object);
			if let Some(previous_key) = first_by_primary.insert(object_key.0, object_key)
				&& previous_key != object_key
			{
				return Err(CollectionFailure::Ambiguous(GitHubAmbiguity::DuplicateConflictingObject));
			}
			if let Some(previous) = objects.get(&object_key) {
				if previous != &object {
					return Err(CollectionFailure::Ambiguous(GitHubAmbiguity::DuplicateConflictingObject));
				}
			} else {
				objects.insert(object_key, object);
			}
		}

		match page.continuation {
			GitHubPageContinuation::End => {
				return Ok(CompleteInventory {
					snapshot: snapshot.expect("a fetched page always establishes a snapshot"),
					objects: objects.into_values().collect(),
				});
			},
			GitHubPageContinuation::Truncated =>
				return Err(CollectionFailure::Ambiguous(GitHubAmbiguity::PaginationTruncated)),
			GitHubPageContinuation::Next(cursor) => {
				if requested.as_ref() == Some(&cursor) || !seen_cursors.insert(cursor.clone()) {
					return Err(CollectionFailure::Ambiguous(GitHubAmbiguity::CursorCycle));
				}
				requested = Some(cursor);
			},
		}
	}

	Err(CollectionFailure::Ambiguous(GitHubAmbiguity::PaginationTruncated))
}

fn pull_request_key(value: &GitHubPullRequestObservation) -> (u64, u64) {
	(value.id.get(), value.number)
}

fn check_key(value: &GitHubCheckObservation) -> (u64, u64) {
	(value.run_id.get(), value.suite_id.get())
}

fn reconcile_pull_request_inventory(
	phase: GitHubEffectPhase,
	authority: GitHubPullRequestAuthority,
	spec: GitHubPullRequestSpec,
	inventory: CompleteInventory<GitHubPullRequestObservation>,
) -> GitHubEffectResolution<GitHubPullRequestCompletion, GitHubPullRequestMutationPermit> {
	let mut candidate = None;
	for observation in inventory.objects {
		let marker = match &observation.marker {
			GitHubProviderField::Redacted =>
				return ambiguity_with_snapshot(GitHubAmbiguity::ProviderRedacted, inventory.snapshot),
			GitHubProviderField::Visible(marker) => marker,
		};
		let marker_matches = marker.as_ref() == Some(authority.marker());
		let identity_matches = match authority.target {
			GitHubPullRequestTarget::Unassigned => false,
			GitHubPullRequestTarget::Exact { id, number } => observation.id == id && observation.number == number,
		};
		let same_head = observation.revisions.head_branch == authority.revisions.head_branch;
		if marker_matches || identity_matches || same_head {
			if candidate.is_some() {
				return ambiguity_with_snapshot(GitHubAmbiguity::DuplicateConflictingObject, inventory.snapshot);
			}
			if !marker_matches {
				return ambiguity_with_snapshot(GitHubAmbiguity::DurableMarkerConflict, inventory.snapshot);
			}
			candidate = Some(observation);
		}
	}

	let Some(observation) = candidate else {
		return absent_pull_request(phase, authority, spec, inventory.snapshot);
	};
	if !target_matches_pull_request(authority.target, &observation) {
		return stale_with_snapshot(GitHubStaleReason::PullRequestIdentityChanged, inventory.snapshot);
	}
	if let Some(reason) = stale_revisions(&authority.revisions, &observation.revisions) {
		return stale_with_snapshot(reason, inventory.snapshot);
	}
	if observation.state != GitHubPullRequestState::Open {
		return stale_with_snapshot(GitHubStaleReason::PullRequestNotOpen, inventory.snapshot);
	}
	match &observation.spec {
		GitHubProviderField::Redacted => ambiguity_with_snapshot(GitHubAmbiguity::ProviderRedacted, inventory.snapshot),
		GitHubProviderField::Visible(observed) if observed != &spec =>
			ambiguity_with_snapshot(GitHubAmbiguity::ExternallyChangedFields, inventory.snapshot),
		GitHubProviderField::Visible(_) => GitHubEffectResolution::Completed(GitHubPullRequestCompletion {
			snapshot: inventory.snapshot,
			pull_request: observation,
		}),
	}
}

fn absent_pull_request(
	phase: GitHubEffectPhase,
	authority: GitHubPullRequestAuthority,
	spec: GitHubPullRequestSpec,
	snapshot: GitHubSnapshotId,
) -> GitHubEffectResolution<GitHubPullRequestCompletion, GitHubPullRequestMutationPermit> {
	match phase {
		GitHubEffectPhase::Fresh => GitHubEffectResolution::MutationRequired(
			GitHubPullRequestMutationPermit { authority, spec },
		),
		GitHubEffectPhase::ReadbackOnly => GitHubEffectResolution::NoEffect(GitHubNoEffectEvidence {
			reason: GitHubNoEffectReason::CompletelyObservedAbsentAfterPossibleMutation,
			snapshot: Some(snapshot),
		}),
	}
}

fn target_matches_pull_request(
	target: GitHubPullRequestTarget,
	observation: &GitHubPullRequestObservation,
) -> bool {
	match target {
		GitHubPullRequestTarget::Unassigned => true,
		GitHubPullRequestTarget::Exact { id, number } => observation.id == id && observation.number == number,
	}
}

fn reconcile_check_inventory(
	phase: GitHubEffectPhase,
	authority: GitHubCheckAuthority,
	spec: GitHubCheckSpec,
	inventory: CompleteInventory<GitHubCheckObservation>,
) -> GitHubEffectResolution<GitHubCheckCompletion, GitHubCheckMutationPermit> {
	let mut candidate = None;
	for observation in inventory.objects {
		let marker = match &observation.marker {
			GitHubProviderField::Redacted =>
				return check_ambiguity_with_snapshot(GitHubAmbiguity::ProviderRedacted, inventory.snapshot),
			GitHubProviderField::Visible(marker) => marker,
		};
		let marker_matches = marker.as_ref() == Some(authority.marker());
		let run_matches = authority.target.run_id == Some(observation.run_id);
		let same_named_head = observation.revisions.head_branch == authority.revisions.head_branch
			&& matches!(&observation.spec, GitHubProviderField::Visible(observed) if observed.name() == spec.name());
		if marker_matches || run_matches || same_named_head {
			if candidate.is_some() {
				return check_ambiguity_with_snapshot(GitHubAmbiguity::DuplicateConflictingObject, inventory.snapshot);
			}
			if !marker_matches {
				return check_ambiguity_with_snapshot(GitHubAmbiguity::DurableMarkerConflict, inventory.snapshot);
			}
			candidate = Some(observation);
		}
	}

	let Some(observation) = candidate else {
		return absent_check(phase, authority, spec, inventory.snapshot);
	};
	if observation.pull_request != authority.pull_request {
		return check_ambiguity_with_snapshot(GitHubAmbiguity::DurableMarkerConflict, inventory.snapshot);
	}
	if !target_matches_check(authority.target, &observation) {
		return check_stale_with_snapshot(GitHubStaleReason::CheckIdentityChanged, inventory.snapshot);
	}
	if let Some(reason) = stale_revisions(&authority.revisions, &observation.revisions) {
		return check_stale_with_snapshot(reason, inventory.snapshot);
	}
	match &observation.spec {
		GitHubProviderField::Redacted => check_ambiguity_with_snapshot(GitHubAmbiguity::ProviderRedacted, inventory.snapshot),
		GitHubProviderField::Visible(observed) if observed != &spec =>
			check_ambiguity_with_snapshot(GitHubAmbiguity::ExternallyChangedFields, inventory.snapshot),
		GitHubProviderField::Visible(_) => GitHubEffectResolution::Completed(GitHubCheckCompletion {
			snapshot: inventory.snapshot,
			check: observation,
		}),
	}
}

fn absent_check(
	phase: GitHubEffectPhase,
	authority: GitHubCheckAuthority,
	spec: GitHubCheckSpec,
	snapshot: GitHubSnapshotId,
) -> GitHubEffectResolution<GitHubCheckCompletion, GitHubCheckMutationPermit> {
	match phase {
		GitHubEffectPhase::Fresh =>
			GitHubEffectResolution::MutationRequired(GitHubCheckMutationPermit { authority, spec }),
		GitHubEffectPhase::ReadbackOnly => GitHubEffectResolution::NoEffect(GitHubNoEffectEvidence {
			reason: GitHubNoEffectReason::CompletelyObservedAbsentAfterPossibleMutation,
			snapshot: Some(snapshot),
		}),
	}
}

fn target_matches_check(target: GitHubCheckTarget, observation: &GitHubCheckObservation) -> bool {
	target.suite_id.is_none_or(|id| id == observation.suite_id)
		&& target.run_id.is_none_or(|id| id == observation.run_id)
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

fn pull_request_collection_failure(
	failure: CollectionFailure,
) -> GitHubEffectResolution<GitHubPullRequestCompletion, GitHubPullRequestMutationPermit> {
	match failure {
		CollectionFailure::Read(GitHubReadFailure::TemporarilyUnavailable | GitHubReadFailure::RateLimited) =>
			GitHubEffectResolution::ReadbackRequired(GitHubReadbackReason::TemporarilyUnavailable),
		CollectionFailure::Read(failure) => ambiguous_without_snapshot(read_failure_ambiguity(failure)),
		CollectionFailure::Ambiguous(reason) => ambiguous_without_snapshot(reason),
	}
}

fn check_collection_failure(
	failure: CollectionFailure,
) -> GitHubEffectResolution<GitHubCheckCompletion, GitHubCheckMutationPermit> {
	match failure {
		CollectionFailure::Read(GitHubReadFailure::TemporarilyUnavailable | GitHubReadFailure::RateLimited) =>
			GitHubEffectResolution::ReadbackRequired(GitHubReadbackReason::TemporarilyUnavailable),
		CollectionFailure::Read(failure) => check_ambiguous_without_snapshot(read_failure_ambiguity(failure)),
		CollectionFailure::Ambiguous(reason) => check_ambiguous_without_snapshot(reason),
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
		GitHubReadFailure::TemporarilyUnavailable | GitHubReadFailure::RateLimited =>
			GitHubAmbiguity::MalformedProviderResponse,
	}
}

fn stale_without_snapshot<C, M>(reason: GitHubStaleReason) -> GitHubEffectResolution<C, M> {
	GitHubEffectResolution::Stale(GitHubStaleEvidence { reason, snapshot: None })
}
fn stale_with_snapshot(
	reason: GitHubStaleReason,
	snapshot: GitHubSnapshotId,
) -> GitHubEffectResolution<GitHubPullRequestCompletion, GitHubPullRequestMutationPermit> {
	GitHubEffectResolution::Stale(GitHubStaleEvidence { reason, snapshot: Some(snapshot) })
}
fn check_stale_with_snapshot(
	reason: GitHubStaleReason,
	snapshot: GitHubSnapshotId,
) -> GitHubEffectResolution<GitHubCheckCompletion, GitHubCheckMutationPermit> {
	GitHubEffectResolution::Stale(GitHubStaleEvidence { reason, snapshot: Some(snapshot) })
}
fn ambiguous_without_snapshot<C, M>(reason: GitHubAmbiguity) -> GitHubEffectResolution<C, M> {
	GitHubEffectResolution::Ambiguous(GitHubAmbiguityEvidence { reason, snapshot: None })
}
fn check_ambiguous_without_snapshot(
	reason: GitHubAmbiguity,
) -> GitHubEffectResolution<GitHubCheckCompletion, GitHubCheckMutationPermit> {
	ambiguous_without_snapshot(reason)
}
fn ambiguity_with_snapshot(
	reason: GitHubAmbiguity,
	snapshot: GitHubSnapshotId,
) -> GitHubEffectResolution<GitHubPullRequestCompletion, GitHubPullRequestMutationPermit> {
	GitHubEffectResolution::Ambiguous(GitHubAmbiguityEvidence { reason, snapshot: Some(snapshot) })
}
fn check_ambiguity_with_snapshot(
	reason: GitHubAmbiguity,
	snapshot: GitHubSnapshotId,
) -> GitHubEffectResolution<GitHubCheckCompletion, GitHubCheckMutationPermit> {
	GitHubEffectResolution::Ambiguous(GitHubAmbiguityEvidence { reason, snapshot: Some(snapshot) })
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
			matches!(index, 8 | 13 | 18 | 23) || byte.is_ascii_digit() || (b'a'..=b'f').contains(byte)
		})
}
