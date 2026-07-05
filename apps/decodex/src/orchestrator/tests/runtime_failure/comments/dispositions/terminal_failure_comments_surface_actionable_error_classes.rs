use crate::orchestrator::tests::runtime_failure::orchestrator;

#[test]
fn terminal_failure_comments_surface_actionable_error_classes() {
	for (error_class, next_action, expected_snippets) in [
		(
			"human_attention_required",
			"inspect the issue comment and worktree, resolve the blocker manually, clear label `decodex:needs-attention`, then move the issue back to a startable state if another automated run is desired",
			&["inspect the issue comment and worktree", "resolve the blocker manually"][..],
		),
		(
			"review_handoff_writeback_failed",
			"inspect the tracker state, PR, and worktree, repair the incomplete review handoff manually, clear label `decodex:needs-attention`, then move the issue back to a startable state if another automated run is desired",
			&["repair the incomplete review handoff manually"][..],
		),
		(
			"stalled_run_detected",
			"inspect the worktree and app-server activity for the stalled lane, resolve the blocker manually, `decodex:needs-attention` could not be applied because it does not exist on the team; the issue remains in `In Progress` to block automatic retries, so move it back to a startable state manually if another automated run is desired",
			&["does not exist on the team", "remains in `In Progress`"][..],
		),
	] {
		let comment = orchestrator::format_terminal_failure_comment(
			"pub-101-attempt-1-123",
			1,
			String::from(".worktrees/PUB-101"),
			"x/pubfi-pub-101",
			None,
			error_class,
			next_action,
		);

		assert!(comment.contains(&format!("- error_class: `{error_class}`")));
		assert!(comment.contains("Sensitive runtime details were withheld"));

		for expected_snippet in expected_snippets {
			assert!(comment.contains(expected_snippet), "{error_class} missing {expected_snippet}");
		}
	}
}
