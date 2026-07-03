mod activity;
mod auth_failure;
mod login;
mod pool;
mod record;
mod refresh;
mod selection;
mod storage;
mod usage;

pub(crate) use self::{
	auth_failure::CodexAccountAuthFailure,
	login::CodexAccountLogin,
	pool::{CodexAccountPool, CodexAccountProvider},
};
#[cfg(test)] pub(crate) use crate::state::CodexAccountActivitySummary;

#[cfg(test)] use std::path::Path;
use std::time::Duration;

use self::record::AccountPoolRecord;
#[cfg(test)]
use self::{
	refresh::{CodexTokenData, ProactiveRefreshReason},
	selection::compare_account_candidates,
	usage::{
		CreditsSnapshot, UsageWindow, profile_snapshot_from_payload, usage_snapshot_from_payload,
	},
};
const DEFAULT_USAGE_ENDPOINT: &str = "https://chatgpt.com/backend-api/wham/usage";
const DEFAULT_PROFILE_ENDPOINT: &str = "https://chatgpt.com/backend-api/wham/profiles/me";
const DEFAULT_REFRESH_ENDPOINT: &str = "https://auth.openai.com/oauth/token";
const CODEX_USER_AGENT: &str = "codex-cli";
const CHATGPT_OAUTH_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
const TOKEN_REFRESH_INTERVAL_SECONDS: i64 = 8 * 24 * 60 * 60;

#[cfg(test)] mod tests;
