use std::{
	error::Error,
	fmt::{Display, Formatter},
	future::Future,
};

use crate::{ProjectId, ProjectStatus};

/// Application port for the one global Advisor authority.
pub trait AgentRepository {
	/// Adapter-owned error.
	type Error: Error + Send + Sync + 'static;

	/// Idempotently bootstrap or deterministically read the one global Advisor.
	fn bootstrap_advisor(
		&self,
		advisor: Agent,
	) -> impl Future<Output = Result<Agent, Self::Error>> + Send;

	/// Deterministically read the global Advisor, when bootstrapped.
	fn advisor(&self) -> impl Future<Output = Result<Option<Agent>, Self::Error>> + Send;

	/// Deterministically read one stable Agent identity.
	fn agent(
		&self,
		id: &AgentId,
	) -> impl Future<Output = Result<Option<Agent>, Self::Error>> + Send;
}

/// Stable canonical Agent identity.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AgentId(String);
impl AgentId {
	/// Parse one canonical lowercase RFC 9562 UUID version 4 identity.
	pub fn new(value: impl Into<String>) -> Result<Self, AgentError> {
		let value = value.into();

		if !is_canonical_uuid_v4(&value) {
			return Err(AgentError::InvalidAgentId);
		}

		Ok(Self(value))
	}

	/// Borrow the canonical Agent identity.
	pub fn as_str(&self) -> &str {
		&self.0
	}
}

impl Display for AgentId {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(&self.0)
	}
}

/// Stable inert Agent authority with no behavior or Conversation binding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Agent {
	id: AgentId,
	role: AgentRole,
	project_id: Option<ProjectId>,
	status: AgentStatus,
	revision: u64,
}
impl Agent {
	/// Create revision one of the global active Advisor identity.
	pub fn advisor(id: AgentId) -> Self {
		Self {
			id,
			role: AgentRole::Advisor,
			project_id: None,
			status: AgentStatus::Active,
			revision: 1,
		}
	}

	/// Create revision one of one Project's canonical active Lead identity.
	pub fn lead(id: AgentId, project_id: ProjectId) -> Self {
		Self {
			id,
			role: AgentRole::Lead,
			project_id: Some(project_id),
			status: AgentStatus::Active,
			revision: 1,
		}
	}

	/// Validate deterministic persistence readback without enabling arbitrary roles.
	pub fn from_stored(
		id: AgentId,
		role: AgentRole,
		project_id: Option<ProjectId>,
		status: AgentStatus,
		revision: u64,
	) -> Result<Self, AgentError> {
		if revision == 0 {
			return Err(AgentError::InvalidRevision);
		}
		if !matches!(
			(role, project_id.as_ref()),
			(AgentRole::Advisor, None) | (AgentRole::Lead, Some(_))
		) {
			return Err(AgentError::InvalidRoleProjectBinding);
		}

		Ok(Self { id, role, project_id, status, revision })
	}

	/// Stable identity.
	pub const fn id(&self) -> &AgentId {
		&self.id
	}

	/// Closed role.
	pub const fn role(&self) -> AgentRole {
		self.role
	}

	/// Owning Project for a Lead; always absent for the global Advisor.
	pub const fn project_id(&self) -> Option<&ProjectId> {
		self.project_id.as_ref()
	}

	/// Current inert lifecycle.
	pub const fn status(&self) -> AgentStatus {
		self.status
	}

	/// Positive optimistic revision.
	pub const fn revision(&self) -> u64 {
		self.revision
	}

	/// Apply one legal expected-revision lifecycle transition.
	pub fn transition(
		&mut self,
		expected_revision: u64,
		status: AgentStatus,
	) -> Result<(), AgentError> {
		if expected_revision == 0 || expected_revision != self.revision {
			return Err(AgentError::RevisionConflict);
		}
		if status == self.status
			|| !matches!(
				(self.status, status),
				(AgentStatus::Active, AgentStatus::Paused | AgentStatus::Retired)
					| (AgentStatus::Paused, AgentStatus::Active | AgentStatus::Retired)
			) {
			return Err(AgentError::InvalidLifecycle);
		}

		self.revision = self.revision.checked_add(1).ok_or(AgentError::InvalidRevision)?;
		self.status = status;

		Ok(())
	}
}

