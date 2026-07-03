pub(super) mod support;

mod contexts;
mod fixtures;
mod gh_fixtures;
mod git_helpers;
mod tests_authority;
mod tests_cleanup;
mod tests_landing;
mod tests_records;
mod tests_recovery;

pub(in crate::manual::tests) use self::{
	contexts::repo_root_manual_land_context,
	fixtures::{merged_manual_land_state, sample_issue, sample_landing_state, sample_workflow},
	gh_fixtures::{
		install_fake_admin_merge_gh, install_fake_admin_merge_gh_with_merge_exit_code,
		install_fake_landing_state_gh, install_fake_repo_view_gh,
	},
	git_helpers::{
		create_dirty_merged_worktree_debt, git_add_and_commit, git_success, init_git_checkout,
		init_git_checkout_with_origin, merge_manual_land_test_branch, remove_test_lane_checkout,
	},
};
