use std::{fs, os::unix::fs::PermissionsExt};

use tempfile::TempDir;

use crate::github::{self, RemoteRefDeleteReadback};

#[test]
fn exact_remote_ref_stops_before_unsupported_unconditional_delete() {
	let fixture = GhFixture::present("expected-oid");
	let result = reconcile(&fixture, "expected-oid").expect("readback");
	assert!(matches!(result, RemoteRefDeleteReadback::ConditionalMutationUnsupported { .. }));
	let args = fs::read_to_string(&fixture.log).expect("log");
	assert!(args.contains("api --method GET repos/helixbox/pubfi-mono/git/ref/heads/x/pubfi"));
	assert!(!args.contains("DELETE"));
}

#[test]
fn missing_remote_ref_returns_receipt_and_changed_oid_is_drift() {
	let missing = GhFixture::missing();
	let result = reconcile(&missing, "expected-oid").expect("missing readback");
	let RemoteRefDeleteReadback::AlreadyAbsent(receipt) = result else {
		panic!("expected absence receipt");
	};
	assert_eq!(receipt.request_digest(), "request");

	let changed = GhFixture::present("different-oid");
	assert!(matches!(
		reconcile(&changed, "expected-oid").expect("drift readback"),
		RemoteRefDeleteReadback::PrerequisiteDrift { .. }
	));
}

fn reconcile(
	fixture: &GhFixture,
	expected_oid: &str,
) -> color_eyre::Result<RemoteRefDeleteReadback> {
	github::reconcile_remote_ref_delete(
		fixture.temp.path(),
		"helixbox",
		"pubfi-mono",
		"x/pubfi",
		expected_oid,
		"request",
		"2026-07-12T00:00:00Z",
		1,
		"token",
		Some(&fixture.command),
	)
}

struct GhFixture {
	temp: TempDir,
	command: std::path::PathBuf,
	log: std::path::PathBuf,
}
impl GhFixture {
	fn present(oid: &str) -> Self {
		Self::new(&format!(
			"printf '%s' '{{\"ref\":\"refs/heads/x/pubfi\",\"object\":{{\"sha\":\"{oid}\",\"type\":\"commit\"}}}}'"
		))
	}

	fn missing() -> Self {
		Self::new("printf '%s' 'HTTP 422: Reference does not exist' >&2; exit 1")
	}

	fn new(response: &str) -> Self {
		let temp = TempDir::new().expect("tempdir");
		let command = temp.path().join("gh");
		let log = temp.path().join("args.log");
		fs::write(
			&command,
			format!("#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\n{response}\n", log.display()),
		)
		.expect("fake gh");
		fs::set_permissions(&command, fs::Permissions::from_mode(0o755)).expect("mode");
		Self { temp, command, log }
	}
}
