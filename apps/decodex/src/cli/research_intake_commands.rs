//! Intake, archive, and maintenance CLI command definitions.

mod archive;
mod intake;
mod maintenance;

#[cfg(test)]
pub(in crate::cli) use self::intake::{
	IntakeGoalCommand, IntakeIssuesCommand, IntakeRecoverAction, IntakeRecoverCommand,
	IntakeSubcommand,
};
pub(in crate::cli) use self::{
	archive::ArchiveLinearCommand, intake::IntakeCommand, maintenance::MaintenanceCommand,
};
