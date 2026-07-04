import fs from "node:fs/promises";
import path from "node:path";

import { accountUsageStatus } from "./accounts.mjs";
import { nowUnix } from "./time.mjs";

export function parseJwtPayload(token) {
	if (!token || typeof token !== "string") {
		return {};
	}
	const payload = token.split(".")[1];
	if (!payload) {
		return {};
	}

	try {
		return JSON.parse(Buffer.from(payload, "base64url").toString("utf8"));
	} catch (_error) {
		return {};
	}
}

export function accountFingerprint(accountId) {
	const value = String(accountId || "");

	return value ? `...${value.slice(-6)}` : "unknown";
}

export function scoreAccount(account) {
	const primary = account.primary_remaining_percent ?? 0;
	const secondary = account.secondary_remaining_percent ?? primary;
	let score = primary * 1_000 + secondary * 10;

	if (account.rate_limit_reached_type) {
		score -= 50_000;
	}
	if (account.credits_has_credits === false && account.credits_unlimited !== true) {
		score -= 100_000;
	}

	return score;
}

export async function codexAuthAccounts(authDir) {
	const entries = await fs.readdir(authDir, { withFileTypes: true });
	const files = entries
		.filter((entry) => entry.isFile() && /^auth.*\.json$/u.test(entry.name))
		.map((entry) => entry.name)
		.sort((left, right) => {
			if (left === "auth.json") {
				return -1;
			}
			if (right === "auth.json") {
				return 1;
			}

			return left.localeCompare(right);
		});

	if (!files.length) {
		throw new Error(`No auth*.json files found in ${authDir}`);
	}

	const accounts = [];

	for (const file of files) {
		const authPath = path.join(authDir, file);
		const raw = JSON.parse(await fs.readFile(authPath, "utf8"));
		const tokens = raw.tokens || {};
		const claims = parseJwtPayload(tokens.id_token);
		const base = {
			account_email: raw.email || tokens.email || claims.email || null,
			account_fingerprint: accountFingerprint(tokens.account_id),
			plan_type: null,
			status: "available",
			refresh_status: "not_needed",
			checked_at_unix_epoch: nowUnix(),
			selected_at_unix_epoch: null,
			primary_window_seconds: 18_000,
			primary_remaining_percent: null,
			primary_resets_at_unix_epoch: null,
			secondary_window_seconds: 604_800,
			secondary_remaining_percent: null,
			secondary_resets_at_unix_epoch: null,
			credits_has_credits: null,
			credits_unlimited: null,
			credits_balance: null,
			rate_limit_reached_type: null,
			cooldown_until_unix_epoch: null,
			note: "real auth loaded; usage not queried",
		};

		accounts.push(base);
	}

	const selected = accounts.reduce((best, current) =>
		scoreAccount(current) > scoreAccount(best) ? current : best,
	);
	for (const account of accounts) {
		account.status = account === selected ? "selected" : accountUsageStatus(account);
		account.selected_at_unix_epoch = account === selected ? nowUnix() : null;
	}

	return accounts;
}

