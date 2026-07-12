//! Canonical project and lane identities for the Lane Authority v2 runtime.

mod kernel;

pub use kernel::{LaneAggregate, LaneCommand, LanePhase, LaneTransitionRejection, transition};

use crate::prelude::{Result, eyre};

/// Immutable project identity attested at registration and checked at admission.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ProjectBinding {
	project_key: String,
	github_owner: String,
	github_repository: String,
	tracker_team_id: String,
	routing_label: String,
	config_fingerprint: String,
}
impl ProjectBinding {
	pub(crate) fn from_validated_parts(
		project_key: &str,
		github_owner: &str,
		github_repository: &str,
		tracker_team_id: &str,
		routing_label: &str,
		config_fingerprint: &str,
	) -> Self {
		Self {
			project_key: project_key.to_owned(),
			github_owner: github_owner.to_owned(),
			github_repository: github_repository.to_owned(),
			tracker_team_id: tracker_team_id.to_owned(),
			routing_label: routing_label.to_owned(),
			config_fingerprint: config_fingerprint.to_owned(),
		}
	}

	/// Build a complete binding. Empty identity components are rejected.
	pub fn new(
		project_key: &str,
		github_owner: &str,
		github_repository: &str,
		tracker_team_id: &str,
		routing_label: &str,
		config_fingerprint: &str,
	) -> Result<Self> {
		for (field, value) in [
			("project_key", project_key),
			("github_owner", github_owner),
			("github_repository", github_repository),
			("tracker_team_id", tracker_team_id),
			("routing_label", routing_label),
			("config_fingerprint", config_fingerprint),
		] {
			if value.trim().is_empty() {
				eyre::bail!("Project binding `{field}` cannot be empty.");
			}
		}

		Ok(Self::from_validated_parts(
			project_key,
			github_owner,
			github_repository,
			tracker_team_id,
			routing_label,
			config_fingerprint,
		))
	}

	pub fn project_key(&self) -> &str {
		&self.project_key
	}

	pub fn github_owner(&self) -> &str {
		&self.github_owner
	}

	pub fn github_repository(&self) -> &str {
		&self.github_repository
	}

	pub fn tracker_team_id(&self) -> &str {
		&self.tracker_team_id
	}

	pub fn routing_label(&self) -> &str {
		&self.routing_label
	}

	pub fn config_fingerprint(&self) -> &str {
		&self.config_fingerprint
	}
}

/// Sole identity for one tracker lane within one project.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct LaneId {
	project_key: String,
	tracker_issue_id: String,
}
impl LaneId {
	pub fn new(project_key: &str, tracker_issue_id: &str) -> Result<Self> {
		if project_key.trim().is_empty() || tracker_issue_id.trim().is_empty() {
			eyre::bail!("Lane identity requires non-empty project and tracker issue ids.");
		}

		Ok(Self {
			project_key: project_key.to_owned(),
			tracker_issue_id: tracker_issue_id.to_owned(),
		})
	}

	pub fn project_key(&self) -> &str {
		&self.project_key
	}

	pub fn tracker_issue_id(&self) -> &str {
		&self.tracker_issue_id
	}
}

#[cfg(test)]
mod tests {
	use super::{LaneId, ProjectBinding};

	#[test]
	fn lane_identity_is_project_qualified() {
		let left = LaneId::new("pubfi", "issue-1").expect("lane");
		let right = LaneId::new("decodex", "issue-1").expect("lane");
		assert_ne!(left, right);
	}

	#[test]
	fn project_binding_rejects_incomplete_identity() {
		let error = ProjectBinding::new("pubfi", "helixbox", "", "team", "queue", "fp")
			.expect_err("missing repository must fail");
		assert!(error.to_string().contains("github_repository"));
	}
}
