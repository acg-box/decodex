use std::{env, process::Command};

pub(crate) const DECODEX_ACTIVE_RUN_SERVICE_ID_ENV: &str = "DECODEX_ACTIVE_RUN_SERVICE_ID";
pub(crate) const DECODEX_ACTIVE_RUN_ID_ENV: &str = "DECODEX_ACTIVE_RUN_ID";
pub(crate) const DECODEX_ACTIVE_RUN_ISSUE_ID_ENV: &str = "DECODEX_ACTIVE_RUN_ISSUE_ID";
pub(crate) const DECODEX_ACTIVE_RUN_ISSUE_IDENTIFIER_ENV: &str =
	"DECODEX_ACTIVE_RUN_ISSUE_IDENTIFIER";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ActiveRunCommitContext {
	service_id: String,
	run_id: String,
	issue_id: String,
	issue_identifier: String,
}
impl ActiveRunCommitContext {
	pub(crate) fn new(
		service_id: String,
		run_id: String,
		issue_id: String,
		issue_identifier: String,
	) -> Self {
		Self { service_id, run_id, issue_id, issue_identifier }
	}

	pub(crate) fn from_process_env() -> Option<Self> {
		Some(Self {
			service_id: env::var(DECODEX_ACTIVE_RUN_SERVICE_ID_ENV).ok()?,
			run_id: env::var(DECODEX_ACTIVE_RUN_ID_ENV).ok()?,
			issue_id: env::var(DECODEX_ACTIVE_RUN_ISSUE_ID_ENV).ok()?,
			issue_identifier: env::var(DECODEX_ACTIVE_RUN_ISSUE_IDENTIFIER_ENV).ok()?,
		})
	}

	pub(crate) fn apply_to(&self, command: &mut Command) {
		command
			.env(DECODEX_ACTIVE_RUN_SERVICE_ID_ENV, &self.service_id)
			.env(DECODEX_ACTIVE_RUN_ID_ENV, &self.run_id)
			.env(DECODEX_ACTIVE_RUN_ISSUE_ID_ENV, &self.issue_id)
			.env(DECODEX_ACTIVE_RUN_ISSUE_IDENTIFIER_ENV, &self.issue_identifier);
	}

	pub(crate) fn service_id(&self) -> &str {
		&self.service_id
	}

	pub(crate) fn run_id(&self) -> &str {
		&self.run_id
	}

	pub(crate) fn issue_id(&self) -> &str {
		&self.issue_id
	}

	pub(crate) fn issue_identifier(&self) -> &str {
		&self.issue_identifier
	}
}