/// Closed stable Agent role authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentRole {
	/// One global advisory-only identity with no Project binding.
	Advisor,
	/// One canonical identity bound to one Project.
	Lead,
}

/// Inert Agent lifecycle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentStatus {
	/// Identity is current for its role.
	Active,
	/// Identity is retained but temporarily inactive.
	Paused,
	/// Identity is terminal and retained for readback.
	Retired,
}

/// Closed Agent-domain validation failure without caller-controlled text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentError {
	/// Agent identity was not one canonical UUID version 4.
	InvalidAgentId,
	/// Advisor or Lead Project binding violated the closed role invariant.
	InvalidRoleProjectBinding,
	/// Persisted or incremented revision was outside its positive domain.
	InvalidRevision,
	/// Expected revision did not match current authority.
	RevisionConflict,
	/// Lifecycle transition was not legal.
	InvalidLifecycle,
}
impl Error for AgentError {}

impl Display for AgentError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(match self {
			Self::InvalidAgentId => "invalid Agent identity",
			Self::InvalidRoleProjectBinding => "invalid Agent role and Project binding",
			Self::InvalidRevision => "invalid Agent revision",
			Self::RevisionConflict => "Agent revision conflict",
			Self::InvalidLifecycle => "invalid Agent lifecycle transition",
		})
	}
}

/// Map one Project lifecycle to the required canonical Lead lifecycle.
pub const fn lead_status_for_project(status: ProjectStatus) -> AgentStatus {
	match status {
		ProjectStatus::Active => AgentStatus::Active,
		ProjectStatus::Paused => AgentStatus::Paused,
		ProjectStatus::Archived => AgentStatus::Retired,
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
	use crate::{Agent, AgentError, AgentId, AgentRole, AgentStatus, ProjectId};

	#[test]
	fn agent_ids_reject_noncanonical_uuid_shapes_versions_and_placeholders() {
		for value in [
			"",
			"20000000-0000-4000-8000-00000000000A",
			"20000000000040008000000000000001",
			"20000000-0000-5000-8000-000000000001",
			"20000000-0000-4000-c000-000000000001",
			"not-a-canonical-agent-id",
		] {
			assert_eq!(AgentId::new(value), Err(AgentError::InvalidAgentId));
		}
	}

	#[test]
	fn advisor_is_global_and_lead_requires_exactly_one_project() {
		let advisor_id = AgentId::new("20000000-0000-4000-8000-000000000001").unwrap();
		let lead_id = AgentId::new("20000000-0000-4000-8000-000000000002").unwrap();
		let project_id = ProjectId::new("10000000-0000-4000-8000-000000000001").unwrap();
		let advisor = Agent::advisor(advisor_id.clone());
		let lead = Agent::lead(lead_id, project_id.clone());

		assert_eq!(advisor.role(), AgentRole::Advisor);
		assert_eq!(advisor.project_id(), None);
		assert_eq!(lead.project_id(), Some(&project_id));
		assert_eq!(
			Agent::from_stored(
				advisor_id,
				AgentRole::Advisor,
				Some(project_id),
				AgentStatus::Active,
				1,
			),
			Err(AgentError::InvalidRoleProjectBinding)
		);
	}

	#[test]
	fn retired_agent_is_terminal_and_revisions_are_optimistic() {
		let mut advisor =
			Agent::advisor(AgentId::new("20000000-0000-4000-8000-000000000001").unwrap());

		assert_eq!(advisor.transition(2, AgentStatus::Retired), Err(AgentError::RevisionConflict));

		advisor.transition(1, AgentStatus::Retired).unwrap();

		assert_eq!(advisor.revision(), 2);
		assert_eq!(advisor.transition(2, AgentStatus::Active), Err(AgentError::InvalidLifecycle));
	}
}
