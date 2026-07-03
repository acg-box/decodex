use crate::tracker::public_text;

#[test]
fn rejects_legacy_public_comments_with_sensitive_or_unknown_paths() {
	for (body, expected_error) in [
		(
			"decodex run failed and will retry\n\n- worktree_path: `/absolute/path/to/repo/.worktrees/DEC-1`",
			"`worktree_path` must be repository-relative, not `/absolute/path/to/repo/.worktrees/DEC-1`.",
		),
		(
			"decodex run failed and will retry\n\n- unexpected_path: `/absolute/path/to/repo/.worktrees/DEC-1`",
			"Unsupported structured field `unexpected_path` in public issue comments.",
		),
		(
			"decodex run failed and will retry\n\n- worktree_path: `C:/absolute/path/to/repo/.worktrees/DEC-1`",
			"`body` must be public/team-visible text; host-local paths are not allowed.",
		),
	] {
		let error = public_text::validate_public_comment_body(body)
			.expect_err("legacy free-form body should still fail public text validation");

		assert_eq!(error, expected_error);
	}
}
