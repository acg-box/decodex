use crate::git_credentials::GitCredentialEnvironment;

#[derive(Clone, Copy)]
pub(crate) struct GitCredentialSource<'a> {
	token_env_var: &'a str,
	token: &'a str,
}
impl<'a> GitCredentialSource<'a> {
	pub(crate) fn new(token_env_var: &'a str, token: &'a str) -> Self {
		Self { token_env_var, token }
	}

	pub(crate) fn materialize_github_credentials(self) -> GitCredentialEnvironment {
		GitCredentialEnvironment::with_github_credentials(
			self.token_env_var.to_owned(),
			self.token.to_owned(),
		)
	}
}
