use clap::Parser;

use crate::cli::Cli;

#[test]
fn rejects_commit_related_before_execution() {
	let error = Cli::try_parse_from([
		"decodex",
		"commit",
		"redesign decodex cli",
		"--authority",
		"XY-225",
		"--related",
		"XY-201",
		"--breaking",
	])
	.expect_err("commit-local authority must reject related issues");

	assert!(error.to_string().contains("unexpected argument '--related'"));
}
