use clap::Parser;

use crate::cli::Cli;

#[test]
fn land_manual_authority_requires_pr() {
	let error = Cli::try_parse_from(["decodex", "land", "ship hotfix", "--manual-authority"])
		.expect_err("manual authority land should require an explicit PR");

	assert!(error.to_string().contains("--manual-authority"));
	assert!(error.to_string().contains("--pr"));
}
