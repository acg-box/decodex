use std::path::Path;

#[rustfmt::skip]
use crate::orchestrator::tests::{self};
pub(in crate::orchestrator::tests) fn initialize_closeout_cleanup_origin(
	repo_root: &Path,
	remote_root: &Path,
) {
	tests::git_status_success(
		remote_root.parent().expect("remote root should have parent"),
		&[
			"init",
			"--bare",
			"--initial-branch",
			"main",
			remote_root.to_str().expect("remote path should be utf-8"),
		],
	);
	tests::git_status_success(
		repo_root,
		&["remote", "add", "origin", remote_root.to_string_lossy().as_ref()],
	);
	tests::git_status_success(repo_root, &["push", "-u", "origin", "main"]);
}

pub(in crate::orchestrator::tests) fn route_origin_github_url_to_local_bare_repo(
	repo_root: &Path,
	remote_root: &Path,
) {
	let github_remote = "https://github.com/hack-ink/decodex.git";
	let local_remote = format!("file://{}", remote_root.display());

	tests::git_status_success(
		repo_root,
		&["config", &format!("url.{local_remote}.insteadOf"), github_remote],
	);
	tests::git_status_success(repo_root, &["remote", "set-url", "origin", github_remote]);
}
