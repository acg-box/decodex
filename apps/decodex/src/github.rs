mod branch;
mod command;
mod comments;
mod landing_state;
mod locator;
mod merge_readback;
mod pr_close;
mod ref_cleanup;
mod repository;
mod status;

pub(crate) use self::ref_cleanup::{RemoteRefDeleteReadback, reconcile_remote_ref_delete};
#[cfg(test)]
pub(crate) use self::{
	branch::{gh_delete_ref_missing_branch, github_api_ref_path},
	command::{GH_FALLBACK_PATHS, GhCommandDiscoveryTier, gh_command_resolution_from_env},
	pr_close::{PullRequestCloseReadback, reconcile_pull_request_close},
};
pub(crate) use self::{
	command::{
		GhCommandResolution, configure_gh_command, gh_command_resolution, gh_command_with_config,
	},
	comments::post_pull_request_issue_comment,
	landing_state::inspect_pull_request_landing_state,
	locator::parse_pull_request_url,
	merge_readback::{
		PullRequestMergeViewResponse, admin_merge_pull_request, inspect_pull_request_merge_commit,
		inspect_pull_request_merge_readback, pull_request_is_merged_at_head,
		wait_for_commit_subject, wait_for_pull_request_merge_commit,
	},
	status::{
		CommitStatusPublishRequest, CommitStatusState, commit_status_description_with_base_ref_oid,
		inspect_required_commit_status_contexts, publish_commit_status,
	},
};
pub(crate) use branch::delete_pull_request_head_branch_if_present;
#[cfg(test)]
pub(crate) use merge_readback::{
	commit_subject_wait_error_is_retryable, configure_admin_merge_command,
	merge_commit_wait_error_is_retryable,
};
pub(crate) use repository::{
	RepositoryContext, inspect_repository_context, pull_request_matches_repository,
};
#[cfg(test)]
mod tests;
