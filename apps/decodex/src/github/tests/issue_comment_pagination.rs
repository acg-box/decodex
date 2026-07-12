use std::{fs, os::unix::fs::PermissionsExt, process::Command};

use crate::github;
use tempfile::TempDir;

#[test]
fn issue_comment_marker_scan_uses_all_pages_and_finds_page_two() {
	let mut command = Command::new("gh");
	github::comments::configure_issue_comments_list_command(
		&mut command,
		"repos/hack-ink/decodex/issues/1073/comments?per_page=100",
	);
	let args = command.get_args().map(|arg| arg.to_string_lossy()).collect::<Vec<_>>();
	assert_eq!(
		args,
		vec![
			"api",
			"--paginate",
			"--slurp",
			"repos/hack-ink/decodex/issues/1073/comments?per_page=100",
		],
	);

	let payload = br#"[
		[{"id": 1, "body": "unrelated", "created_at": "2026-07-12T00:00:00Z"}],
		[{"id": 2, "body": "<!-- decodex:operation-1 -->", "created_at": "2026-07-12T00:01:00Z"}]
	]"#;
	let matched = github::comments::find_issue_comment_marker_in_slurped_pages(
		payload,
		"<!-- decodex:operation-1 -->",
		"https://github.com/hack-ink/decodex/pull/1073",
	)
	.expect("all-page marker scan");
	assert_eq!(matched, Some((2, 1_783_814_460)));

	let temp_dir = TempDir::new().expect("tempdir");
	let fake_gh = temp_dir.path().join("gh");
	fs::write(&fake_gh, format!("#!/bin/sh\nprintf '%s' '{}'\n", String::from_utf8_lossy(payload)))
		.expect("fake gh");
	fs::set_permissions(&fake_gh, fs::Permissions::from_mode(0o755)).expect("fake gh mode");
	let ensured = github::comments::ensure_pull_request_issue_comment(
		temp_dir.path(),
		"https://github.com/hack-ink/decodex/pull/1073",
		"<!-- decodex:operation-1 -->",
		"closeout\n<!-- decodex:operation-1 -->",
		"token",
		Some(&fake_gh),
	)
	.expect("existing page-two marker");
	assert_eq!(ensured, (2, 1_783_814_460, false));
}
