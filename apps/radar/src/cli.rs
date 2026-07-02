mod commands;

use clap::Parser;

use crate::{cli::commands::RadarSubcommand, prelude::Result};

/// Root CLI parser for the Radar auxiliary tool.
#[derive(Debug, Parser)]
#[command(
	about = "Auxiliary Radar automation and artifact tooling.",
	version,
	arg_required_else_help = true,
	rename_all = "kebab",
	subcommand_required = true
)]
pub(crate) struct Cli {
	#[command(subcommand)]
	command: RadarSubcommand,
}
impl Cli {
	pub(crate) fn run(&self) -> Result<()> {
		self.command.run()
	}
}
