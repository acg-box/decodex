use std::{
	collections::BTreeMap,
	error::Error,
	ffi::OsStr,
	fmt::{Debug, Display, Formatter},
	future::Future,
	path::{Component, Path, PathBuf},
};

use crate::{Agent, AgentRole, AgentStatus};

/// Maximum UTF-8 bytes in one stable repository identity.
pub const MAX_REPOSITORY_IDENTITY_BYTES: usize = 128;
/// Maximum encoded bytes in one server-host Project path.
pub const MAX_PROJECT_PATH_BYTES: usize = 4_096;
/// Maximum fields in one Project metadata projection.
pub const MAX_PROJECT_METADATA_FIELDS: usize = 32;
/// Maximum bytes in one Project metadata key.
pub const MAX_PROJECT_METADATA_KEY_BYTES: usize = 64;
/// Maximum bytes in one Project metadata string value.
pub const MAX_PROJECT_METADATA_VALUE_BYTES: usize = 256;

/// Application port for transactional Project authority operations.
pub trait ProjectRepository {
	/// Adapter-owned error.
	type Error: Error + Send + Sync + 'static;

	/// Atomically create an active Project and its one canonical active Lead.
	fn create_project(
		&self,
		project: Project,
		lead: Agent,
	) -> impl Future<Output = Result<ProjectAuthority, Self::Error>> + Send;

	/// Deterministically read one Project with its canonical Lead.
	fn project(
		&self,
		id: &ProjectId,
	) -> impl Future<Output = Result<Option<ProjectAuthority>, Self::Error>> + Send;

	/// Atomically transition a Project and canonical Lead at one expected revision.
	fn transition_project(
		&self,
		id: &ProjectId,
		expected_revision: u64,
		status: ProjectStatus,
	) -> impl Future<Output = Result<ProjectAuthority, Self::Error>> + Send;
}

/// Stable canonical Project identity.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ProjectId(String);
impl ProjectId {
	/// Parse one canonical lowercase RFC 9562 UUID version 4 identity.
	pub fn new(value: impl Into<String>) -> Result<Self, ProjectError> {
		let value = value.into();

		if !is_canonical_uuid_v4(&value) {
			return Err(ProjectError::InvalidProjectId);
		}

		Ok(Self(value))
	}

	/// Borrow the canonical Project identity.
	pub fn as_str(&self) -> &str {
		&self.0
	}
}

impl Display for ProjectId {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(&self.0)
	}
}

/// Stable repository identity independent from its current server-host root.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RepositoryIdentity(String);
impl RepositoryIdentity {
	/// Parse bounded canonical lowercase repository identity text.
	pub fn new(value: impl Into<String>) -> Result<Self, ProjectError> {
		let value = value.into();

		if !is_canonical_repository_identity(&value) {
			return Err(ProjectError::InvalidRepositoryIdentity);
		}

		Ok(Self(value))
	}

	/// Borrow the canonical repository identity.
	pub fn as_str(&self) -> &str {
		&self.0
	}
}

impl Display for RepositoryIdentity {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(&self.0)
	}
}

/// Absolute path meaningful only on the Decodex server host.
#[derive(Clone, Eq, PartialEq)]
pub struct ServerProjectPath(PathBuf);
impl ServerProjectPath {
	fn new(value: PathBuf) -> Result<Self, ProjectError> {
		if !is_normalized_absolute_host_path(&value) {
			return Err(ProjectError::InvalidServerHostPath);
		}

		Ok(Self(value))
	}

	/// Access the path explicitly as a server-host-only value.
	pub fn as_server_path(&self) -> &Path {
		&self.0
	}
}

impl Debug for ServerProjectPath {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str("ServerProjectPath(<server-host-only>)")
	}
}

/// Stable repository binding and default working directory for one Project.
#[derive(Clone, Eq, PartialEq)]
pub struct ProjectRepositoryBinding {
	identity: RepositoryIdentity,
	root: ServerProjectPath,
	default_cwd: ServerProjectPath,
}
impl ProjectRepositoryBinding {
	/// Bind a repository identity to an absolute server-host root and contained default cwd.
	pub fn new(
		identity: RepositoryIdentity,
		root: PathBuf,
		default_cwd: PathBuf,
	) -> Result<Self, ProjectError> {
		let root = ServerProjectPath::new(root)?;
		let default_cwd = ServerProjectPath::new(default_cwd)?;

		if !default_cwd.as_server_path().starts_with(root.as_server_path()) {
			return Err(ProjectError::DefaultCwdOutsideRepository);
		}

		Ok(Self { identity, root, default_cwd })
	}

