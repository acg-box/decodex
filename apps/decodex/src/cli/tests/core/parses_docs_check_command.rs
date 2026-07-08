use clap::Parser;

use crate::cli::{Cli, Command, docs_commands::DocsCommand};

#[test]
fn parses_docs_check_command() {
	let cli = Cli::parse_from(["decodex", "docs", "check"]);

	assert!(matches!(cli.command, Command::Docs(DocsCommand { .. })));
}
