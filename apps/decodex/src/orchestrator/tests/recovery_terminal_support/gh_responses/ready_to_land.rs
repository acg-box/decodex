#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{env, fs, path::PathBuf};

use tempfile::TempDir;

use crate::{test_support::TestEnvVarGuard, worktree::WorktreeSpec};

pub(in crate::orchestrator::tests) fn install_fake_ready_to_land_admin_merge_gh_response(
	temp_dir: &TempDir,
	worktree: &WorktreeSpec,
	pr_url: &str,
	head_oid: &str,
) -> (TestEnvVarGuard, PathBuf) {
	let fake_gh_dir = temp_dir.path().join("fake-ready-to-land-bin");
	let fake_gh_path = fake_gh_dir.join("gh");
	let invocation_log_path = temp_dir.path().join("ready-to-land-gh-invocation.log");
	let fake_graphql_response = serde_json::json!({
		"data": {
			"repository": {
				"mergeCommitAllowed": true,
				"pullRequest": {
					"url": pr_url,
					"state": "OPEN",
					"isDraft": false,
					"reviewDecision": "APPROVED",
					"mergeable": "MERGEABLE",
					"mergeStateStatus": "CLEAN",
					"headRefName": worktree.branch_name.clone(),
					"headRefOid": head_oid,
					"headRepository": { "name": "decodex" },
					"headRepositoryOwner": { "login": "hack-ink" },
					"reactionGroups": [],
					"comments": {
						"nodes": [],
						"pageInfo": { "hasNextPage": false, "endCursor": null }
					},
					"reviews": { "nodes": [] },
					"reviewRequests": { "totalCount": 0 },
					"reviewThreads": {
						"nodes": [],
						"pageInfo": { "hasNextPage": false, "endCursor": null }
					},
					"commits": {
						"nodes": [
							{ "commit": { "statusCheckRollup": { "state": "SUCCESS" } } }
						]
					}
				}
			}
		}
	})
	.to_string();
	let fake_pr_view_response = serde_json::json!({
		"state": "MERGED",
		"headRefOid": head_oid,
		"mergeCommit": { "oid": "cafebabe" },
	})
	.to_string();

	fs::create_dir_all(&fake_gh_dir).expect("fake gh directory should exist");
	fs::write(
		&fake_gh_path,
		format!(
			"#!/bin/sh\n\
if [ \"$1\" = \"api\" ] && [ \"$2\" = \"graphql\" ]; then\n\
  printf '%s' '{}'\n\
  exit 0\n\
fi\n\
if [ \"$1\" = \"pr\" ] && [ \"$2\" = \"merge\" ]; then\n\
  printf '%s\\n' \"$@\" >> '{}'\n\
  exit 0\n\
fi\n\
if [ \"$1\" = \"pr\" ] && [ \"$2\" = \"view\" ]; then\n\
  printf '%s' '{}'\n\
  exit 0\n\
fi\n\
echo \"unexpected gh invocation: $*\" >&2\n\
exit 1\n",
			fake_graphql_response,
			invocation_log_path.display(),
			fake_pr_view_response
		),
	)
	.expect("fake gh script should write");

	let mut permissions =
		fs::metadata(&fake_gh_path).expect("fake gh metadata should read").permissions();

	#[cfg(unix)]
	PermissionsExt::set_mode(&mut permissions, 0o755);
	fs::set_permissions(&fake_gh_path, permissions)
		.expect("fake gh script should become executable");

	let path_env = env::var("PATH").unwrap_or_default();

	(
		TestEnvVarGuard::set("PATH", &format!("{}:{path_env}", fake_gh_dir.display())),
		invocation_log_path,
	)
}
