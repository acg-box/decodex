use std::path::Path;

use clap::{Args, Subcommand};

use crate::{
	prelude::Result,
	recovery::{self, StaleActiveDiagnoseRequest, StaleActiveReleaseRequest},
};

#[derive(Debug, Args)]
pub(in crate::cli) struct StaleActiveRecoveryCommand {
	#[command(subcommand)]
	pub(in crate::cli) command: StaleActiveRecoverySubcommand,
}
impl StaleActiveRecoveryCommand {
	pub(in crate::cli) fn run(&self, config_path: Option<&Path>) -> Result<()> {
		match &self.command {
			StaleActiveRecoverySubcommand::Diagnose(args) => recovery::run_stale_active_diagnose(
				config_path,
				&StaleActiveDiagnoseRequest { issue: args.issue.clone(), json: args.json },
			),
			StaleActiveRecoverySubcommand::Release(args) => recovery::run_stale_active_release(
				config_path,
				&StaleActiveReleaseRequest { issue: args.issue.clone(), dry_run: args.dry_run },
			),
		}
	}
}

#[derive(Debug, Args)]
pub(in crate::cli) struct StaleActiveDiagnoseCommand {
	/// Issue identifier or tracker issue id to inspect. Omit to inspect active-label issues.
	#[arg(value_name = "ISSUE")]
	pub(in crate::cli) issue: Option<String>,
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	pub(in crate::cli) json: bool,
}

#[derive(Debug, Args)]
pub(in crate::cli) struct StaleActiveReleaseCommand {
	/// Issue identifier or tracker issue id for the stale active lane.
	#[arg(value_name = "ISSUE")]
	pub(in crate::cli) issue: String,
	/// Validate only; do not clear labels, terminalize the run, or write private audit evidence.
	#[arg(long)]
	pub(in crate::cli) dry_run: bool,
}

#[derive(Debug, Subcommand)]
pub(in crate::cli) enum StaleActiveRecoverySubcommand {
	/// Read-only diagnosis for tracker-present stale active ownership.
	Diagnose(StaleActiveDiagnoseCommand),
	/// Clear a proven stale active label and terminalize stale local ownership.
	Release(StaleActiveReleaseCommand),
}
