use std::path::Path;

use crate::{
	manual::{ManualAuthority, ManualLandContext, RepositoryContext, tests::fixtures},
	tracker::privacy_classifier::ConfiguredPublicProjectionPrivacyClassifier,
};

pub(in crate::manual::tests) fn repo_root_manual_land_context(
	repo_root: &Path,
	worktree_root: &Path,
) -> ManualLandContext {
	ManualLandContext {
		cwd: repo_root.to_path_buf(),
		current_branch: String::from("main"),
		worktree_root: repo_root.to_path_buf(),
		project_worktree_root: worktree_root.to_path_buf(),
		canonical_repo_root: repo_root.to_path_buf(),
		authority: ManualAuthority::Manual,
		service_id: String::from("decodex"),
		workflow: Some(fixtures::sample_workflow()),
		github_token_env_var: String::from("GITHUB_TOKEN"),
		github_token: String::from("test-token"),
		github_command_path: None,
		repository: RepositoryContext {
			owner: String::from("hack-ink"),
			name: String::from("decodex"),
			default_branch: String::from("main"),
			merge_commit_allowed: true,
		},
		prepared_closeout: None,
		review_handoff: None,
		pr_url: String::from("https://github.com/hack-ink/decodex/pull/64"),
		review_branch: String::from("main"),
		public_projection_privacy_classifier: ConfiguredPublicProjectionPrivacyClassifier::Disabled,
	}
}
