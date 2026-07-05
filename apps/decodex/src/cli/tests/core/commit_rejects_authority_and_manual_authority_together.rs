use clap::Parser;

use crate::cli::Cli;

#[test]
fn commit_rejects_authority_and_manual_authority_together() {
	let error = Cli::try_parse_from([
		"decodex",
		"commit",
		"ship hotfix",
		"--authority",
		"XY-225",
		"--manual-authority",
	])
	.expect_err("authority and manual-authority should conflict");

	assert!(error.to_string().contains("--authority"));
	assert!(error.to_string().contains("--manual-authority"));
}
