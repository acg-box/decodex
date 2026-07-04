use tempfile::TempDir;

use crate::{
	recovery::{
		RecoveryRuntimeMutationPolicy,
		tests::{self, FinalNeedsAttentionTracker, stale_active::release},
	},
	tracker,
};

#[test]
fn stale_active_release_revalidates_needs_attention_before_final_label_removal() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let context = tests::sample_recovery_context(
		&temp_dir,
		RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
	);
	let active_label = tracker::automation_active_label(context.config.service_id());
	let needs_attention_label =
		context.workflow.frontmatter().tracker().needs_attention_label().to_owned();
	let mut issue = tests::sample_issue_with_labels("Todo", &[active_label]);

	issue.identifier = String::from("PUB-1626");

	context
		.state_store
		.record_run_attempt("run-1626", &issue.id, 1, "running")
		.expect("run attempt should record");
	context
		.state_store
		.upsert_worktree(
			context.config.service_id(),
			&issue.id,
			"x/pubfi-pub-1626",
			&context.config.worktree_root().join("PUB-1626").display().to_string(),
		)
		.expect("worktree mapping should record");

	let tracker = FinalNeedsAttentionTracker::new(issue, needs_attention_label);
	let mut diagnostics = release::diagnose_stale_active_issues(
		context.config.service_id(),
		&context.workflow,
		context.config.worktree_root(),
		&context.state_store,
		&tracker,
		Some("PUB-1626"),
		RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
	)
	.expect("initial stale active diagnosis should run");
	let diagnostic = diagnostics.pop().expect("diagnostic should exist");

	assert!(diagnostic.recoverable());

	let error = release::apply_stale_active_release_with_tracker(
		&tracker,
		&context.config,
		&context.workflow,
		&context.state_store,
		&diagnostic,
	)
	.expect_err("late needs-attention should block active-label release");
	let message = error.to_string();

	assert!(message.contains("safety inspection changed before apply"));
	assert!(message.contains("needs_attention_label_present"));
	assert!(
		tracker.label_removals.borrow().is_empty(),
		"active label should not be removed after late needs-attention appears"
	);
}
