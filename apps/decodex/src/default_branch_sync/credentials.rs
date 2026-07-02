use crate::git_credentials::{GitCredentialEnvironment, GitCredentialSource};

pub(in crate::default_branch_sync) fn materialize_git_credentials(
	credentials: Option<GitCredentialSource<'_>>,
) -> GitCredentialEnvironment {
	let Some(credentials) = credentials else {
		return GitCredentialEnvironment::default();
	};

	credentials.materialize_github_credentials()
}
