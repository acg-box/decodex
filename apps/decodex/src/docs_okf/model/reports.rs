use std::path::PathBuf;

use crate::docs_okf::{DocsCheckScope, OkfCheckProfile};

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct DocsCheckReport {
	pub(in crate::docs_okf) scope: DocsCheckScope,
	pub(in crate::docs_okf) docs_root: PathBuf,
	pub(in crate::docs_okf) concept_count: usize,
	pub(in crate::docs_okf) link_count: usize,
	pub(in crate::docs_okf) issues: Vec<DocsCheckIssue>,
}
impl DocsCheckReport {
	/// Return whether the check found at least one docs issue.
	pub(crate) fn has_issues(&self) -> bool {
		!self.issues.is_empty()
	}
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct OkfCheckReport {
	pub(in crate::docs_okf) profile: OkfCheckProfile,
	pub(in crate::docs_okf) bundle_root: PathBuf,
	pub(in crate::docs_okf) concept_count: usize,
	pub(in crate::docs_okf) link_count: usize,
	pub(in crate::docs_okf) issues: Vec<DocsCheckIssue>,
}
impl OkfCheckReport {
	/// Return whether the check found at least one OKF issue.
	pub(crate) fn has_issues(&self) -> bool {
		!self.issues.is_empty()
	}

	/// Return the profile used for this OKF check.
	pub(crate) fn profile(&self) -> OkfCheckProfile {
		self.profile
	}
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct OkfInitReport {
	pub(in crate::docs_okf) profile: OkfCheckProfile,
	pub(in crate::docs_okf) bundle_root: PathBuf,
	pub(in crate::docs_okf) created: Vec<PathBuf>,
	pub(in crate::docs_okf) unchanged: Vec<PathBuf>,
}
impl OkfInitReport {
	/// Return the profile used for this OKF init.
	pub(crate) fn profile(&self) -> OkfCheckProfile {
		self.profile
	}
}

#[derive(Debug, Eq, PartialEq)]
pub(in crate::docs_okf) struct DocsCheckIssue {
	pub(in crate::docs_okf) path: Option<PathBuf>,
	pub(in crate::docs_okf) message: String,
}
