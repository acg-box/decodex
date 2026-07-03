//! Objective Contract payload and lifecycle transitions.

mod accessors;
mod transitions;
mod validation_impl;

use serde::{Deserialize, Serialize};

use crate::autonomy_objective::lifecycle::{
	AutonomyObjectiveAcceptance, AutonomyObjectiveRejection, AutonomyObjectiveState,
	AutonomyObjectiveSupersession,
};

pub(crate) const AUTONOMY_OBJECTIVE_SCHEMA: &str = "decodex.autonomy_objective/1";
pub(crate) const AUTONOMY_OBJECTIVE_RECORD_VERSION: u16 = 1;

/// Versioned project-level Objective Contract payload.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AutonomyObjectiveContract {
	#[serde(default = "autonomy_objective_schema")]
	schema: String,
	#[serde(default = "autonomy_objective_record_version")]
	record_version: u16,
	project_id: String,
	id: String,
	version: u64,
	state: AutonomyObjectiveState,
	summary: String,
	#[serde(default)]
	goals: Vec<String>,
	#[serde(default)]
	non_goals: Vec<String>,
	#[serde(default)]
	metrics: Vec<String>,
	#[serde(default)]
	allowed_surfaces: Vec<String>,
	#[serde(default)]
	allowed_signal_kinds: Vec<String>,
	#[serde(default)]
	validation_gates: Vec<String>,
	review_policy: String,
	memory_policy: String,
	report_policy: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	acceptance: Option<AutonomyObjectiveAcceptance>,
	#[serde(skip_serializing_if = "Option::is_none")]
	rejection: Option<AutonomyObjectiveRejection>,
	#[serde(skip_serializing_if = "Option::is_none")]
	supersession: Option<AutonomyObjectiveSupersession>,
}
pub(in crate::autonomy_objective::contract) fn autonomy_objective_schema() -> String {
	AUTONOMY_OBJECTIVE_SCHEMA.to_owned()
}

pub(in crate::autonomy_objective::contract) const fn autonomy_objective_record_version() -> u16 {
	AUTONOMY_OBJECTIVE_RECORD_VERSION
}
