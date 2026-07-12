use clap::Parser;

use crate::cli::Cli;

#[test]
fn rejects_land_related_before_execution() {
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
