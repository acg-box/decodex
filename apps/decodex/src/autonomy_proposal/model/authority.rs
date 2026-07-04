use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AutonomyProposalAuthorityActorKind {
	User,
	RuntimePolicy,
	ExternalAgent,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AutonomyProposalAcceptedProjectPolicy {
	pub(crate) project_id: String,
	pub(crate) objective_id: String,
	pub(crate) objective_version: u64,
	pub(crate) accepted_policy_id: String,
	pub(crate) accepted_policy_version: String,
	pub(crate) authority_ref: String,
	pub(crate) authorized_actor: String,
	pub(crate) authorized_actor_kind: AutonomyProposalAuthorityActorKind,
	pub(crate) authorized_acceptance_sources: Vec<String>,
	pub(crate) authorized_scopes: Vec<String>,
}
