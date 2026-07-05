use crate::orchestrator::tests::operator::status::dashboard;

#[test]
fn operator_dashboard_accounts_keeps_debug_credit_and_reset_copy_compact() {
	let response = dashboard::dashboard_response();

	assert_account_debug_and_credit_copy(&response);
	assert_account_reset_copy(&response);
	assert_account_pool_render_copy(&response);
}

fn assert_account_debug_and_credit_copy(response: &str) {
	let active_debug = response
		.split("<summary>Debug Details</summary>")
		.nth(1)
		.expect("active debug details should exist")
		.split("</details>")
		.next()
		.expect("active debug details should end");

	assert!(!active_debug.contains("field(\"Account\", codexAccountDebugSummary(account))"));
	assert!(!active_debug.contains("field(\"Freshness source\","));
	assert!(!active_debug.contains("field(\"Lane activity\","));
	assert!(!active_debug.contains("field(\"Last protocol activity\","));
	assert!(
		!response.contains("field(\"Accounts\", codexAccountPoolDebugSummary(codexAccounts(run)))")
	);
	assert!(
		response
			.contains("field(\"Account\", codexAccountDebugSummary(codexAccount(run, snapshot)))")
	);
	assert!(
		response
			.contains("facts.push([\"Account\", codexAccountHistorySummary(codexAccount(run))])")
	);
	assert!(!response.contains("facts.push([\"Codex pool\""));
	assert!(!response.contains("account <strong>"));
	assert!(response.contains("credits_unlimited"));
	assert!(response.contains("function formatCodexAccountCreditsBalance(value)"));
	assert!(
		response
			.contains("const balance = formatCodexAccountCreditsBalance(account.credits_balance);")
	);
	assert!(response.contains("return number.toFixed(2);"));
	assert!(!response.contains(".replace(/\\.00$/, \"\")"));
	assert!(!response.contains(".replace(/(\\.\\d)0$/, \"$1\")"));
	assert!(response.contains("function codexAccountCreditsTone(account)"));
	assert!(response.contains("function codexAccountUsageLimited(account)"));
	assert!(response.contains("if (status === \"available\")"));
	assert!(response.contains("return \"ready\";"));
	assert!(response.contains("codexAccountReachedType(account).includes(\"credit\")"));
	assert!(response.contains("const credits = codexAccountCreditsSummary(account);"));
	assert!(response.contains("const resetCredits = codexAccountResetCreditsSummary(account);"));
	assert!(response.contains("const creditValue = credits || \"-\";"));
	assert!(response.contains("reset cards ${resetCredits}"));
	assert!(response.contains("const creditTone = codexAccountCreditsTone(account);"));
	assert!(!response.contains("<span>credits</span>"));
	assert!(response.contains("<strong>${escapeHtml(creditValue)}</strong>"));
	assert!(response.contains("account-row-reset-credit"));
	assert!(response.contains("function codexAccountResetCreditCompactTimestamp(value)"));
	assert!(response.contains("const CODEX_ACCOUNT_RESET_CREDIT_LOCALE = \"en-US\";"));
	assert!(response.contains("const CODEX_ACCOUNT_RESET_CREDIT_TIME_ZONE = \"Asia/Shanghai\";"));
	assert!(response.contains("function codexAccountResetCreditExpiry(card)"));
	assert!(response.contains("return codexAccountResetCreditCompactTimestamp(card.expiresAt);"));
	assert!(!response.contains("return `${granted} -> ${expires}`;"));
	assert!(response.contains("${cards\n\t\t\t\t\t\t\t\t\t.map((card) => {"));
	assert!(!response.contains("cards.slice(0, 5)"));
	assert!(!response.contains("account-reset-credit-more"));

	let account_credit_index = response
		.find("<div class=\"account-row-credit${creditClass}\"")
		.expect("account credit cell render");
	let account_status_index =
		response.find("<div class=\"account-row-state\">").expect("account status cell render");

	assert!(account_credit_index < account_status_index);
	assert!(response.contains("return \"0.00\";"));
	assert!(!response.contains("return \"No Credits\";"));
	assert!(response.contains("return \"Unlimited\";"));

	let account_status_label = response
		.split("function codexAccountStatusLabel(account)")
		.nth(1)
		.expect("account status label function should exist")
		.split("function codexAccountCreditsSummary(account)")
		.next()
		.expect("account status label function should have an end");

	assert!(account_status_label.contains("return refresh;"));
	assert!(account_status_label.contains("return displayToken(status);"));
	assert!(!account_status_label.contains("Refresh failed"));
	assert!(!account_status_label.contains("Ready"));
	assert!(response.contains("return codexAccountTokenValue(account.refresh_status);"));
	assert!(response.contains("return \"-\";"));
	assert!(!response.contains("depleted"));
	assert!(response.contains("rate_limit_reached_type"));
	assert!(response.contains("if (codexAccountUsageLimited(account))"));
	assert!(account_status_label.contains("return reached || (String(status).trim() && status !== \"available\" ? status : \"usage_limited\");"));
}

