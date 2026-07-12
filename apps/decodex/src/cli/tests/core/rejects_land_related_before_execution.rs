use clap::Parser;

use crate::cli::Cli;

#[test]
fn lane_authority_v2_c6_adj_02() {
	let error = Cli::try_parse_from([
		"decodex",
		"land",
		"land summary",
		"--authority",
		"XY-1251",
		"--related",
		"XY-1249",
	])
	.expect_err("land must reject unsupported related issues at parse time");

	assert!(error.to_string().contains("unexpected argument '--related'"));
}
