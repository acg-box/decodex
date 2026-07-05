use std::path::PathBuf;

use crate::{agent::codex_accounts::AccountPoolRecord, state::CodexAccountActivitySummary};

#[derive(Clone, Eq, PartialEq)]
pub(in crate::agent::codex_accounts::activity) struct AccountActivityCacheKey {
	pub(in crate::agent::codex_accounts::activity) path: PathBuf,
	pub(in crate::agent::codex_accounts::activity) usage_endpoint: String,
	pub(in crate::agent::codex_accounts::activity) profile_endpoint: Option<String>,
	pub(in crate::agent::codex_accounts::activity) reset_credits_endpoint: String,
	pub(in crate::agent::codex_accounts::activity) refresh_endpoint: String,
}

#[derive(Clone)]
pub(in crate::agent::codex_accounts::activity) struct AccountActivityCacheEntry {
	pub(in crate::agent::codex_accounts::activity) key: AccountActivityCacheKey,
	pub(in crate::agent::codex_accounts::activity) checked_at_unix_epoch: i64,
	pub(in crate::agent::codex_accounts::activity) summaries: Vec<CodexAccountActivitySummary>,
}

#[derive(Clone)]
pub(in crate::agent::codex_accounts::activity) struct AccountActivityProbeInput {
	pub(in crate::agent::codex_accounts::activity) index: usize,
	pub(in crate::agent::codex_accounts::activity) record: AccountPoolRecord,
}

pub(in crate::agent::codex_accounts::activity) struct AccountActivityProbeResult {
	pub(in crate::agent::codex_accounts::activity) index: usize,
	pub(in crate::agent::codex_accounts::activity) record: AccountPoolRecord,
	pub(in crate::agent::codex_accounts::activity) summary: CodexAccountActivitySummary,
	pub(in crate::agent::codex_accounts::activity) records_changed: bool,
}
