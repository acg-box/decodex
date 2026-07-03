#[cfg(unix)] use std::os::unix::fs::PermissionsExt;
use std::{env, fs, path::PathBuf};

use tempfile::TempDir;

#[rustfmt::skip]
use crate::test_support::TestEnvVarGuard;
#[rustfmt::skip]
use crate::worktree::WorktreeSpec;
pub(in crate::orchestrator::tests) fn install_fake_open_pr_gh_response(
	temp_dir: &TempDir,
	worktree: &WorktreeSpec,
	pr_url: &str,
	head_oid: &str,
) -> TestEnvVarGuard {
	let fake_gh_dir = temp_dir.path().join("fake-bin");
	let fake_gh_path = fake_gh_dir.join("gh");
	let fake_gh_response = serde_json::json!({
		"data": {
			"repository": {
				"mergeCommitAllowed": true,
				"pullRequest": {
					"url": pr_url,
					"state": "OPEN",
					"isDraft": false,
					"reviewDecision": "APPROVED",
					"baseRefName": "main",
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

pub(in crate::orchestrator::tests) fn install_fake_closeout_gh_responses(
	temp_dir: &TempDir,
	worktree: &WorktreeSpec,
	pr_url: &str,
	head_oid: &str,
) -> TestEnvVarGuard {
	install_fake_closeout_gh_responses_with_state(temp_dir, worktree, pr_url, head_oid, "MERGED")
}

pub(in crate::orchestrator::tests) fn install_fake_closeout_gh_responses_with_state(
	temp_dir: &TempDir,
	worktree: &WorktreeSpec,
	pr_url: &str,
	head_oid: &str,
	pr_state: &str,
) -> TestEnvVarGuard {
	install_fake_closeout_gh_responses_with_states(
		temp_dir, worktree, pr_url, head_oid, pr_state, pr_state,
	)
}

pub(in crate::orchestrator::tests) fn install_fake_closeout_gh_responses_with_states(
	temp_dir: &TempDir,
	worktree: &WorktreeSpec,
	pr_url: &str,
	head_oid: &str,
	pr_view_state: &str,
	graphql_state: &str,
) -> TestEnvVarGuard {
	let fake_gh_dir = temp_dir.path().join("fake-closeout-bin");
	let fake_gh_path = fake_gh_dir.join("gh");
	let fake_pr_view_response = serde_json::json!({
		"url": pr_url,
		"state": pr_view_state,
		"isDraft": false,
		"baseRefName": "main",
		"headRefName": worktree.branch_name.clone(),
		"headRefOid": head_oid,
		"headRepository": { "name": "decodex" },
		"headRepositoryOwner": { "login": "hack-ink" }
	})
	.to_string();
	let fake_graphql_response = serde_json::json!({
		"data": {
			"repository": {
				"mergeCommitAllowed": true,
				"pullRequest": {
					"url": pr_url,
					"state": graphql_state,
					"isDraft": false,
					"reviewDecision": "APPROVED",
					"baseRefName": "main",
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
if [ \"$1\" = \"--version\" ]; then\n\
  printf '%s\\n' 'gh version 2.0.0'\n\
  exit 0\n\
fi\n\
if [ \"$1\" = \"pr\" ] && [ \"$2\" = \"view\" ]; then\n\
  printf '%s' '{}'\n\
  exit 0\n\
fi\n\
if [ \"$1\" = \"api\" ] && [ \"$2\" = \"graphql\" ]; then\n\
  printf '%s' '{}'\n\
  exit 0\n\
fi\n\
if [ \"$1\" = \"api\" ] && [ \"$2\" = \"--method\" ] && [ \"$3\" = \"DELETE\" ]; then\n\
  exit 0\n\
fi\n\
echo \"unexpected gh invocation: $*\" >&2\n\
exit 1\n",
			fake_pr_view_response, fake_graphql_response
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
