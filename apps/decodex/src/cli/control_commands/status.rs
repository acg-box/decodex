use clap::Args;

use crate::{cli::ProjectConfigArgs, orchestrator, prelude::Result};

#[derive(Debug, Args)]
pub(in crate::cli) struct StatusCommand {
	#[command(flatten)]
	pub(in crate::cli) project_config: ProjectConfigArgs,
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	pub(in crate::cli) json: bool,
	/// Maximum number of recent runs to display.
	#[arg(long, value_name = "COUNT", default_value_t = orchestrator::DEFAULT_STATUS_RUN_LIMIT)]
	pub(in crate::cli) limit: usize,
	/// Refresh live tracker and pull-request observers before printing status.
	#[arg(long)]
	pub(in crate::cli) live: bool,
}
impl StatusCommand {
	pub(in crate::cli) fn run(&self) -> Result<()> {
		orchestrator::print_status(self.project_config.as_path(), self.json, self.limit, self.live)
	}
}
