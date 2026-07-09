use clap::{Parser, error::ErrorKind};

use crate::cli::{
	Cli, Command,
	openwiki_commands::{OpenWikiCommand, OpenWikiSubcommand},
};

#[test]
fn parses_openwiki_check_command() {
	let cli = Cli::parse_from(["decodex", "openwiki", "check"]);

	assert!(matches!(
		cli.command,
		Command::OpenWiki(OpenWikiCommand { command: OpenWikiSubcommand::Check(_) })
	));
}

#[test]
fn openwiki_help_is_available() {
	for args in
		[&["decodex", "openwiki", "--help"][..], &["decodex", "openwiki", "check", "--help"][..]]
	{
		let error = Cli::try_parse_from(args.iter().copied()).expect_err("help should render");

		assert_eq!(error.kind(), ErrorKind::DisplayHelp);
	}
}
