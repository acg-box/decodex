use clap::{Args, Subcommand};

use crate::{
	maintenance::{self, MaintenanceMode, MaintenancePruneRequest, MaintenanceScope},
	prelude::Result,
};

#[derive(Debug, Args)]
pub(in crate::cli) struct MaintenanceCommand {
	#[command(subcommand)]
	pub(in crate::cli) command: MaintenanceSubcommand,
}
impl MaintenanceCommand {
	pub(in crate::cli) fn run(&self) -> Result<()> {
		match &self.command {
			MaintenanceSubcommand::Prune(args) => args.run(),
		}
	}
}

#[derive(Debug, Args)]
pub(in crate::cli) struct MaintenancePruneCommand {
	/// Report candidates without applying retention changes. This is the default mode.
	#[arg(long, conflicts_with = "apply")]
	dry_run: bool,
	/// Apply safe file retention, state-aware runtime compaction, and WAL checkpointing.
	#[arg(long, conflicts_with = "dry_run")]
	apply: bool,
	/// Emit the maintenance report as JSON.
	#[arg(long)]
	json: bool,
}
impl MaintenancePruneCommand {
	fn run(&self) -> Result<()> {
		let mode = if self.apply { MaintenanceMode::Apply } else { MaintenanceMode::DryRun };

		maintenance::run_prune_command(MaintenancePruneRequest {
			mode,
			scope: MaintenanceScope::Full,
			json: self.json,
		})
	}
}

#[derive(Debug, Subcommand)]
pub(in crate::cli) enum MaintenanceSubcommand {
	/// Inspect or apply local Decodex storage retention.
	Prune(MaintenancePruneCommand),
}
