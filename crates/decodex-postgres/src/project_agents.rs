use std::{collections::BTreeMap, fmt::Display, path::PathBuf};

use serde_json::{Map, Value};
use tokio_postgres::{Error, Row, error::DbError};

use crate::{CreateProject, PostgresStore, StoreError};
use decodex_core::{
	Agent, AgentId, AgentRepository, AgentRole, AgentStatus, Project, ProjectAuthority, ProjectId,
	ProjectMetadata, ProjectMetadataValue, ProjectRepository, ProjectRepositoryBinding,
	ProjectStatus, RepositoryIdentity,
};

impl PostgresStore {
	/// Idempotently bootstrap or read the one global inert Advisor identity.
	pub async fn bootstrap_advisor(&self, advisor: Agent) -> Result<Agent, StoreError> {
		if advisor.role() != AgentRole::Advisor
			|| advisor.project_id().is_some()
			|| advisor.status() != AgentStatus::Active
			|| advisor.revision() != 1
		{
			return Err(StoreError::InvalidInput(
				"Advisor bootstrap requires revision-one global active authority",
			));
		}

		let client = crate::checkout(self.pool(), &self.connector).await?;
		let row = client
			.query_one(
				"SELECT agent_id::text, role::text, project_id::text, status::text, revision \
				 FROM decodex.bootstrap_advisor(\
				 $1::pg_catalog.text::decodex.canonical_uuid_v4_text)",
				&[&advisor.id().as_str()],
			)
			.await?;

		decode_agent(&row, 0)
	}

	/// Atomically create an active Project and its one canonical active Lead.
	pub async fn create_project(
		&self,
		request: CreateProject,
	) -> Result<ProjectAuthority, StoreError> {
		let CreateProject { project, lead } = request;

		ProjectAuthority::new(project.clone(), lead.clone()).map_err(|_| {
			StoreError::InvalidInput("Project creation requires matching revision-one active Lead")
		})?;

		if project.status() != ProjectStatus::Active || project.revision() != 1 {
			return Err(StoreError::InvalidInput(
				"Project creation requires revision-one active authority",
			));
		}

		let root = project
			.repository()
			.root()
			.as_server_path()
			.to_str()
			.ok_or(StoreError::InvalidInput("Project paths must be UTF-8"))?;
		let cwd = project
			.repository()
			.default_cwd()
			.as_server_path()
			.to_str()
			.ok_or(StoreError::InvalidInput("Project paths must be UTF-8"))?;
		let metadata = encode_metadata(project.metadata());

		crate::ensure_credential_negative_json(&metadata)?;

		let client = crate::checkout(self.pool(), &self.connector).await?;
		let row = client
			.query_one(
				"SELECT project_id::text, repository_identity, repository_root, default_cwd, \
				 project_status::text, metadata, project_revision, agent_id::text, \
				 agent_role::text, agent_status::text, agent_revision \
				 FROM decodex.create_project(\
				 $1::pg_catalog.text::decodex.canonical_uuid_v4_text,$2,$3,$4,$5,\
				 $6::pg_catalog.text::decodex.canonical_uuid_v4_text)",
				&[
					&project.id().as_str(),
					&project.repository().identity().as_str(),
					&root,
					&cwd,
					&metadata,
					&lead.id().as_str(),
				],
			)
			.await
			.map_err(project_agent_database_error)?;

		decode_project_authority(&row)
	}

	/// Deterministically read one Project with its canonical Lead.
	pub async fn project(&self, id: &ProjectId) -> Result<Option<ProjectAuthority>, StoreError> {
		let client = crate::checkout(self.pool(), &self.connector).await?;
		let row = client
			.query_opt(
				"SELECT project.project_id::text, project.repository_identity, \
				 project.repository_root, project.default_cwd, project.status::text, \
				 project.metadata, project.revision, lead.agent_id::text, lead.role::text, \
				 lead.status::text, lead.revision \
				 FROM decodex.projects AS project \
				 JOIN decodex.agents AS lead ON lead.project_id=project.project_id AND lead.role='lead' \
				 WHERE project.project_id=$1::text::uuid",
				&[&id.as_str()],
			)
			.await?;

		row.as_ref().map(decode_project_authority).transpose()
	}

