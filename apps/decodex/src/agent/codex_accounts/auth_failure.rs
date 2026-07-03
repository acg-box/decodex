use std::{
	error::Error,
	fmt::{Display, Formatter},
};

use crate::agent::codex_accounts::AccountPoolRecord;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CodexAccountAuthFailure {
	account_fingerprint: Option<String>,
	email: Option<String>,
	reason: String,
}
impl CodexAccountAuthFailure {
	pub(in crate::agent::codex_accounts) fn from_record(
		record: &AccountPoolRecord,
		reason: impl Into<String>,
	) -> Self {
		Self::new(record.account_fingerprint(), record.email(), reason)
	}

	pub(crate) fn new(
		account_fingerprint: Option<String>,
		email: Option<String>,
		reason: impl Into<String>,
	) -> Self {
		Self { account_fingerprint, email, reason: reason.into() }
	}

	pub(crate) const fn error_class(&self) -> &'static str {
		"codex_account_auth_failed"
	}

	pub(crate) fn account_fingerprint(&self) -> Option<&str> {
		self.account_fingerprint.as_deref()
	}

	pub(crate) fn email(&self) -> Option<&str> {
		self.email.as_deref()
	}

	pub(crate) fn reason(&self) -> &str {
		&self.reason
	}

	fn account_label(&self) -> String {
		self.email
			.clone()
			.or_else(|| self.account_fingerprint.clone())
			.unwrap_or_else(|| String::from("unknown account"))
	}

	pub(crate) fn terminal_next_action(&self, recovery_gate: &str) -> String {
		format!(
			"re-login or remove Decodex Codex account `{}`, verify `decodex account list --json` reports no `auth_failed` selected account, then {recovery_gate}",
			self.account_label()
		)
	}
}

impl Display for CodexAccountAuthFailure {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		write!(
			formatter,
			"Codex account `{}` authentication failed: {}",
			self.account_label(),
			self.reason
		)
	}
}

impl Error for CodexAccountAuthFailure {}
