use crate::{
	github,
	manual::{self, LandExecutionMode, MANUAL_LAND_MERGE_VISIBILITY_TIMEOUT, ManualLandContext},
	prelude::{Result, eyre},
};

pub(in crate::manual) fn execute_land_merge(
	context: &ManualLandContext,
	current_head: &str,
	landed_change_record: &str,
	execution_mode: LandExecutionMode,
) -> Result<String> {
	match execution_mode {
		LandExecutionMode::MergeAndCloseout => {
			manual::ensure_clean_worktree(&context.cwd)?;

			if !context.repository.merge_commit_allowed {
				eyre::bail!(
					"GitHub repository `{}/{}` does not allow merge commits, but `decodex land` requires an admin merge commit.",
					context.repository.owner,
					context.repository.name
				);
			}

			if let Err(error) = github::admin_merge_pull_request(
				&context.canonical_repo_root,
				&context.pr_url,
				current_head,
				Some(landed_change_record),
				&context.github_token,
				context.github_command_path.as_deref(),
			) {
				if matches!(
					github::pull_request_is_merged_at_head(
						&context.canonical_repo_root,
						&context.pr_url,
						current_head,
						&context.github_token,
						context.github_command_path.as_deref(),
					),
					Ok(true)
				) {
					return github::wait_for_pull_request_merge_commit(
						&context.canonical_repo_root,
						&context.pr_url,
						&context.github_token,
						MANUAL_LAND_MERGE_VISIBILITY_TIMEOUT,
						context.github_command_path.as_deref(),
					);
				}

				return Err(error);
			}
		},
		LandExecutionMode::CloseoutOnly => {},
	}

	github::wait_for_pull_request_merge_commit(
		&context.canonical_repo_root,
		&context.pr_url,
		&context.github_token,
		MANUAL_LAND_MERGE_VISIBILITY_TIMEOUT,
		context.github_command_path.as_deref(),
	)
}

pub(in crate::manual) fn load_authoritative_landed_change_record(
	context: &ManualLandContext,
	merge_commit: &str,
) -> Result<String> {
	github::wait_for_commit_subject(
		&context.canonical_repo_root,
		&context.pr_url,
		merge_commit,
		&context.github_token,
		MANUAL_LAND_MERGE_VISIBILITY_TIMEOUT,
		context.github_command_path.as_deref(),
	)
}
