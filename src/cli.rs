use clap::{
	Parser,
	builder::{
		Styles,
		styling::{AnsiColor, Effects},
	},
};

use crate::prelude::Result;

/// Cli.
#[derive(Debug, Parser)]
#[command(
	version = concat!(
		env!("CARGO_PKG_VERSION"),
		"-",
		env!("VERGEN_GIT_SHA"),
		"-",
		env!("VERGEN_CARGO_TARGET_TRIPLE"),
	),
	rename_all = "kebab",
	styles = styles(),
)]
pub struct Cli {
	/// Placeholder.
	#[arg(long, short, value_name = "NUM", default_value_t = String::from("Welcome to use name_placeholder!"))]
	placeholder: String,
}
impl Cli {
	pub fn run(&self) -> Result<()> {
		tracing::info!(?self, "Running CLI command");

		Ok(())
	}
}

fn styles() -> Styles {
	Styles::styled()
		.header(AnsiColor::Red.on_default() | Effects::BOLD)
		.usage(AnsiColor::Red.on_default() | Effects::BOLD)
		.literal(AnsiColor::Blue.on_default() | Effects::BOLD)
		.placeholder(AnsiColor::Green.on_default())
}

#[cfg(test)]
mod tests {
	use clap::Parser;

	use crate::cli::Cli;

	#[test]
	fn default_placeholder_mentions_name_placeholder() {
		assert_eq!(Cli::parse_from(["app"]).placeholder, "Welcome to use name_placeholder!");
	}
}