fn assert_account_reset_copy(response: &str) {
	assert!(response.contains("cooldown_until_unix_epoch"));
	assert!(response.contains("`${prefix}_remaining_percent`"));
	assert!(response.contains("`${prefix}_resets_at_unix_epoch`"));
	assert!(response.contains("value === 18_000"));
	assert!(response.contains("value === 604_800"));
	assert!(response.contains("function formatCodexAccountResetDuration(seconds)"));
	assert!(response.contains("function codexAccountResetDistance(value)"));
	assert!(response.contains("function codexAccountResetDisplay(data)"));
	assert!(!response.contains("const shortWindow = windowSeconds === 18_000;"));
	assert!(
		response.contains("return { short: \"0m\", phrase: \"reset due now\", isPast: true };")
	);
	assert!(response.contains("return { short, phrase: `resets in ${short}`, isPast: false };"));
	assert!(response.contains("date: \"\","));
	assert!(response.contains("date: resetAt,"));
	assert!(response.contains("aria: \"reset unavailable\","));
	assert!(response.contains("reset at ${resetAt}, ${distance.phrase}"));
	assert!(
		response.contains("data.remainingPercent == null ? \"-\" : `${data.remainingPercent}%`;")
	);
	assert!(response.contains("aria-label=\"${escapeHtml(label)} usage unavailable\""));
	assert!(response.contains("const resetTitle = `${label} ${remaining}, ${reset.aria}`;"));
	assert!(
		response.contains("<span class=\"account-window-reset\">${escapeHtml(reset.short)}</span>")
	);
	assert!(response.contains(
		"${reset.date ? `<span class=\"account-window-date\">${escapeHtml(reset.date)}</span>` : \"\"}"
	));
	assert!(!response.contains("<strong>${escapeHtml(reset.main)}</strong>"));
	assert!(!response.contains("<span>${escapeHtml(reset.detail)}</span>"));
	assert!(response.contains("class=\"account-status\""));
	assert!(response.contains("function codexAccountWindowTone(percent)"));
	assert!(response.contains(".account-window.is-warn > strong"));
	assert!(response.contains(".account-window.is-danger > strong"));
	assert!(!response.contains("function codexAccountLowestRemaining(account)"));
	assert!(!response.contains("lowestRemaining <= 20"));
	assert!(!response.contains("account-meter"));
	assert!(!response.contains("lowestRemaining}%"));
}

fn assert_account_pool_render_copy(response: &str) {
	assert!(response.contains(
		"renderStableList(nodes.accountPool, renderCodexAccountPool(accounts, snapshot));"
	));
	assert!(response.contains("syncAccountSelectionConfirmationDom();"));
	assert!(
		!response
			.contains("nodes.accountPool.innerHTML = renderCodexAccountPool(accounts, snapshot)")
	);
	assert!(response.contains("renderAccountPrivacyToggle();"));
	assert!(!response.contains("setPanelMeta(nodes.accountPoolMeta"));
	assert!(!response.contains("nodes.accountPoolMeta.textContent = snapshot"));
	assert!(!response.contains("nodes.accountPoolMeta"));
	assert!(!response.contains("account-row-windows"));
	assert!(!response.contains("account-mini-window"));
	assert!(!response.contains("account-mini-label"));
	assert!(!response.contains("grid-template-columns: repeat(auto-fit, minmax(240px, 1fr));"));
	assert!(
		!response.contains("grid-template-columns: minmax(170px, 1fr) minmax(360px, 1.7fr) 118px;")
	);
	assert!(!response.contains("border-right: 1px solid var(--line);"));
	assert!(!response.contains("box-shadow: inset 3px 0 0 var(--success)"));
	assert!(!response.contains(">Emails</span>"));
	assert!(!response.contains("[\"checked\""));
}
