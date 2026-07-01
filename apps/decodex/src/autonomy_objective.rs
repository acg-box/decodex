//! Versioned Objective Contract model for project-autonomy authority.

mod contract;

mod lifecycle;

mod validation;

#[allow(unused_imports)]
pub(crate) use contract::{
	AUTONOMY_OBJECTIVE_RECORD_VERSION, AUTONOMY_OBJECTIVE_SCHEMA, AutonomyObjectiveContract,
};

pub(crate) use lifecycle::{
	AutonomyObjectiveAcceptance, AutonomyObjectiveActorKind, AutonomyObjectiveRejection,
	AutonomyObjectiveState, AutonomyObjectiveSupersession,
};

#[cfg(test)] mod tests;
