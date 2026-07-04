use std::path::Path;

use clap::{Args, Subcommand};

use crate::{
	prelude::Result,
	recovery::{self, GhostLaneCleanupRequest, GhostLaneDiagnoseRequest},
};

#[derive(Debug, Args)]
pub(in crate::cli) struct GhostLaneRecoveryCommand {
	#[command(subcommand)]
	pub(in crate::cli) command: GhostLaneRecoverySubcommand,
}
impl GhostLaneRecoveryCommand {
	pub(in crate::cli) fn run(&self, config_path: Option<&Path>) -> Result<()> {
		match &self.command {
			GhostLaneRecoverySubcommand::Diagnose(args) => recovery::run_ghost_lane_diagnose(
				config_path,
				&GhostLaneDiagnoseRequest { issue: args.issue.clone(), json: args.json },
			),
			GhostLaneRecoverySubcommand::Cleanup(args) => recovery::run_ghost_lane_cleanup(
				config_path,
				&GhostLaneCleanupRequest { issue: args.issue.clone(), dry_run: args.dry_run },
			),
		}
	}
}

#[derive(Debug, Args)]
pub(in crate::cli) struct GhostLaneDiagnoseCommand {
	/// Issue identifier or local issue id to inspect. Omit to inspect leased lanes.
	#[arg(value_name = "ISSUE")]
	pub(in crate::cli) issue: Option<String>,
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	pub(in crate::cli) json: bool,
}

#[derive(Debug, Args)]
pub(in crate::cli) struct GhostLaneCleanupCommand {
	/// Issue identifier or local issue id for the ghost lane.
	#[arg(value_name = "ISSUE")]
	pub(in crate::cli) issue: String,
	/// Validate only; do not terminalize the run lease or write private audit evidence.
	#[arg(long)]
	pub(in crate::cli) dry_run: bool,
}

#[derive(Debug, Subcommand)]
pub(in crate::cli) enum GhostLaneRecoverySubcommand {
	/// Read-only diagnosis for local leases whose tracker issue is missing.
	Diagnose(GhostLaneDiagnoseCommand),
	/// Terminalize a proven ghost lane and clear its local run lease.
	Cleanup(GhostLaneCleanupCommand),
}
