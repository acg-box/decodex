use std::{cell::RefCell, path::Path};

use crate::agent::tracker_tool_bridge::{
	LocalRepoDetails, LocalRepoInspector, PullRequestDetails, PullRequestInspector,
};

pub(crate) struct FakePullRequestInspector {
	pub(crate) responses: RefCell<Vec<std::result::Result<PullRequestDetails, String>>>,
}
impl FakePullRequestInspector {
	pub(crate) fn new(responses: Vec<std::result::Result<PullRequestDetails, String>>) -> Self {
		Self { responses: RefCell::new(responses) }
	}
}

impl PullRequestInspector for FakePullRequestInspector {
	fn inspect_pull_request(
		&self,
		_cwd: &Path,
		_pr_url: &str,
		_github_token: &str,
		_gh_command_path: Option<&Path>,
	) -> std::result::Result<PullRequestDetails, String> {
		self.responses.borrow_mut().remove(0)
	}
}

pub(crate) struct GitHubTokenAssertingPullRequestInspector {
	pub(crate) expected_token: String,
	pub(crate) response: PullRequestDetails,
}
impl PullRequestInspector for GitHubTokenAssertingPullRequestInspector {
	fn inspect_pull_request(
		&self,
		_cwd: &Path,
		_pr_url: &str,
		github_token: &str,
		_gh_command_path: Option<&Path>,
	) -> std::result::Result<PullRequestDetails, String> {
		assert_eq!(github_token, self.expected_token.as_str());

		Ok(self.response.clone())
	}
}

pub(crate) struct FakeLocalRepoInspector {
	pub(crate) responses: RefCell<Vec<std::result::Result<LocalRepoDetails, String>>>,
}
impl FakeLocalRepoInspector {
	pub(crate) fn new(responses: Vec<std::result::Result<LocalRepoDetails, String>>) -> Self {
		Self { responses: RefCell::new(responses) }
	}
}

impl LocalRepoInspector for FakeLocalRepoInspector {
	fn inspect_local_repo(&self, _cwd: &Path) -> std::result::Result<LocalRepoDetails, String> {
		let mut responses = self.responses.borrow_mut();

		match responses.len() {
			0 => panic!("fake local repo inspector ran out of responses"),
			1 => responses[0].clone(),
			_ => responses.remove(0),
		}
	}
}
