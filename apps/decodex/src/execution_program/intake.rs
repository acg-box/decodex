//! Program Intake Plan metadata for execution programs.

mod plan;
mod validation;

pub(crate) use self::plan::{
	PROGRAM_INTAKE_PLAN_RECORD_VERSION, PROGRAM_INTAKE_PLAN_SCHEMA, ProgramIntakeKind,
	ProgramIntakePlan,
};
