#[cfg(unix)] use std::os::unix::fs::PermissionsExt;
use std::{env, fs};

use tempfile::TempDir;

use crate::{test_support::TestEnvVarGuard, worktree::WorktreeSpec};

pub(in crate::orchestrator::tests) fn install_fake_conflicting_pr_gh_response(
	temp_dir: &TempDir,
	worktree: &WorktreeSpec,
	pr_url: &str,
	head_oid: &str,
) -> TestEnvVarGuard {
	let fake_gh_dir = temp_dir.path().join("fake-conflict-bin");
	let fake_gh_path = fake_gh_dir.join("gh");
	let fake_gh_response = serde_json::json!({
		"data": {
			"repository": {
				"mergeCommitAllowed": true,
				"pullRequest": {
					"url": pr_url,
					"state": "OPEN",
					"isDraft": false,
					"reviewDecision": "REVIEW_REQUIRED",
					"baseRefName": "main",
					"mergeable": "CONFLICTING",
					"mergeStateStatus": "DIRTY",
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
	fs::write(&fake_gh_path, format!("#!/bin/sh\nprintf '%s' '{fake_gh_response}'\n"))
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