	/// Stable repository identity.
	pub const fn identity(&self) -> &RepositoryIdentity {
		&self.identity
	}

	/// Absolute root on the server host.
	pub const fn root(&self) -> &ServerProjectPath {
		&self.root
	}

	/// Absolute default cwd on the server host, contained by the root.
	pub const fn default_cwd(&self) -> &ServerProjectPath {
		&self.default_cwd
	}
}

impl Debug for ProjectRepositoryBinding {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("ProjectRepositoryBinding")
			.field("identity", &self.identity)
			.field("root", &self.root)
			.field("default_cwd", &self.default_cwd)
			.finish()
	}
}

/// Bounded deterministic Project metadata projection.
#[derive(Clone, Default, Eq, PartialEq)]
pub struct ProjectMetadata(BTreeMap<String, ProjectMetadataValue>);
impl ProjectMetadata {
	/// Validate one closed metadata projection.
	pub fn new(values: BTreeMap<String, ProjectMetadataValue>) -> Result<Self, ProjectError> {
		if values.len() > MAX_PROJECT_METADATA_FIELDS {
			return Err(ProjectError::InvalidMetadata);
		}

		for (key, value) in &values {
			if key.is_empty()
				|| key.len() > MAX_PROJECT_METADATA_KEY_BYTES
				|| !key.bytes().enumerate().all(|(index, byte)| {
					if index == 0 {
						byte.is_ascii_lowercase()
					} else {
						byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'
					}
				}) {
				return Err(ProjectError::InvalidMetadata);
			}
			if crate::is_credential_metadata_key(key) {
				return Err(ProjectError::CredentialRejected);
			}
			if matches!(value, ProjectMetadataValue::Text(text) if text.len() > MAX_PROJECT_METADATA_VALUE_BYTES || text.chars().any(char::is_control))
			{
				return Err(ProjectError::InvalidMetadata);
			}
			if matches!(value, ProjectMetadataValue::Text(text) if crate::contains_credential_material(text))
			{
				return Err(ProjectError::CredentialRejected);
			}
		}

		Ok(Self(values))
	}

	/// Empty metadata.
	pub const fn empty() -> Self {
		Self(BTreeMap::new())
	}

	/// Borrow metadata in canonical key order.
	pub const fn as_map(&self) -> &BTreeMap<String, ProjectMetadataValue> {
		&self.0
	}
}

impl Debug for ProjectMetadata {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("ProjectMetadata")
			.field("field_count", &self.0.len())
			.finish_non_exhaustive()
	}
}

/// Canonical inert Project authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Project {
	id: ProjectId,
	repository: ProjectRepositoryBinding,
	status: ProjectStatus,
	metadata: ProjectMetadata,
	revision: u64,
}
impl Project {
	/// Create revision one of an active Project.
	pub fn new(
		id: ProjectId,
		repository: ProjectRepositoryBinding,
		metadata: ProjectMetadata,
	) -> Self {
		Self { id, repository, status: ProjectStatus::Active, metadata, revision: 1 }
	}

	/// Validate deterministic persistence readback.
	pub fn from_stored(
		id: ProjectId,
		repository: ProjectRepositoryBinding,
		status: ProjectStatus,
		metadata: ProjectMetadata,
		revision: u64,
	) -> Result<Self, ProjectError> {
		if revision == 0 {
			return Err(ProjectError::InvalidRevision);
		}

		Ok(Self { id, repository, status, metadata, revision })
	}

	/// Stable identity.
	pub const fn id(&self) -> &ProjectId {
		&self.id
	}

	/// Stable repository and host-path binding.
	pub const fn repository(&self) -> &ProjectRepositoryBinding {
		&self.repository
	}

	/// Current inert lifecycle.
	pub const fn status(&self) -> ProjectStatus {
		self.status
	}

	/// Bounded metadata.
	pub const fn metadata(&self) -> &ProjectMetadata {
		&self.metadata
	}

	/// Positive optimistic revision.
	pub const fn revision(&self) -> u64 {
		self.revision
	}

