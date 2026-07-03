#[cfg(unix)] use std::os::unix::fs::PermissionsExt;
use std::{
	env, fs,
	path::{Path, PathBuf},
};

use tempfile::TempDir;

use crate::test_support::TestEnvVarGuard;

pub(in crate::manual::tests) fn install_fake_landing_state_gh(
	temp_dir: &TempDir,
	state: &str,
	branch_name: &str,
	head_oid: &str,
	merge_commit: &str,
) -> TestEnvVarGuard {
	let fake_gh_dir = temp_dir.path().join("fake-recovery-bin");
	let fake_gh_path = fake_gh_dir.join("gh");

	fs::create_dir_all(&fake_gh_dir).expect("fake gh directory should exist");
	fs::write(
		&fake_gh_path,
		format!(
			"#!/bin/sh\n\
if [ \"$1\" = \"api\" ] && [ \"$2\" = \"graphql\" ]; then\n\
  printf '%s' '{}'\n\
  exit 0\n\
fi\n\
if [ \"$1\" = \"pr\" ] && [ \"$2\" = \"view\" ]; then\n\
  printf '%s' '{}'\n\
  exit 0\n\
fi\n\
echo \"unexpected gh invocation: $*\" >&2\n\
exit 1\n",
			serde_json::json!({
				"data": {
					"repository": {
						"pullRequest": {
							"url": "https://github.com/hack-ink/decodex/pull/64",
							"state": state,
							"isDraft": false,
							"reviewDecision": "APPROVED",
							"baseRefName": "main",
							"mergeable": "MERGEABLE",
							"mergeStateStatus": "CLEAN",
							"headRefName": branch_name,
							"headRefOid": head_oid,
							"reviewRequests": { "totalCount": 0 },
							"reviewThreads": {
								"nodes": [],
								"pageInfo": { "hasNextPage": false, "endCursor": null },
							},
							"commits": {
								"nodes": [
									{
										"commit": {
											"statusCheckRollup": { "state": "SUCCESS" },
										},
									},
								],
							},
						},
					},
				},
			}),
			serde_json::json!({
				"state": state,
				"headRefOid": head_oid,
				"mergeCommit": { "oid": merge_commit },
			}),
		),
	)
	.expect("fake gh script should write");

	make_executable(&fake_gh_path);

	let path_env = env::var("PATH").unwrap_or_default();

	TestEnvVarGuard::set("PATH", &format!("{}:{path_env}", fake_gh_dir.display()))
}

pub(in crate::manual::tests) fn install_fake_repo_view_gh(temp_dir: &TempDir) -> PathBuf {
	let fake_gh_dir = temp_dir.path().join("fake-repo-view-bin");
	let fake_gh_path = fake_gh_dir.join("gh");

	fs::create_dir_all(&fake_gh_dir).expect("fake gh directory should exist");
	fs::write(
		&fake_gh_path,
		format!(
			"#!/bin/sh\n\
if [ \"$1\" = \"repo\" ] && [ \"$2\" = \"view\" ]; then\n\
  printf '%s' '{}'\n\
  exit 0\n\
fi\n\
if [ \"$1\" = \"auth\" ] && [ \"$2\" = \"token\" ]; then\n\
  printf '%s\\n' 'ghp_fake_auth_token'\n\
  exit 0\n\
fi\n\
echo \"unexpected gh invocation: $*\" >&2\n\
exit 1\n",
			serde_json::json!({
				"name": "decodex",
				"owner": { "login": "hack-ink" },
				"defaultBranchRef": { "name": "main" },
				"mergeCommitAllowed": true,
			}),
		),
	)
	.expect("fake gh script should write");

	make_executable(&fake_gh_path);

	fake_gh_dir
}

pub(in crate::manual::tests) fn install_fake_admin_merge_gh(
	temp_dir: &TempDir,
	merged_head_oid: &str,
) -> (TestEnvVarGuard, PathBuf) {
	install_fake_admin_merge_gh_with_merge_exit_code(
		temp_dir,
		merged_head_oid,
		"deadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
		r#"{"schema":"decodex/commit/1","summary":"ship hotfix","authority":"manual"}"#,
		0,
	)
}

pub(in crate::manual::tests) fn install_fake_admin_merge_gh_with_merge_exit_code(
	temp_dir: &TempDir,
	merged_head_oid: &str,
	pr_head_oid: &str,
	merge_subject: &str,
	merge_exit_code: i32,
) -> (TestEnvVarGuard, PathBuf) {
	let fake_gh_dir = temp_dir.path().join("fake-bin");
	let fake_gh_path = fake_gh_dir.join("gh");
	let invocation_log_path = temp_dir.path().join("gh-invocation.log");

	fs::create_dir_all(&fake_gh_dir).expect("fake gh directory should exist");
	fs::write(
		&fake_gh_path,
		format!(
			"#!/bin/sh\n\
printf '%s\\n' \"$*\" >> '{}'\n\
if [ \"$1\" = \"pr\" ] && [ \"$2\" = \"merge\" ]; then\n\
  exit {}\n\
fi\n\
if [ \"$1\" = \"pr\" ] && [ \"$2\" = \"view\" ]; then\n\
  printf '%s' '{}'\n\
  exit 0\n\
fi\n\
if [ \"$1\" = \"api\" ]; then\n\
  printf '%s' '{}'\n\
  exit 0\n\
fi\n\
echo \"unexpected gh invocation: $*\" >&2\n\
exit 1\n",
			invocation_log_path.display(),
			merge_exit_code,
			serde_json::json!({
				"state": "MERGED",
				"headRefOid": pr_head_oid,
				"mergeCommit": { "oid": merged_head_oid },
			}),
			serde_json::json!({
				"commit": { "message": format!("{merge_subject}\n\n") },
			}),
		),
	)
	.expect("fake gh script should write");

	make_executable(&fake_gh_path);

	let path_env = env::var("PATH").unwrap_or_default();

	(
		TestEnvVarGuard::set("PATH", &format!("{}:{path_env}", fake_gh_dir.display())),
		invocation_log_path,
	)
}

fn make_executable(path: &Path) {
	let mut permissions = fs::metadata(path).expect("fake gh metadata should read").permissions();

	#[cfg(unix)]
	{
		PermissionsExt::set_mode(&mut permissions, 0o755);
	}

	fs::set_permissions(path, permissions).expect("fake gh script should become executable");
}
