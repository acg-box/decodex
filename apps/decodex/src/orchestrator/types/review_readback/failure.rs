use crate::orchestrator::types::{ErrorKind, Report};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PullRequestReadbackRootCause {
	MissingGithubCli,
	MissingGithubToken,
	GithubAuthFailed,
	GithubApiReadFailed,
	GithubResponseParseFailed,
	PullRequestShapeReadFailed,
	LineageValidationFailed,
	TrackerIssueReadbackFailed,
}
impl PullRequestReadbackRootCause {
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::MissingGithubCli => "missing_github_cli",
			Self::MissingGithubToken => "missing_github_token",
			Self::GithubAuthFailed => "github_auth_failed",
			Self::GithubApiReadFailed => "github_api_read_failed",
			Self::GithubResponseParseFailed => "github_response_parse_failed",
			Self::PullRequestShapeReadFailed => "pull_request_shape_read_failed",
			Self::LineageValidationFailed => "lineage_validation_failed",
			Self::TrackerIssueReadbackFailed => "tracker_issue_readback_failed",
		}
	}
}

#[derive(Debug)]
pub(crate) struct PullRequestReadbackFailure {
	pub(crate) root_cause: PullRequestReadbackRootCause,
	pub(crate) error: Report,
}
impl PullRequestReadbackFailure {
	pub(crate) fn from_report(error: Report) -> Self {
		let root_cause = classify_pull_request_readback_report(&error);

		Self { root_cause, error }
	}

	pub(crate) fn into_report(self) -> Report {
		self.error
	}

	pub(crate) fn root_cause(&self) -> PullRequestReadbackRootCause {
		self.root_cause
	}
}

impl From<Report> for PullRequestReadbackFailure {
	fn from(error: Report) -> Self {
		Self::from_report(error)
	}
}

pub(crate) fn classify_pull_request_readback_report(
	error: &Report,
) -> PullRequestReadbackRootCause {
	if report_has_io_error_kind(error, ErrorKind::NotFound) {
		return PullRequestReadbackRootCause::MissingGithubCli;
	}
	if report_contains_any(
		error,
		&[
			"must be configured for this github-backed operation",
			"failed to read environment variable",
			"must not be blank",
		],
	) {
		return PullRequestReadbackRootCause::MissingGithubToken;
	}
	if report_chain_has_serde_json_error(error) {
		return PullRequestReadbackRootCause::GithubResponseParseFailed;
	}
	if report_contains_any(
		error,
		&[
			"pull request url",
			"did not include a repository",
			"did not include a pull request",
			"without an end cursor",
		],
	) {
		return PullRequestReadbackRootCause::PullRequestShapeReadFailed;
	}
	if report_contains_any(
		error,
		&[
			"bad credentials",
			"requires authentication",
			"authentication required",
			"not logged in",
			"gh auth login",
			"http 401",
			"http 403",
		],
	) {
		return PullRequestReadbackRootCause::GithubAuthFailed;
	}

	PullRequestReadbackRootCause::GithubApiReadFailed
}

fn report_has_io_error_kind(error: &Report, kind: ErrorKind) -> bool {
	error.chain().any(|cause| {
		cause.downcast_ref::<std::io::Error>().is_some_and(|error| error.kind() == kind)
	})
}

fn report_chain_has_serde_json_error(error: &Report) -> bool {
	error.chain().any(|cause| cause.downcast_ref::<serde_json::Error>().is_some())
}

fn report_contains_any(error: &Report, needles: &[&str]) -> bool {
	error.chain().any(|cause| {
		let message = cause.to_string().to_ascii_lowercase();

		needles.iter().any(|needle| message.contains(needle))
	})
}