	/// Atomically transition one Project and its canonical Lead.
	pub async fn transition_project(
		&self,
		id: &ProjectId,
		expected_revision: u64,
		status: ProjectStatus,
	) -> Result<ProjectAuthority, StoreError> {
		let expected_revision = i64::try_from(expected_revision)
			.map_err(|_| StoreError::InvalidInput("Project revision exceeds PostgreSQL bigint"))?;
		let client = crate::checkout(self.pool(), &self.connector).await?;
		let row = client
			.query_opt(
				"SELECT project_id::text, repository_identity, repository_root, default_cwd, \
				 project_status::text, metadata, project_revision, agent_id::text, \
				 agent_role::text, agent_status::text, agent_revision \
				 FROM decodex.transition_project(\
				 $1::pg_catalog.text::decodex.canonical_uuid_v4_text,$2,\
				 $3::text::decodex.project_status)",
				&[&id.as_str(), &expected_revision, &project_status_text(status)],
			)
			.await?;

		if let Some(row) = row {
			return decode_project_authority(&row);
		}

		let actual = client
			.query_opt(
				"SELECT revision FROM decodex.projects WHERE project_id=$1::text::uuid",
				&[&id.as_str()],
			)
			.await?
			.map(|row| row.get(0));

		Err(StoreError::RevisionConflict {
			entity: id.to_string(),
			expected: Some(expected_revision),
			actual,
		})
	}

	/// Deterministically read the global Advisor.
	pub async fn advisor(&self) -> Result<Option<Agent>, StoreError> {
		let client = crate::checkout(self.pool(), &self.connector).await?;
		let row = client
			.query_opt(
				"SELECT agent_id::text, role::text, project_id::text, status::text, revision \
				 FROM decodex.agents WHERE role='advisor'",
				&[],
			)
			.await?;

		row.as_ref().map(|row| decode_agent(row, 0)).transpose()
	}

	/// Deterministically read one stable Agent identity.
	pub async fn agent(&self, id: &AgentId) -> Result<Option<Agent>, StoreError> {
		let client = crate::checkout(self.pool(), &self.connector).await?;
		let row = client
			.query_opt(
				"SELECT agent_id::text, role::text, project_id::text, status::text, revision \
				 FROM decodex.agents WHERE agent_id=$1::text::uuid",
				&[&id.as_str()],
			)
			.await?;

		row.as_ref().map(|row| decode_agent(row, 0)).transpose()
	}
}

impl ProjectRepository for PostgresStore {
	type Error = StoreError;

	async fn create_project(
		&self,
		project: Project,
		lead: Agent,
	) -> Result<ProjectAuthority, Self::Error> {
		Self::create_project(self, CreateProject { project, lead }).await
	}

	async fn project(&self, id: &ProjectId) -> Result<Option<ProjectAuthority>, Self::Error> {
		Self::project(self, id).await
	}

	async fn transition_project(
		&self,
		id: &ProjectId,
		expected_revision: u64,
		status: ProjectStatus,
	) -> Result<ProjectAuthority, Self::Error> {
		Self::transition_project(self, id, expected_revision, status).await
	}
}

impl AgentRepository for PostgresStore {
	type Error = StoreError;

	async fn bootstrap_advisor(&self, advisor: Agent) -> Result<Agent, Self::Error> {
		Self::bootstrap_advisor(self, advisor).await
	}

	async fn advisor(&self) -> Result<Option<Agent>, Self::Error> {
		Self::advisor(self).await
	}

	async fn agent(&self, id: &AgentId) -> Result<Option<Agent>, Self::Error> {
		Self::agent(self, id).await
	}
}

