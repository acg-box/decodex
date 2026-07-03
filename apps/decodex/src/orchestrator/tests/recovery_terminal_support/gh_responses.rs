mod closeout;
mod conflicting;
mod merged;
mod open;
mod ready_to_land;

pub(in crate::orchestrator::tests) use self::{
	closeout::{
		install_fake_closeout_gh_responses, install_fake_closeout_gh_responses_with_state,
		install_fake_closeout_gh_responses_with_states,
	},
	conflicting::install_fake_conflicting_pr_gh_response,
	merged::{
		install_fake_merged_pr_gh_response, install_fake_merged_pr_gh_response_with_base_ref,
		install_fake_merged_pr_gh_response_with_delete_exit_code,
	},
	open::install_fake_open_pr_gh_response,
	ready_to_land::install_fake_ready_to_land_admin_merge_gh_response,
};
