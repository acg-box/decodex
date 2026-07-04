use crate::tracker::public_text::{self};

#[test]
fn accepts_public_collaboration_identifiers() {
	for value in [
		"PR https://github.com/hack-ink/decodex/pull/42 is ready.",
		"Branch y/decodex-xy-519 reached commit 0123456789abcdef0123456789abcdef01234567.",
		"Issue XY-519 updated docs/spec/runtime.md and .worktrees/XY-519.",
	] {
		public_text::validate_public_text_field("summary", value)
			.unwrap_or_else(|error| panic!("public value should validate: {error}"));
	}
}

#[test]
fn rejects_leakage_shaped_public_text() {
	for value in [
		"Local checkout was /Users/example/code/repo.",
		"Read ~/.codex/auth.json for the selected account.",
		"Windows path C:\\Users\\example\\repo was present.",
		"Missing GITHUB_PAT_Y blocked the push.",
		"Selected account was account=...e4919e.",
		"Missing API key for tracker writes.",
		"CODEX_HOME pointed at private configuration.",
		"codex.github-identity was routed to a private person.",
		"Selected account user@example.com was active.",
	] {
		let error = public_text::validate_public_text_field("evidence", value)
			.expect_err("leakage-shaped value should be rejected");

		assert!(error.contains("public/team-visible"));
	}
}

#[test]
fn validates_public_comment_structured_paths() {
	public_text::validate_public_comment_body(
		"decodex run failed and will retry\n\n- worktree_path: `.worktrees/DEC-1`",
	)
	.expect("repo-relative worktree path should be public");

	for (body, expected_error) in [
		(
			"decodex run failed and will retry\n\n- worktree_path: `/absolute/path/to/repo/.worktrees/DEC-1`",
			"`worktree_path` must be repository-relative",
		),
		(
			"decodex run failed and will retry\n\n- unexpected_path: `.worktrees/DEC-1`",
			"Unsupported structured field `unexpected_path`",
		),
	] {
		let error = public_text::validate_public_comment_body(body)
			.expect_err("private or unsupported comment path should be rejected");

		assert!(error.contains(expected_error), "{error}");
	}
}
