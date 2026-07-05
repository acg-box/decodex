use crate::orchestrator::{self, tests};

#[test]
fn repo_gate_selects_matching_profile_for_scoped_lane_changes() {
	let (temp_dir, config, workflow) = tests::temp_project_layout_with_workflow_markdown(
		&tests::profile_scoped_workflow_markdown("pubfi"),
	);
	let repo_root = config.repo_root();
	let remote_root = temp_dir.path().join("origin.git");

	tests::add_origin_remote(repo_root, &remote_root);
	tests::checkout_new_branch(repo_root, "config-subset");
	tests::commit_worktree_change(
		repo_root,
		"config/new-surface.toml",
		"name = \"new-surface\"\n",
		"config subset change",
	);

	let selection =
		orchestrator::select_repo_gate_for_worktree(workflow.frontmatter().execution(), repo_root);

	assert_eq!(selection.profile_name(), Some("config_subset"));
	assert!(selection.canonicalize_commands().is_empty());
	assert_eq!(selection.verify_commands(), ["python3 -c 'print(\"ok\")'"]);
}
