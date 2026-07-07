#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{env, fs};

use tempfile::TempDir;

use crate::{test_support::TestEnvVarGuard, worktree::WorktreeSpec};

pub(in crate::orchestrator::tests) fn install_fake_merged_pr_gh_response(
	temp_dir: &TempDir,
	worktree: &WorktreeSpec,
	pr_url: &str,
	head_oid: &str,
) -> TestEnvVarGuard {
	install_fake_merged_pr_gh_response_with_base_ref_and_delete_exit_code(
		temp_dir, worktree, pr_url, head_oid, "main", 0,
	)
}

pub(in crate::orchestrator::tests) fn install_fake_merged_pr_gh_response_with_base_ref(
	temp_dir: &TempDir,
	worktree: &WorktreeSpec,
	pr_url: &str,
	head_oid: &str,
	base_ref_name: &str,
) -> TestEnvVarGuard {
	install_fake_merged_pr_gh_response_with_base_ref_and_delete_exit_code(
		temp_dir,
		worktree,
		pr_url,
		head_oid,
		base_ref_name,
		0,
	)
}

pub(in crate::orchestrator::tests) fn install_fake_merged_pr_gh_response_with_delete_exit_code(
	temp_dir: &TempDir,
	worktree: &WorktreeSpec,
	pr_url: &str,
	head_oid: &str,
	delete_exit_code: i32,
) -> TestEnvVarGuard {
	install_fake_merged_pr_gh_response_with_base_ref_and_delete_exit_code(
		temp_dir,
		worktree,
		pr_url,
		head_oid,
		"main",
		delete_exit_code,
	)
}

pub(in crate::orchestrator::tests) fn install_fake_merged_pr_gh_response_with_base_ref_and_delete_exit_code(
	temp_dir: &TempDir,
	worktree: &WorktreeSpec,
	pr_url: &str,
	head_oid: &str,
	base_ref_name: &str,
	delete_exit_code: i32,
) -> TestEnvVarGuard {
	let fake_gh_dir = temp_dir.path().join("fake-bin");
	let fake_gh_path = fake_gh_dir.join("gh");
	let fake_pr_view_response = serde_json::json!({
		"state": "MERGED",
		"headRefOid": head_oid,
		"mergeCommit": { "oid": "cafebabe" }
	})
	.to_string();
	let fake_gh_response = serde_json::json!({
		"data": {
			"repository": {
				"mergeCommitAllowed": true,
				"pullRequest": {
					"url": pr_url,
					"state": "MERGED",
					"isDraft": false,
					"reviewDecision": "APPROVED",
					"baseRefName": base_ref_name,
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
if [ \"$1\" = \"api\" ] && [ \"$2\" = \"--method\" ] && [ \"$3\" = \"DELETE\" ]; then\n\
  if [ {delete_exit_code} -eq 0 ]; then\n\
    exit 0\n\
  fi\n\
  echo 'delete denied by fake gh' >&2\n\
  exit {delete_exit_code}\n\
fi\n\
echo \"unexpected gh invocation: $*\" >&2\n\
exit 1\n",
			fake_gh_response, fake_pr_view_response
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

	TestEnvVarGuard::set("PATH", &format!("{}:{path_env}", fake_gh_dir.display()))
}
