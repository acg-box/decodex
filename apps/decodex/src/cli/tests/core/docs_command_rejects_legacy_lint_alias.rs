use clap::Parser;

use crate::cli::Cli;

#[test]
fn docs_command_rejects_legacy_lint_alias() {
	let error = Cli::try_parse_from(["decodex", "docs", "lint"])
		.expect_err("command aliases are not supported");

	assert!(error.to_string().contains("unrecognized subcommand 'lint'"));
}
