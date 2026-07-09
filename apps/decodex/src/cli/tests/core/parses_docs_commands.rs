use clap::{Parser, error::ErrorKind};

use crate::cli::{
	Cli, Command,
	docs_commands::{DocsCommand, DocsSubcommand},
};

#[test]
fn parses_docs_check_command() {
	let cli = Cli::parse_from(["decodex", "docs", "check"]);

	assert!(matches!(
		cli.command,
		Command::Docs(DocsCommand { command: DocsSubcommand::Check(_) })
	));
}

#[test]
fn docs_help_is_available() {
	for args in [&["decodex", "docs", "--help"][..], &["decodex", "docs", "check", "--help"][..]] {
		let error = Cli::try_parse_from(args.iter().copied()).expect_err("help should render");

		assert_eq!(error.kind(), ErrorKind::DisplayHelp);
	}
}
