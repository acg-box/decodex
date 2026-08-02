mod social;
mod validation;

use clap::{Parser, Subcommand};

use crate::{
	cli::{social::SocialCommand, validation::ValidateSocialCommand},
	prelude::Result,
};

/// Root CLI parser for Decodex Publisher.
#[derive(Debug, Parser)]
#[command(
	about = "Hard publication boundaries for Decodex content agents.",
	version,
	arg_required_else_help = true,
	rename_all = "kebab",
	subcommand_required = true
)]
pub(crate) struct Cli {
	#[command(subcommand)]
	command: PublisherSubcommand,
}
impl Cli {
	pub(crate) fn run(&self) -> Result<()> {
		match &self.command {
			PublisherSubcommand::Social(args) => args.run(),
			PublisherSubcommand::ValidateSocial(args) => args.run(),
		}
	}
}

#[derive(Debug, Subcommand)]
enum PublisherSubcommand {
	/// Record content evidence and run bounded social publication workflows.
	Social(Box<SocialCommand>),
	/// Validate Decodex social candidate, reservation, and post artifacts.
	ValidateSocial(ValidateSocialCommand),
}
