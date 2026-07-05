use serde::{Deserialize, Serialize};

use crate::autonomy_objective::AutonomyObjectiveActorKind;

/// Acceptance metadata that turns a draft objective version into authority.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AutonomyObjectiveAcceptance {
	pub(super) accepted_by: String,
	pub(super) accepted_by_kind: AutonomyObjectiveActorKind,
	pub(super) accepted_at: String,
	pub(super) acceptance_source: String,
}

/// Rejection metadata for a draft objective version that did not become authority.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AutonomyObjectiveRejection {
	pub(super) rejected_by: String,
	pub(super) rejected_at: String,
	pub(super) rejection_source: String,
	pub(super) reason: String,
}

/// Supersession metadata linking an older objective version to the replacing version.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AutonomyObjectiveSupersession {
	pub(super) superseded_by_objective_id: String,
	pub(super) superseded_by_version: u64,
	pub(super) superseded_by: String,
	pub(super) superseded_at: String,
	pub(super) supersession_source: String,
	pub(super) reason: String,
}