fn decode_project_authority(row: &Row) -> Result<ProjectAuthority, StoreError> {
	let project_id = ProjectId::new(row.get::<_, String>(0)).map_err(incompatible_core)?;
	let repository = ProjectRepositoryBinding::new(
		RepositoryIdentity::new(row.get::<_, String>(1)).map_err(incompatible_core)?,
		PathBuf::from(row.get::<_, String>(2)),
		PathBuf::from(row.get::<_, String>(3)),
	)
	.map_err(incompatible_core)?;
	let project = Project::from_stored(
		project_id.clone(),
		repository,
		decode_project_status(row.get(4))?,
		decode_metadata(row.get(5))?,
		u64::try_from(row.get::<_, i64>(6)).map_err(|_| incompatible())?,
	)
	.map_err(incompatible_core)?;
	let lead = Agent::from_stored(
		AgentId::new(row.get::<_, String>(7)).map_err(incompatible_core)?,
		decode_agent_role(row.get(8))?,
		Some(project_id),
		decode_agent_status(row.get(9))?,
		u64::try_from(row.get::<_, i64>(10)).map_err(|_| incompatible())?,
	)
	.map_err(incompatible_core)?;

	ProjectAuthority::new(project, lead).map_err(incompatible_core)
}

fn decode_agent(row: &Row, offset: usize) -> Result<Agent, StoreError> {
	let project_id = row
		.get::<_, Option<String>>(offset + 2)
		.map(ProjectId::new)
		.transpose()
		.map_err(incompatible_core)?;

	Agent::from_stored(
		AgentId::new(row.get::<_, String>(offset)).map_err(incompatible_core)?,
		decode_agent_role(row.get(offset + 1))?,
		project_id,
		decode_agent_status(row.get(offset + 3))?,
		u64::try_from(row.get::<_, i64>(offset + 4)).map_err(|_| incompatible())?,
	)
	.map_err(incompatible_core)
}

fn encode_metadata(metadata: &ProjectMetadata) -> Value {
	Value::Object(
		metadata
			.as_map()
			.iter()
			.map(|(key, value)| {
				let value = match value {
					ProjectMetadataValue::Text(value) => Value::String(value.clone()),
					ProjectMetadataValue::Boolean(value) => Value::Bool(*value),
				};

				(key.clone(), value)
			})
			.collect::<Map<_, _>>(),
	)
}

fn decode_metadata(value: Value) -> Result<ProjectMetadata, StoreError> {
	let Value::Object(value) = value else { return Err(incompatible()) };
	let values = value
		.into_iter()
		.map(|(key, value)| {
			let value = match value {
				Value::String(value) => ProjectMetadataValue::Text(value),
				Value::Bool(value) => ProjectMetadataValue::Boolean(value),
				_ => return Err(incompatible()),
			};

			Ok((key, value))
		})
		.collect::<Result<BTreeMap<_, _>, StoreError>>()?;

	ProjectMetadata::new(values).map_err(incompatible_core)
}

fn decode_project_status(value: &str) -> Result<ProjectStatus, StoreError> {
	match value {
		"active" => Ok(ProjectStatus::Active),
		"paused" => Ok(ProjectStatus::Paused),
		"archived" => Ok(ProjectStatus::Archived),
		_ => Err(incompatible()),
	}
}

const fn project_status_text(value: ProjectStatus) -> &'static str {
	match value {
		ProjectStatus::Active => "active",
		ProjectStatus::Paused => "paused",
		ProjectStatus::Archived => "archived",
	}
}

fn decode_agent_role(value: &str) -> Result<AgentRole, StoreError> {
	match value {
		"advisor" => Ok(AgentRole::Advisor),
		"lead" => Ok(AgentRole::Lead),
		_ => Err(incompatible()),
	}
}

fn decode_agent_status(value: &str) -> Result<AgentStatus, StoreError> {
	match value {
		"active" => Ok(AgentStatus::Active),
		"paused" => Ok(AgentStatus::Paused),
		"retired" => Ok(AgentStatus::Retired),
		_ => Err(incompatible()),
	}
}

fn incompatible_core(error: impl Display) -> StoreError {
	StoreError::Incompatible(format!("invalid stored Project/Agent authority: {error}"))
}

fn incompatible() -> StoreError {
	StoreError::Incompatible("invalid stored Project/Agent authority".into())
}

fn project_agent_database_error(error: Error) -> StoreError {
	if error.as_db_error().and_then(DbError::constraint) == Some("projects_identity_pair") {
		StoreError::InvalidInput("Project and repository identities are already bound differently")
	} else {
		StoreError::from(error)
	}
}