	/// Apply one legal expected-revision lifecycle transition.
	pub fn transition(
		&mut self,
		expected_revision: u64,
		status: ProjectStatus,
	) -> Result<(), ProjectError> {
		if expected_revision == 0 || expected_revision != self.revision {
			return Err(ProjectError::RevisionConflict);
		}
		if status == self.status
			|| !matches!(
				(self.status, status),
				(ProjectStatus::Active, ProjectStatus::Paused | ProjectStatus::Archived)
					| (ProjectStatus::Paused, ProjectStatus::Active | ProjectStatus::Archived)
			) {
			return Err(ProjectError::InvalidLifecycle);
		}

		self.revision = self.revision.checked_add(1).ok_or(ProjectError::InvalidRevision)?;
		self.status = status;

		Ok(())
	}
}

/// Project plus its one canonical Lead deterministic readback.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectAuthority {
	/// Canonical Project aggregate.
	pub project: Project,
	/// Canonical Lead for the Project.
	pub lead: Agent,
}
impl ProjectAuthority {
	/// Validate the Project/Lead identity, role, lifecycle, and revision invariant.
	pub fn new(project: Project, lead: Agent) -> Result<Self, ProjectError> {
		if lead.role() != AgentRole::Lead || lead.project_id() != Some(project.id()) {
			return Err(ProjectError::InvalidLead);
		}

		let lifecycle_matches = match project.status() {
			ProjectStatus::Active => lead.status() == AgentStatus::Active,
			ProjectStatus::Paused => lead.status() == AgentStatus::Paused,
			ProjectStatus::Archived => lead.status() == AgentStatus::Retired,
		};

		if !lifecycle_matches || lead.revision() != project.revision() {
			return Err(ProjectError::InvalidLead);
		}

		Ok(Self { project, lead })
	}
}

/// Closed scalar retained in bounded Project metadata.
#[derive(Clone, Eq, PartialEq)]
pub enum ProjectMetadataValue {
	/// Bounded ordinary text.
	Text(String),
	/// Non-secret boolean fact.
	Boolean(bool),
}
impl Debug for ProjectMetadataValue {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Text(_) => formatter.write_str("Text(<redacted>)"),
			Self::Boolean(value) => formatter.debug_tuple("Boolean").field(value).finish(),
		}
	}
}

/// Inert Project lifecycle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProjectStatus {
	/// Project owns one active canonical Lead.
	Active,
	/// Project is retained but temporarily inactive.
	Paused,
	/// Project is terminal and retained for readback.
	Archived,
}

/// Closed Project-domain validation failure without caller-controlled text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProjectError {
	/// Project identity was not one canonical UUID version 4.
	InvalidProjectId,
	/// Repository identity was empty, unbounded, or noncanonical.
	InvalidRepositoryIdentity,
	/// Root or cwd was not a bounded normalized absolute server-host path.
	InvalidServerHostPath,
	/// Default cwd was not contained by the repository root.
	DefaultCwdOutsideRepository,
	/// Metadata exceeded its closed field, key, or value bounds.
	InvalidMetadata,
	/// Metadata contained a credential-shaped key or concrete credential value.
	CredentialRejected,
	/// Persisted or incremented revision was outside its positive domain.
	InvalidRevision,
	/// Expected revision did not match current authority.
	RevisionConflict,
	/// Lifecycle transition was not legal.
	InvalidLifecycle,
	/// Canonical Lead did not match the Project invariant.
	InvalidLead,
}
impl Error for ProjectError {}

