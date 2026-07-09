use std::{
	env,
	ffi::OsString,
	fs,
	os::unix::fs::PermissionsExt as _,
	path::{Path, PathBuf},
};

use clap::Parser as _;
use tempfile::TempDir;

use crate::{cli::Cli, github, pull_request};

struct EnvGuard {
	key: String,
	previous: Option<OsString>,
}
impl EnvGuard {
	fn set(key: &str, value: &str) -> Self {
		let previous = env::var_os(key);

		unsafe { env::set_var(key, value) };

		Self { key: key.to_owned(), previous }
	}
}
impl Drop for EnvGuard {
	fn drop(&mut self) {
		match self.previous.take() {
			Some(previous) => unsafe { env::set_var(&self.key, previous) },
			None => unsafe { env::remove_var(&self.key) },
		}
	}
}

#[test]
fn verify_publish_status_e2e_publishes_and_reads_landing_gate() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let gh_log_path = temp_dir.path().join("gh.log");
	let gh_path = write_fake_gh(temp_dir.path(), &gh_log_path);
	let config_path = write_project_config(temp_dir.path(), &gh_path);
	let _token_guard = EnvGuard::set("DECODX_VERIFY_STATUS_E2E_TOKEN", "ghp_test");

	Cli::parse_from([
		"decodex",
		"verify",
		"publish-status",
		"--config",
		config_path.to_str().expect("config path should be utf8"),
		"--pr",
		"https://github.com/hack-ink/decodex/pull/42",
		"--context",
		"decodex/local-full-check",
		"--state",
		"success",
		"--expected-head",
		"head-sha",
		"--expected-base-ref",
		"main",
		"--expected-base-oid",
		"base-sha",
		"--description",
		"cargo make check passed",
	])
	.run()
	.expect("publish-status should publish the exact-head local validation status");

	let gh_log = fs::read_to_string(&gh_log_path).expect("fake gh should record calls");

	assert!(gh_log.contains("api graphql"));
	assert!(gh_log.contains("api --method POST repos/hack-ink/decodex/statuses/head-sha"));
	assert!(gh_log.contains("-f state=success"));
	assert!(gh_log.contains("-f context=decodex/local-full-check"));
	assert!(gh_log.contains("-f description=cargo make check passed; base_ref_oid=base-sha"));

	let landing_state = github::inspect_pull_request_landing_state(
		temp_dir.path(),
		"https://github.com/hack-ink/decodex/pull/42",
		"ghp_test",
		Some(&gh_path),
		&[String::from("decodex/local-full-check")],
		&[String::from("decodex-bot")],
	)
	.expect("landing readback should trust the matching local validation status");

	assert_eq!(landing_state.head_ref_oid, "head-sha");
	assert_eq!(landing_state.base_ref_oid.as_deref(), Some("base-sha"));
	assert!(pull_request::manual_landing_gates_satisfied(landing_state.gate_view()));
}

fn write_project_config(temp_dir: &Path, gh_path: &Path) -> PathBuf {
	let config_path = temp_dir.join("project.toml");
	let config = format!(
		r#"
service_id = "pubfi"

[tracker]
api_key_env_var = "HOME"

[github]
token_env_var = "DECODX_VERIFY_STATUS_E2E_TOKEN"
command_path = "{}"
landing_mode = "fast"
landing_actors = ["decodex-bot"]

[paths]
repo_root = "."
"#,
		gh_path.display()
	);

	fs::write(&config_path, config).expect("project config should write");

	config_path
}

fn write_fake_gh(temp_dir: &Path, gh_log_path: &Path) -> PathBuf {
	let gh_path = temp_dir.join("gh");
	let script = format!(
		r#"#!/bin/sh
set -eu
printf '%s\n' "$*" >> '{log_path}'

case "$*" in
  *"api graphql"*)
    cat <<'JSON'
{{"data":{{"repository":{{"pullRequest":{{"url":"https://github.com/hack-ink/decodex/pull/42","state":"OPEN","isDraft":false,"reviewDecision":"APPROVED","baseRefName":"main","baseRefOid":"base-sha","mergeable":"MERGEABLE","mergeStateStatus":"CLEAN","headRefName":"xy/local-full-check-status","headRefOid":"head-sha","reviewRequests":{{"totalCount":0}},"reviewThreads":{{"nodes":[],"pageInfo":{{"hasNextPage":false,"endCursor":null}}}},"commits":{{"nodes":[{{"commit":{{"statusCheckRollup":{{"state":"PENDING"}}}}}}]}}}}}}}}}}
JSON
    ;;
  *"--method POST repos/hack-ink/decodex/statuses/head-sha"*)
    cat <<'JSON'
{{}}
JSON
    ;;
  *"repos/hack-ink/decodex/commits/head-sha/statuses"*)
    if ! grep -q 'base_ref_oid=base-sha' '{log_path}'; then
      echo 'expected publish call with base_ref_oid=base-sha before status read' >&2
      exit 1
    fi
    cat <<'JSON'
[{{"context":"decodex/local-full-check","state":"success","description":"cargo make check passed; base_ref_oid=base-sha","creator":{{"login":"decodex-bot"}}}}]
JSON
    ;;
  *)
    echo "unexpected gh invocation: $*" >&2
    exit 1
    ;;
esac
"#,
		log_path = gh_log_path.display()
	);

	fs::write(&gh_path, script).expect("fake gh should write");

	let mut permissions = fs::metadata(&gh_path).expect("fake gh metadata").permissions();

	permissions.set_mode(0o755);

	fs::set_permissions(&gh_path, permissions).expect("fake gh should be executable");

	gh_path
}
