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
			MaintenanceSubcommand::InitializeRuntime(args) => args.run(),
		}
	}
}

#[derive(Debug, Args)]
pub(in crate::cli) struct MaintenanceInitializeRuntimeCommand {
	/// Runtime generation to initialize and atomically select.
	#[arg(long, value_name = "NUMBER")]
	generation: u64,
	/// Confirm that the legacy runtime was intentionally reset and only preserved config remains.
	#[arg(long)]
	confirm_empty_reset: bool,
}
impl MaintenanceInitializeRuntimeCommand {
	fn run(&self) -> Result<()> {
		if !self.confirm_empty_reset {
			color_eyre::eyre::bail!(
				"Fresh runtime initialization requires --confirm-empty-reset."
			);
		}
		let database = crate::runtime::initialize_fresh_runtime_generation(self.generation)?;
		println!("initialized runtime generation {} at {}", self.generation, database.display());
		Ok(())
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
	/// Initialize and select a fresh Lane Authority runtime after an explicit offline reset.
	InitializeRuntime(MaintenanceInitializeRuntimeCommand),
	/// Inspect or apply local Decodex storage retention.
	Prune(MaintenancePruneCommand),
}
