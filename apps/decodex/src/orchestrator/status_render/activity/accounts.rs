use crate::orchestrator::{self, CodexAccountActivitySummary, status_render::activity};

pub(crate) fn render_account_summary(summary: Option<&CodexAccountActivitySummary>) -> String {
	let Some(summary) = summary else {
		return String::from("none");
	};
	let plan = summary.plan_type.as_deref().unwrap_or("unknown");
	let reached = summary.rate_limit_reached_type.as_deref().unwrap_or("none");
	let credits = render_codex_account_credits(summary);
	let token_status = render_codex_account_token_status(&summary.refresh_status);
	let primary = render_codex_account_window(
		summary.primary_window_seconds,
		summary.primary_remaining_percent,
		summary.primary_resets_at_unix_epoch,
	);
	let secondary = render_codex_account_window(
		summary.secondary_window_seconds,
		summary.secondary_remaining_percent,
		summary.secondary_resets_at_unix_epoch,
	);

	format!(
		"account={}; plan={plan}; status={}; token={token_status}; primary={primary}; secondary={secondary}; credits={credits}; reached={reached}",
		summary.account_fingerprint, summary.status,
	)
}

pub(crate) fn render_accounts_summary(accounts: &[CodexAccountActivitySummary]) -> String {
	if accounts.is_empty() {
		return String::from("none");
	}

	accounts
		.iter()
		.map(|summary| render_account_summary(Some(summary)))
		.collect::<Vec<_>>()
		.join(" | ")
}

fn render_codex_account_window(
	window_seconds: Option<i64>,
	remaining_percent: Option<i64>,
	resets_at_unix_epoch: Option<i64>,
) -> String {
	let label = window_seconds.map(codex_window_label).unwrap_or_else(|| String::from("window"));
	let remaining =
		remaining_percent.map_or_else(|| String::from("unknown"), |value| format!("{value}%"));
	let reset = orchestrator::format_optional_unix_timestamp(resets_at_unix_epoch)
		.unwrap_or_else(|| String::from("unknown"));

	format!("{label} remaining={remaining} reset={reset}")
}

fn render_codex_account_credits(summary: &CodexAccountActivitySummary) -> String {
	if summary.credits_unlimited == Some(true) {
		return String::from("unlimited");
	}

	match (summary.credits_has_credits, summary.credits_balance.as_deref()) {
		(Some(false), Some(balance)) => format!("depleted balance={balance}"),
		(Some(false), None) => String::from("depleted"),
		(_, Some(balance)) => format!("balance={balance}"),
		(Some(true), None) => String::from("available"),
		(None, None) => String::from("unknown"),
	}
}

fn render_codex_account_token_status(refresh_status: &str) -> &'static str {
	match refresh_status {
		"not_needed" | "none" => "ok",
		"succeeded" | "refreshed" => "refreshed",
		"failed" => "refresh_failed",
		_ => "unknown",
	}
}

fn codex_window_label(window_seconds: i64) -> String {
	match window_seconds {
		18_000 => String::from("5h"),
		604_800 => String::from("7d"),
		seconds => activity::format_seconds_compact(seconds),
	}
}
