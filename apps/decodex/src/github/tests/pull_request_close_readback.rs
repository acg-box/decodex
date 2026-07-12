use std::{fs, os::unix::fs::PermissionsExt};

use tempfile::TempDir;

use crate::github::{self, PullRequestCloseReadback};

#[test]
fn open_pull_request_stops_before_unsupported_unconditional_close() {
	let fixture = GhFixture::new("open", "head-1", "main");
	let result = github::reconcile_pull_request_close(
		fixture.temp.path(),
		"https://github.com/helixbox/pubfi-mono/pull/826",
		"head-1",
		"main",
		"request",
		"token",
		Some(&fixture.command),
	)
	.expect("readback");
	assert!(matches!(result, PullRequestCloseReadback::ConditionalMutationUnsupported { .. }));
	let args = fs::read_to_string(&fixture.log).expect("command log");
	assert!(args.contains("api --method GET repos/helixbox/pubfi-mono/pulls/826"));
	assert!(!args.contains("PATCH"));
}

#[test]
fn already_closed_exact_head_returns_receipt_and_drift_does_not() {
	let fixture = GhFixture::new("closed", "head-1", "main");
	let result = github::reconcile_pull_request_close(
		fixture.temp.path(),
		"https://github.com/helixbox/pubfi-mono/pull/826",
		"head-1",
		"main",
		"request",
		"token",
		Some(&fixture.command),
	)
	.expect("readback");
	let PullRequestCloseReadback::AlreadyClosed(receipt) = result else {
		panic!("expected receipt");
	};
	assert_eq!(receipt.request_digest(), "request");

	let drift = github::reconcile_pull_request_close(
		fixture.temp.path(),
		"https://github.com/helixbox/pubfi-mono/pull/826",
		"different-head",
		"main",
		"request",
		"token",
		Some(&fixture.command),
	)
	.expect("drift readback");
	assert!(matches!(drift, PullRequestCloseReadback::PrerequisiteDrift { .. }));
}

struct GhFixture {
	temp: TempDir,
	command: std::path::PathBuf,
	log: std::path::PathBuf,
}
impl GhFixture {
	fn new(state: &str, head: &str, base: &str) -> Self {
		let temp = TempDir::new().expect("tempdir");
		let command = temp.path().join("gh");
		let log = temp.path().join("args.log");
		let payload = serde_json::json!({
			"id": 826,
			"number": 826,
			"state": state,
			"updated_at": "2026-07-12T00:00:00Z",
			"head": {"sha": head},
			"base": {"ref": base},
		});
		fs::write(
			&command,
			format!(
				"#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nprintf '%s' '{}'\n",
				log.display(),
				payload
			),
		)
		.expect("fake gh");
		fs::set_permissions(&command, fs::Permissions::from_mode(0o755)).expect("mode");
		Self { temp, command, log }
	}
}
