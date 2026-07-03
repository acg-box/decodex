use clap::Args;

use crate::{
	cli::ProjectConfigArgs,
	orchestrator::{self, DiagnoseRequest},
	prelude::Result,
};

#[derive(Debug, Args)]
pub(in crate::cli) struct DiagnoseCommand {
	#[command(flatten)]
	pub(in crate::cli) project_config: ProjectConfigArgs,
	/// Emit the agent handoff index JSON instead of a one-line path summary.
	#[arg(long)]
	pub(in crate::cli) json: bool,
	/// Maximum number of recent runs to include while generating evidence.
	#[arg(long, value_name = "COUNT", default_value_t = orchestrator::DEFAULT_STATUS_RUN_LIMIT)]
	pub(in crate::cli) limit: usize,
}
impl DiagnoseCommand {
	pub(in crate::cli) fn run(&self) -> Result<()> {
		orchestrator::run_diagnose(DiagnoseRequest {
			config_path: self.project_config.as_path(),
			json: self.json,
			limit: self.limit,
		})
	}
}
