use clap::{Parser, error::ErrorKind};

use crate::cli::Cli;

#[test]
fn rejects_radar_as_runtime_subcommand() {
	let error = Cli::try_parse_from(["decodex", "radar"]).expect_err("radar is a standalone tool");

	assert_eq!(error.kind(), ErrorKind::InvalidSubcommand);
}
