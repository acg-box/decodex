import { nowUnix } from "./time.mjs";

export function accountSelector(account) {
	return account.account_email || account.account_fingerprint || "";
}

export function accountMatchesSelector(account, selector) {
	const value = String(selector || "").trim();

	return Boolean(value && [account.account_email, account.account_fingerprint].includes(value));
}

export function selectedAccountForControl(accounts, fixedAccountSelector) {
	if (fixedAccountSelector) {
		const fixedAccount = accounts.find((item) => accountMatchesSelector(item, fixedAccountSelector));
		if (fixedAccount) {
			return fixedAccount;
		}
	}

	return accounts.find((item) => item.status === "selected") || accounts[0] || null;
}

export function accountsWithSelection(accounts, fixedAccountSelector) {
	const selectedAccount = selectedAccountForControl(accounts, fixedAccountSelector);

	return accounts.map((item) => {
		const selected = selectedAccount && accountSelector(item) === accountSelector(selectedAccount);

		return {
			...item,
			status: selected ? "selected" : accountUsageStatus(item),
			selected_at_unix_epoch: selected ? nowUnix() - 20 : null,
		};
	});
}

export function usageEstimate(accounts) {
	const measuredAccounts = accounts.filter((item) => Number.isFinite(Number(item.seven_day_used_percent)));
	if (!accounts.length || !measuredAccounts.length) {
		return null;
	}

	const totalUsedPercent = measuredAccounts.reduce(
		(total, account) => total + Number(account.seven_day_used_percent || 0),
		0,
	);
	const totalCapacityPercent = accounts.length * 100;
	const totalUsedOfCapacityPercent = (totalUsedPercent / totalCapacityPercent) * 100;

	return {
		window_days: 7,
		account_count: accounts.length,
		account_estimate_count: measuredAccounts.length,
		total_capacity_percent: totalCapacityPercent,
		total_used_percent: totalUsedPercent,
		total_used_of_capacity_percent: totalUsedOfCapacityPercent,
		average_daily_used_percent: totalUsedPercent / 7,
		average_daily_pool_percent: totalUsedOfCapacityPercent / 7,
	};
}

export function accountUsageStatus(account) {
	if (
		account.rate_limit_reached_type ||
		account.primary_remaining_percent === 0 ||
		account.secondary_remaining_percent === 0 ||
		(account.credits_has_credits === false && account.credits_unlimited !== true)
	) {
		return "usage_limited";
	}
	if (account.status === "usage_probe_failed") {
		return "usage_probe_failed";
	}

	return "available";
}

