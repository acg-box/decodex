use crate::orchestrator::tests::operator::status::running_lanes::{
	self, StateStore, fs, orchestrator,
};

#[test]
fn operator_status_snapshot_surfaces_merged_dirty_ad_hoc_worktree() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_path = config.worktree_root().join("accounts-column-format");

	running_lanes::git_status_success(
		config.repo_root(),
		&[
			"worktree",
			"add",
			"-b",
			"xy/accounts-column-format",
			worktree_path.to_str().expect("worktree path should be UTF-8"),
			"main",
		],
	);
	running_lanes::commit_worktree_change(
		&worktree_path,
		"README.md",
		"feature work\n",
		"feature work",
	);
	running_lanes::git_status_success(
		config.repo_root(),
		&["merge", "--no-ff", "xy/accounts-column-format", "-m", "land feature"],
	);
	fs::write(worktree_path.join("README.md"), "dirty after land\n")
		.expect("worktree file should become dirty");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let worktree = snapshot
		.worktrees
		.iter()
		.find(|worktree| worktree.worktree_path == ".worktrees/accounts-column-format")
		.expect("ad-hoc merged dirty worktree should be surfaced");

	assert!(snapshot.warnings.contains(&String::from("merged_worktree_cleanup_pending")));
	assert!(snapshot.warnings.contains(&String::from("merged_dirty_worktree")));
	assert_eq!(worktree.branch_name, "xy/accounts-column-format");
	assert_eq!(worktree.ownership, "post_land_cleanup");
	assert!(
		worktree.ownership_reason.contains("already merged into `main`"),
		"ownership reason should explain why the worktree is no longer usable"
	);
	assert!(
		worktree.hygiene.as_ref().is_some_and(|hygiene| hygiene.dirty),
		"hygiene state should mark the local changes"
	);

	let project = snapshot.projects.first().expect("project summary should exist");

	assert_eq!(project.attention_count, 0);
	assert_eq!(project.cleanup_blocked_count, 1);
	assert_eq!(project.cleanup_pending_count, 0);

	let error = orchestrator::ensure_project_has_no_merged_worktree_cleanup_debt(&config)
		.expect_err("normal automation should stop while merged dirty worktrees remain");

	assert!(error.to_string().contains("Post-land worktree cleanup is pending"));
}

#[test]
fn operator_status_snapshot_explains_unavailable_worktree_hygiene() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");

	fs::remove_dir_all(config.repo_root().join(".git"))
		.expect("repo metadata should be removable for the fixture");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should degrade instead of failing");
	let detail = snapshot
		.warning_details
		.iter()
		.find(|detail| detail.warning == "worktree_hygiene_unavailable")
		.expect("hygiene warning should include operator-facing detail");

	assert!(snapshot.warnings.contains(&String::from("worktree_hygiene_unavailable")));
	assert_eq!(detail.project_id.as_deref(), Some("pubfi"));

	let repo_root = config.repo_root().display().to_string();

	assert_eq!(detail.repo_root.as_deref(), Some(repo_root.as_str()));
	assert!(detail.reason.contains("not a git repository"));
	assert!(
		detail
			.next_action
			.as_deref()
			.is_some_and(|action| action.contains("Remove the stale project registration")),
		"detail should tell the operator how to clear a stale project registration"
	);

	let rendered = orchestrator::render_operator_status(&snapshot);

	assert!(rendered.contains("project=pubfi"));
	assert!(rendered.contains("repo_root="));
	assert!(rendered.contains("Remove the stale project registration"));
}

#[test]
fn operator_status_snapshot_updates_owned_merged_worktree_hygiene_without_global_warning() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue("Done", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");

	running_lanes::git_status_success(
		config.repo_root(),
		&[
			"worktree",
			"add",
			"-b",
			"xy/pub-101-cleanup",
			worktree_path.to_str().expect("worktree path should be UTF-8"),
			"main",
		],
	);
	running_lanes::commit_worktree_change(
		&worktree_path,
		"README.md",
		"feature work\n",
		"feature work",
	);
	running_lanes::git_status_success(
		config.repo_root(),
		&["merge", "--no-ff", "xy/pub-101-cleanup", "-m", "land feature"],
	);
	fs::write(worktree_path.join("README.md"), "dirty after land\n")
		.expect("worktree file should become dirty");

	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"xy/pub-101-cleanup",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let worktree = snapshot
		.worktrees
		.iter()
		.find(|worktree| worktree.worktree_path == ".worktrees/PUB-101")
		.expect("owned merged worktree should still be visible");

	assert!(!snapshot.warnings.contains(&String::from("merged_worktree_cleanup_pending")));
	assert!(!snapshot.warnings.contains(&String::from("merged_dirty_worktree")));
	assert!(
		worktree.hygiene.as_ref().is_some_and(|hygiene| hygiene.dirty),
		"hygiene should still surface on the owned worktree row"
	);

	let project = snapshot.projects.first().expect("project summary should exist");

	assert_eq!(project.attention_count, 0);
	assert_eq!(project.cleanup_blocked_count, 1);
	assert_eq!(project.cleanup_pending_count, 0);
}
