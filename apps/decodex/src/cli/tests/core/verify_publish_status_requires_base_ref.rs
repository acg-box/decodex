use clap::Parser as _;

use crate::cli::Cli;

#[test]
fn verify_publish_status_success_requires_expected_base_ref() {
	let error = Cli::try_parse_from([
		"decodex",
		"verify",
		"publish-status",
		"--config",
		"./project.toml",
		"--pr",
		"https://github.com/hack-ink/decodex/pull/1",
		"--state",
		"success",
		"--expected-head",
		"abc123",
	])
	.expect_err("success status publishing should require a base binding");

	assert!(error.to_string().contains("--expected-base-ref"));
}

#[test]
fn verify_publish_status_success_requires_expected_base_oid() {
	let error = Cli::try_parse_from([
		"decodex",
		"verify",
		"publish-status",
		"--config",
		"./project.toml",
		"--pr",
		"https://github.com/hack-ink/decodex/pull/1",
		"--state",
		"success",
		"--expected-head",
		"abc123",
		"--expected-base-ref",
		"main",
	])
	.expect_err("success status publishing should require a base tip binding");

	assert!(error.to_string().contains("--expected-base-oid"));
}
