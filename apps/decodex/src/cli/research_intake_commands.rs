//! Research, intake, archive, and maintenance CLI command definitions.

mod archive;
mod intake;
mod maintenance;
mod research;

pub(in crate::cli) use self::{
	archive::ArchiveLinearCommand, intake::IntakeCommand, maintenance::MaintenanceCommand,
	research::ResearchCommand,
};
#[cfg(test)]
pub(in crate::cli) use self::{
	intake::{IntakeGoalCommand, IntakeIssuesCommand, IntakeSubcommand},
	research::{
		ResearchCompileCommand, ResearchOutcomeArg, ResearchPromoteCommand, ResearchSubcommand,
	},
};
