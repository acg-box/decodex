use crate::orchestrator::tests::{
	Path, PathBuf, PermissionsExt, RUN_ACTIVITY_MARKER_FILE, TempDir, TestEnvVarGuard, env, fs,
	serde_json,
};

pub(super) fn rewrite_run_activity_marker_host_boot_id(worktree_path: &Path, host_boot_id: &str) {
	let marker_path = worktree_path.join(RUN_ACTIVITY_MARKER_FILE);
	let marker_body = fs::read_to_string(&marker_path).expect("marker body should load");
	let mut host_boot_id_written = false;
	let mut rewritten = marker_body
		.lines()
		.map(|line| {
			if line.starts_with("host_boot_id=") {
				host_boot_id_written = true;

				format!("host_boot_id={host_boot_id}")
			} else {
				line.to_owned()
			}
		})
		.collect::<Vec<_>>();

	if !host_boot_id_written {
		rewritten.push(format!("host_boot_id={host_boot_id}"));
	}

	fs::write(&marker_path, rewritten.join("\n") + "\n").expect("marker body should rewrite");
}

pub(super) fn rewrite_run_activity_marker_process_start_identity(
	worktree_path: &Path,
	process_start_identity: &str,
) {
	let marker_path = worktree_path.join(RUN_ACTIVITY_MARKER_FILE);
	let marker_body = fs::read_to_string(&marker_path).expect("marker body should load");
	let mut process_start_identity_written = false;
	let mut rewritten = marker_body
		.lines()
		.map(|line| {
			if line.starts_with("process_start_identity=") {
				process_start_identity_written = true;

				format!("process_start_identity={process_start_identity}")
			} else {
				line.to_owned()
			}
		})
		.collect::<Vec<_>>();

	if !process_start_identity_written {
		rewritten.push(format!("process_start_identity={process_start_identity}"));
	}

	fs::write(&marker_path, rewritten.join("\n") + "\n").expect("marker body should rewrite");
}

pub(super) fn install_fake_post_issue_comment_gh_response(
	temp_dir: &TempDir,
	comment_id: i64,
	created_at: &str,
) -> TestEnvVarGuard {
	let fake_gh_dir = temp_dir.path().join("fake-bin");
	let fake_gh_path = fake_gh_dir.join("gh");
	let fake_gh_response = serde_json::json!({
		"id": comment_id,
		"created_at": created_at,
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

pub(super) fn install_fake_admin_merge_gh_response(temp_dir: &TempDir) -> (PathBuf, PathBuf) {
	install_fake_admin_merge_gh_response_with_merge_exit_code(temp_dir, "deadbeef", 0)
}

pub(super) fn install_fake_admin_merge_gh_response_with_merge_exit_code(
	temp_dir: &TempDir,
	pr_head_oid: &str,
	merge_exit_code: i32,
) -> (PathBuf, PathBuf) {
	let fake_gh_dir = temp_dir.path().join("fake-bin");
	let fake_gh_path = fake_gh_dir.join("gh");
	let invocation_log_path = temp_dir.path().join("gh-invocation.log");

	fs::create_dir_all(&fake_gh_dir).expect("fake gh directory should exist");
	fs::write(
		&fake_gh_path,
		format!(
			"#!/bin/sh\n\
printf '%s\\n' \"$@\" >> '{}'\n\
if [ \"$1\" = \"pr\" ] && [ \"$2\" = \"merge\" ]; then\n\
  exit {}\n\
fi\n\
if [ \"$1\" = \"pr\" ] && [ \"$2\" = \"view\" ]; then\n\
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
				"mergeCommit": { "oid": "cafebabe" },
			}),
		),
	)
	.expect("fake gh script should write");

	let mut permissions =
		fs::metadata(&fake_gh_path).expect("fake gh metadata should read").permissions();

	#[cfg(unix)]
	PermissionsExt::set_mode(&mut permissions, 0o755);
	fs::set_permissions(&fake_gh_path, permissions)
		.expect("fake gh script should become executable");

	(fake_gh_path, invocation_log_path)
}
