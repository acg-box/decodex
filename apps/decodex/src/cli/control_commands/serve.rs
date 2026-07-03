use clap::Args;

use crate::{
	cli::ProjectConfigArgs,
	orchestrator::{self, ServeRequest},
	prelude::Result,
};

#[derive(Debug, Args)]
pub(in crate::cli) struct ServeCommand {
	#[command(flatten)]
	pub(in crate::cli) project_config: ProjectConfigArgs,
	/// Operator UI listen address.
	#[arg(long, value_name = "ADDR", default_value_t = orchestrator::DEFAULT_OPERATOR_LISTEN_ADDRESS.to_owned())]
	pub(in crate::cli) listen_address: String,
	/// Start the local dev endpoint without polling or dispatching projects.
	#[arg(long, hide = true)]
	pub(in crate::cli) dev: bool,
}
impl ServeCommand {
	pub(in crate::cli) fn run(&self) -> Result<()> {
		orchestrator::run_control_plane(ServeRequest {
			config_path: self.project_config.as_path(),
			listen_address: &self.listen_address,
			dev: self.dev,
		})
	}
}
