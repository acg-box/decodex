use crate::orchestrator::{self, tests};

#[test]
fn repo_gate_falls_back_to_full_gate_when_changed_file_classification_is_unavailable() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout_with_workflow_markdown(
		&tests::profile_scoped_workflow_markdown("pubfi"),
	);
	let repo_root = config.repo_root();

	tests::checkout_new_branch(repo_root, "config-subset");
	tests::commit_worktree_change(
		repo_root,
		"config/new-surface.toml",
		"name = \"new-surface\"\n",
		"config subset change",
	);

	let selection =
		orchestrator::select_repo_gate_for_worktree(workflow.frontmatter().execution(), repo_root);

	assert_eq!(selection.profile_name(), None);
	assert_eq!(selection.canonicalize_commands(), ["cargo make fmt", "cargo make lint-fix"]);
	assert_eq!(selection.verify_commands(), ["cargo make check"]);
}