impl Display for ProjectError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(match self {
			Self::InvalidProjectId => "invalid Project identity",
			Self::InvalidRepositoryIdentity => "invalid repository identity",
			Self::InvalidServerHostPath => "invalid server-host Project path",
			Self::DefaultCwdOutsideRepository => "default cwd is outside the repository root",
			Self::InvalidMetadata => "invalid Project metadata",
			Self::CredentialRejected => "credential-bearing Project metadata rejected",
			Self::InvalidRevision => "invalid Project revision",
			Self::RevisionConflict => "Project revision conflict",
			Self::InvalidLifecycle => "invalid Project lifecycle transition",
			Self::InvalidLead => "invalid canonical Project Lead",
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

fn is_canonical_repository_identity(value: &str) -> bool {
	!value.is_empty()
		&& value.len() <= MAX_REPOSITORY_IDENTITY_BYTES
		&& value.bytes().all(|byte| {
			byte.is_ascii_lowercase()
				|| byte.is_ascii_digit()
				|| matches!(byte, b'-' | b'_' | b'.' | b'/')
		}) && value.split('/').all(|segment| {
		!segment.is_empty()
			&& !matches!(segment, "." | "..")
			&& segment.as_bytes().first().is_some_and(u8::is_ascii_alphanumeric)
			&& segment.as_bytes().last().is_some_and(u8::is_ascii_alphanumeric)
	})
}

fn is_normalized_absolute_host_path(path: &Path) -> bool {
	let encoded = os_bytes(path.as_os_str());
	let lexical_segments_are_canonical =
		encoded.split(|byte| *byte == b'/').all(|segment| segment != b"." && segment != b"..");
	let text_is_control_free =
		path.to_str().is_some_and(|value| !value.chars().any(char::is_control));

	!encoded.is_empty()
		&& encoded.len() <= MAX_PROJECT_PATH_BYTES
		&& text_is_control_free
		&& !encoded.contains(&b'\\')
		&& !encoded.windows(2).any(|pair| pair == b"//")
		&& encoded.last() != Some(&b'/')
		&& lexical_segments_are_canonical
		&& path.is_absolute()
		&& path.parent().is_some()
		&& path
			.components()
			.all(|component| matches!(component, Component::RootDir | Component::Normal(_)))
}

fn os_bytes(value: &OsStr) -> &[u8] {
	value.as_encoded_bytes()
}

#[cfg(test)]
mod tests {
	use std::{collections::BTreeMap, path::PathBuf};

	use crate::{
		Agent, AgentId, AgentStatus, MAX_PROJECT_METADATA_FIELDS, MAX_PROJECT_METADATA_KEY_BYTES,
		MAX_PROJECT_METADATA_VALUE_BYTES, Project, ProjectAuthority, ProjectError, ProjectId,
		ProjectMetadata, ProjectMetadataValue, ProjectRepositoryBinding, ProjectStatus,
		RepositoryIdentity,
	};

	fn binding() -> ProjectRepositoryBinding {
		ProjectRepositoryBinding::new(
			RepositoryIdentity::new("acg-box/decodex").unwrap(),
			PathBuf::from("/srv/repos/decodex"),
			PathBuf::from("/srv/repos/decodex/crates"),
		)
		.unwrap()
	}

	#[test]
	fn project_ids_reject_noncanonical_uuid_shapes_and_versions() {
		for value in [
			"",
			"10000000-0000-4000-8000-00000000000A",
			"10000000000040008000000000000001",
			"10000000-0000-5000-8000-000000000001",
			"10000000-0000-4000-7000-000000000001",
			"not-a-canonical-project-id",
		] {
			assert_eq!(ProjectId::new(value), Err(ProjectError::InvalidProjectId));
		}
	}

	#[test]
	fn repository_binding_is_canonical_server_host_authority() {
		assert_eq!(binding().identity().as_str(), "acg-box/decodex");
		assert!(
			ProjectRepositoryBinding::new(
				RepositoryIdentity::new("acg-box/international").unwrap(),
				PathBuf::from("/srv/répos/décodex"),
				PathBuf::from("/srv/répos/décodex/crates"),
			)
			.is_ok()
		);

		for identity in ["", "Acg-Box/decodex", "acg-box//decodex", "../decodex"] {
			assert_eq!(
				RepositoryIdentity::new(identity),
				Err(ProjectError::InvalidRepositoryIdentity)
			);
		}
		for (root, cwd, error) in [
			("relative", "/srv/repos/decodex", ProjectError::InvalidServerHostPath),
			("/srv/repos/../decodex", "/srv/decodex", ProjectError::InvalidServerHostPath),
			("/srv/repos/decodex", "/srv/repos/other", ProjectError::DefaultCwdOutsideRepository),
		] {
			assert_eq!(
				ProjectRepositoryBinding::new(
					RepositoryIdentity::new("acg-box/decodex").unwrap(),
					PathBuf::from(root),
					PathBuf::from(cwd),
				),
				Err(error)
			);
		}
		for path in [
			"/srv/./repo",
			"/srv/repos/../repo",
			"/srv/repos/line\nfeed",
			"/srv/repos/delete\u{7f}control",
			"/srv/repos/c1\u{85}control",
		] {
			let identity = || RepositoryIdentity::new("acg-box/decodex").unwrap();

			assert_eq!(
				ProjectRepositoryBinding::new(
					identity(),
					PathBuf::from(path),
					PathBuf::from("/srv/repos/decodex"),
				),
				Err(ProjectError::InvalidServerHostPath),
			);
			assert_eq!(
				ProjectRepositoryBinding::new(
					identity(),
					PathBuf::from("/srv/repos/decodex"),
					PathBuf::from(path),
				),
				Err(ProjectError::InvalidServerHostPath),
			);
		}
	}

	#[test]
	fn project_metadata_enforces_closed_bounds() {
		let maximum = (0..MAX_PROJECT_METADATA_FIELDS)
			.map(|index| (format!("field_{index}"), ProjectMetadataValue::Boolean(true)))
			.collect();

		assert!(ProjectMetadata::new(maximum).is_ok());
		assert_eq!(
			ProjectMetadata::new(BTreeMap::from([(
				"x".repeat(MAX_PROJECT_METADATA_KEY_BYTES + 1),
				ProjectMetadataValue::Boolean(true),
			)])),
			Err(ProjectError::InvalidMetadata)
		);
		assert_eq!(
			ProjectMetadata::new(BTreeMap::from([(
				"description".into(),
				ProjectMetadataValue::Text("é".repeat(MAX_PROJECT_METADATA_VALUE_BYTES / 2 + 1)),
			)])),
			Err(ProjectError::InvalidMetadata)
		);
	}

	#[test]
	fn project_metadata_is_credential_negative_and_debug_redacted() {
		for key in ["refresh_token", "password", "authorization", "session_token"] {
			assert_eq!(
				ProjectMetadata::new(BTreeMap::from([(
					key.into(),
					ProjectMetadataValue::Text("ordinary".into()),
				)])),
				Err(ProjectError::CredentialRejected),
			);
		}
		for value in [
			"Bearer abcdefghijklmnop",
			"sk-proj-0123456789abcdef",
			"password=not-for-storage",
			"xoxb-1234567890-abcdef",
		] {
			assert_eq!(
				ProjectMetadata::new(BTreeMap::from([(
					"note".into(),
					ProjectMetadataValue::Text(value.into()),
				)])),
				Err(ProjectError::CredentialRejected),
			);
		}

		assert_eq!(
			ProjectMetadata::new(BTreeMap::from([(
				"note".into(),
				ProjectMetadataValue::Text("before\u{85}after".into()),
			)])),
			Err(ProjectError::InvalidMetadata),
		);

		let metadata = ProjectMetadata::new(BTreeMap::from([
			("note".into(), ProjectMetadataValue::Text("secret sauce".into())),
			("summary".into(), ProjectMetadataValue::Text("token budget".into())),
			("visible".into(), ProjectMetadataValue::Boolean(true)),
		]))
		.expect("ordinary credential-negative metadata remains usable");
		let debug = format!("{metadata:?}");

		assert!(!debug.contains("secret sauce"));
		assert!(!debug.contains("token budget"));
		assert_eq!(
			format!("{:?}", ProjectMetadataValue::Text("private".into())),
			"Text(<redacted>)"
		);
	}

	#[test]
	fn project_and_lead_lifecycle_and_revisions_move_together() {
		let project_id = ProjectId::new("10000000-0000-4000-8000-000000000001").unwrap();
		let mut project = Project::new(project_id.clone(), binding(), ProjectMetadata::empty());
		let mut lead =
			Agent::lead(AgentId::new("20000000-0000-4000-8000-000000000001").unwrap(), project_id);

		assert!(ProjectAuthority::new(project.clone(), lead.clone()).is_ok());
		assert_eq!(
			project.transition(0, ProjectStatus::Paused),
			Err(ProjectError::RevisionConflict)
		);

		project.transition(1, ProjectStatus::Paused).unwrap();
		lead.transition(1, AgentStatus::Paused).unwrap();

		assert_eq!(project.revision(), 2);
		assert!(ProjectAuthority::new(project.clone(), lead.clone()).is_ok());
		assert_eq!(
			project.transition(2, ProjectStatus::Paused),
			Err(ProjectError::InvalidLifecycle)
		);

		project.transition(2, ProjectStatus::Archived).unwrap();
		lead.transition(2, AgentStatus::Retired).unwrap();

		assert!(ProjectAuthority::new(project.clone(), lead).is_ok());
		assert_eq!(
			project.transition(3, ProjectStatus::Active),
			Err(ProjectError::InvalidLifecycle)
		);
	}
}
