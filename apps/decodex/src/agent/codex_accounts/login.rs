use crate::state::CodexAccountActivitySummary;

pub(crate) struct CodexAccountLogin {
	pub(in crate::agent::codex_accounts) access_token: String,
	pub(in crate::agent::codex_accounts) account_id: String,
	pub(in crate::agent::codex_accounts) plan_type: Option<String>,
	pub(in crate::agent::codex_accounts) last_selected_at_unix_epoch: Option<i64>,
	pub(in crate::agent::codex_accounts) summary: CodexAccountActivitySummary,
	pub(in crate::agent::codex_accounts) account_summaries: Vec<CodexAccountActivitySummary>,
}
impl CodexAccountLogin {
	pub(crate) fn access_token(&self) -> &str {
		&self.access_token
	}

	pub(crate) fn account_id(&self) -> &str {
		&self.account_id
	}

	pub(crate) fn plan_type(&self) -> Option<&str> {
		self.plan_type.as_deref()
	}

	pub(crate) fn summary(&self) -> &CodexAccountActivitySummary {
		&self.summary
	}

	pub(crate) fn account_summaries(&self) -> &[CodexAccountActivitySummary] {
		&self.account_summaries
	}

	pub(in crate::agent::codex_accounts) fn mark_selected(&mut self, selected_at_unix_epoch: i64) {
		if self.summary.status == "available" {
			self.summary.status = String::from("selected");
		}

		self.summary.selected_at_unix_epoch = Some(selected_at_unix_epoch);
	}

	pub(in crate::agent::codex_accounts) fn with_account_summaries(
		mut self,
		account_summaries: Vec<CodexAccountActivitySummary>,
	) -> Self {
		self.account_summaries = account_summaries;

		self
	}
}
