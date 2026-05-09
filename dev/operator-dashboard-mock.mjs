#!/usr/bin/env node

import http from "node:http";
import fs from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const DEFAULT_LISTEN_ADDRESS = "127.0.0.1:57399";
const DEFAULT_READY_STALE_SECONDS = 120;
const USAGE_ENDPOINT = "https://chatgpt.com/backend-api/wham/usage";
const CODEX_USER_AGENT = "codex-cli";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

function parseArgs(argv) {
	const options = {
		authDir: null,
		dashboardHtml: path.join(repoRoot, "src/orchestrator/operator_dashboard.html"),
		listenAddress: DEFAULT_LISTEN_ADDRESS,
		queryUsage: false,
		readyStaleSeconds: DEFAULT_READY_STALE_SECONDS,
	};

	for (let index = 0; index < argv.length; index += 1) {
		const arg = argv[index];
		if (arg === "--help" || arg === "-h") {
			printHelp();
			process.exit(0);
		}
		if (arg === "--listen-address") {
			options.listenAddress = requiredValue(argv, (index += 1), arg);
			continue;
		}
		if (arg === "--dashboard-html") {
			options.dashboardHtml = path.resolve(requiredValue(argv, (index += 1), arg));
			continue;
		}
		if (arg === "--codex-auth-dir") {
			options.authDir = path.resolve(requiredValue(argv, (index += 1), arg));
			continue;
		}
		if (arg === "--use-codex-auth") {
			options.authDir = path.join(process.env.HOME || ".", ".codex");
			continue;
		}
		if (arg === "--query-usage") {
			options.queryUsage = true;
			continue;
		}
		if (arg === "--ready-stale-seconds") {
			options.readyStaleSeconds = Number(requiredValue(argv, (index += 1), arg));
			if (!Number.isFinite(options.readyStaleSeconds) || options.readyStaleSeconds <= 0) {
				throw new Error("--ready-stale-seconds must be a positive number");
			}
			continue;
		}

		throw new Error(`Unknown argument: ${arg}`);
	}

	if (options.queryUsage && !options.authDir) {
		throw new Error("--query-usage requires --use-codex-auth or --codex-auth-dir");
	}

	return options;
}

function requiredValue(argv, index, flag) {
	const value = argv[index];
	if (!value || value.startsWith("--")) {
		throw new Error(`${flag} requires a value`);
	}

	return value;
}

function printHelp() {
	console.log(`Usage: node dev/operator-dashboard-mock.mjs [options]

Serves the real operator dashboard HTML with a comprehensive mock /state payload.

Options:
  --listen-address HOST:PORT   Bind address (default ${DEFAULT_LISTEN_ADDRESS})
  --dashboard-html PATH        Dashboard HTML path
  --use-codex-auth             Load auth*.json accounts from ~/.codex
  --codex-auth-dir DIR         Load auth*.json accounts from DIR
  --query-usage                Query ChatGPT usage for loaded auth accounts
  --ready-stale-seconds N      /readyz freshness window (default ${DEFAULT_READY_STALE_SECONDS})
  -h, --help                   Show this help
`);
}

function splitListenAddress(value) {
	const [host, portText] = value.split(":");
	const port = Number(portText);
	if (!host || !Number.isInteger(port) || port <= 0 || port > 65_535) {
		throw new Error(`Invalid listen address: ${value}`);
	}

	return { host, port };
}

function nowUnix() {
	return Math.floor(Date.now() / 1000);
}

function unixToIso(seconds) {
	return new Date(seconds * 1000).toISOString();
}

function ago(seconds) {
	return unixToIso(nowUnix() - seconds);
}

function later(seconds) {
	return unixToIso(nowUnix() + seconds);
}

function unixLater(seconds) {
	return nowUnix() + seconds;
}

function account({
	email,
	fingerprint,
	plan = "pro",
	status = "available",
	primary = 72,
	primaryReset = 14_400,
	secondary = 91,
	secondaryReset = 518_400,
	creditsBalance = "9.99",
	creditsHasCredits = true,
	creditsUnlimited = false,
	note = "usage probe ok",
	selected = false,
}) {
	return {
		account_email: email,
		account_fingerprint: fingerprint,
		plan_type: plan,
		status: selected ? "selected" : status,
		refresh_status: "not_needed",
		checked_at_unix_epoch: nowUnix() - 30,
		selected_at_unix_epoch: selected ? nowUnix() - 20 : null,
		primary_window_seconds: 18_000,
		primary_remaining_percent: primary,
		primary_resets_at_unix_epoch: unixLater(primaryReset),
		secondary_window_seconds: 604_800,
		secondary_remaining_percent: secondary,
		secondary_resets_at_unix_epoch: unixLater(secondaryReset),
		credits_has_credits: creditsHasCredits,
		credits_unlimited: creditsUnlimited,
		credits_balance: creditsBalance,
		rate_limit_reached_type: null,
		cooldown_until_unix_epoch: null,
		note,
	};
}

function mockAccounts() {
	return [
		account({
			email: "primary@example.test",
			fingerprint: "...acct01",
			primary: 96,
			secondary: 92,
			selected: true,
		}),
		account({
			email: "weekly-depleted@example.test",
			fingerprint: "...acct02",
			status: "usage_limited",
			primary: 100,
			secondary: 0,
			creditsBalance: "0",
			creditsHasCredits: false,
		}),
		account({
			email: "nightly@example.test",
			fingerprint: "...acct03",
			primary: 44,
			secondary: 78,
			creditsBalance: "4.20",
		}),
	];
}

function childAgentActivity() {
	return {
		buckets: [
			{
				name: "Model",
				wall_seconds: 693,
				event_count: 12,
				tool_call_count: 0,
				input_tokens: 4_270_000,
				output_tokens: 12_000,
				output_bytes: 0,
			},
			{
				name: "Shell",
				wall_seconds: 96,
				event_count: 10,
				tool_call_count: 6,
				input_tokens: 0,
				output_tokens: 0,
				output_bytes: 24_000,
			},
			{
				name: "Browser/Image",
				wall_seconds: 41,
				event_count: 6,
				tool_call_count: 3,
				input_tokens: 0,
				output_tokens: 0,
				output_bytes: 180_000,
			},
			{
				name: "Tracker",
				wall_seconds: 0,
				event_count: 2,
				tool_call_count: 2,
				input_tokens: 0,
				output_tokens: 0,
				output_bytes: 2_100,
			},
		],
		current_bucket: "Model",
		current_detail: "waiting after tool output",
		current_started_unix_epoch: null,
		current_elapsed_seconds: 652,
		wall_seconds: 830,
		event_count: 30,
		tool_call_count: 11,
		input_tokens_current: 105_000,
		input_tokens_max: 128_000,
		input_tokens_cumulative: 4_270_000,
		output_tokens_cumulative: 12_000,
		largest_tool_output_bytes: 180_000,
		largest_tool_output_tool: "view_image",
		large_output_warnings: ["view_image repeated 3 large outputs; largest 180000 bytes"],
	};
}

function activeRun({
	accounts,
	attempt = 1,
	issue = "XY-445",
	operation = "agent_run",
	status = "running",
	title = "Account pool dashboard polish",
	processAlive = true,
	activeLease = true,
	childActivity = childAgentActivity(),
}) {
	const selectedAccount = accounts.find((item) => item.status === "selected") || accounts[0] || null;

	return {
		project_id: "decodex-preview",
		run_id: `${issue.toLowerCase()}-attempt-${attempt}-mock`,
		issue_id: issue,
		issue_identifier: issue,
		title,
		attempt_number: attempt,
		status,
		attempt_status: status,
		phase: status === "stalled" ? "reconciling" : "executing",
		wait_reason: null,
		current_operation: operation,
		thread_id: `thread-${issue.toLowerCase()}`,
		turn_id: `turn-${attempt}`,
		thread_status: processAlive ? "active" : "systemError",
		thread_active_flags: processAlive ? [] : ["waitingOnApproval"],
		interactive_requested: !processAlive,
		continuation_pending: false,
		active_lease: activeLease,
		queue_lease_state: activeLease ? "held" : "not_held",
		execution_liveness: processAlive ? "process_alive" : "protocol_observed",
		updated_at: ago(12),
		last_run_activity_at: ago(processAlive ? 8 : 190),
		last_protocol_activity_at: ago(processAlive ? 16 : 185),
		last_progress_at: ago(processAlive ? 18 : 240),
		idle_for_seconds: processAlive ? 8 : 190,
		protocol_idle_for_seconds: processAlive ? 16 : 185,
		suspected_stall: !processAlive,
		last_event_type: "turn/completed",
		last_event_at: ago(processAlive ? 16 : 185),
		event_count: childActivity?.event_count || 4,
		process_id: processAlive ? process.pid : 44_444,
		process_alive: processAlive,
		retry_kind: processAlive ? null : "failure_retry",
		next_retry_at: processAlive ? null : later(600),
		effective_model: "gpt-5.4",
		effective_model_provider: "openai",
		effective_cwd: `/Users/x/code/y/hack-ink/decodex/.worktrees/${issue}`,
		effective_approval_policy: "never",
		effective_approvals_reviewer: null,
		effective_sandbox_mode: "danger-full-access",
		protocol_event: `turn/completed @ ${ago(processAlive ? 16 : 185)}`,
		codex_account: selectedAccount,
		codex_accounts: accounts,
		child_agent_activity: childActivity,
		branch_name: `xy/${issue.toLowerCase()}-mock`,
		worktree_path: `/Users/x/code/y/hack-ink/decodex/.worktrees/${issue}`,
	};
}

function queuedCandidates() {
	return [
		{
			issue_id: "issue-xy-445",
			issue_identifier: "XY-445",
			title: "Running lane still owns this queue claim",
			state: "In Progress",
			priority: 1,
			created_at: ago(2_400),
			classification: "claimed",
			reason: "shared_claim_present",
			attention: null,
			blocker_identifiers: [],
		},
		{
			issue_id: "issue-xy-450",
			issue_identifier: "XY-450",
			title: "Ready implementation lane",
			state: "Todo",
			priority: 2,
			created_at: ago(4_800),
			classification: "ready",
			reason: "normal_dispatch",
			attention: null,
			blocker_identifiers: [],
		},
		{
			issue_id: "issue-xy-451",
			issue_identifier: "XY-451",
			title: "Capacity waits without becoming blocked",
			state: "Todo",
			priority: 3,
			created_at: ago(6_200),
			classification: "waiting",
			reason: "global_concurrency_exhausted",
			attention: null,
			blocker_identifiers: [],
		},
		{
			issue_id: "issue-xy-452",
			issue_identifier: "XY-452",
			title: "Needs attention from previous stalled run",
			state: "In Progress",
			priority: 1,
			created_at: ago(8_600),
			classification: "blocked",
			reason: "issue_needs_attention",
			attention: {
				summary: "App-server thread ended with systemError.",
				run_id: "xy-452-attempt-2-mock",
				attempt_number: 2,
				current_operation: "agent_run",
				thread_status: "systemError",
				retry_budget_attempt_count: 2,
				last_activity_at: ago(900),
				last_progress_at: ago(1_200),
				last_event_type: "thread/error",
				event_count: 23,
				process_alive: false,
				worktree_path: "/Users/x/code/y/hack-ink/decodex/.worktrees/XY-452",
				worktree_has_tracked_changes: true,
			},
			blocker_identifiers: ["XY-399"],
		},
	];
}

function postReviewLanes() {
	return [
		{
			issue_id: "issue-xy-460",
			issue_identifier: "XY-460",
			issue_state: "In Review",
			branch_name: "xy/xy-460-ready",
			worktree_path: "/Users/x/code/y/hack-ink/decodex/.worktrees/XY-460",
			classification: "ready_to_land",
			reason: "Approvals and required checks are complete.",
			pr_url: "https://github.com/hack-ink/decodex/pull/460",
			pr_state: "OPEN",
			review_decision: "APPROVED",
			mergeable: "MERGEABLE",
			check_state: "SUCCESS",
			unresolved_review_threads: 0,
		},
		{
			issue_id: "issue-xy-461",
			issue_identifier: "XY-461",
			issue_state: "In Review",
			branch_name: "xy/xy-461-review-wait",
			worktree_path: "/Users/x/code/y/hack-ink/decodex/.worktrees/XY-461",
			classification: "wait_for_review",
			reason: "External review request is pending.",
			pr_url: "https://github.com/hack-ink/decodex/pull/461",
			pr_state: "OPEN",
			review_decision: "REVIEW_REQUIRED",
			mergeable: "UNKNOWN",
			check_state: "PENDING",
			unresolved_review_threads: 1,
		},
		{
			issue_id: "issue-xy-462",
			issue_identifier: "XY-462",
			issue_state: "Done",
			branch_name: "xy/xy-462-closeout",
			worktree_path: "/Users/x/code/y/hack-ink/decodex/.worktrees/XY-462",
			classification: "closeout_blocked",
			reason: "Merged PR is visible but tracker closeout needs operator attention.",
			pr_url: "https://github.com/hack-ink/decodex/pull/462",
			pr_state: "MERGED",
			review_decision: "APPROVED",
			mergeable: "MERGEABLE",
			check_state: "SUCCESS",
			unresolved_review_threads: 0,
		},
	];
}

function worktrees() {
	return [
		{
			issue_id: "issue-xy-445",
			issue_identifier: "XY-445",
			issue_state: "In Progress",
			branch_name: "xy/xy-445-mock",
			worktree_path: "/Users/x/code/y/hack-ink/decodex/.worktrees/XY-445",
			ownership: "active_lane",
			ownership_reason: "Currently leased by a running lane.",
		},
		{
			issue_id: "issue-xy-460",
			issue_identifier: "XY-460",
			issue_state: "In Review",
			branch_name: "xy/xy-460-ready",
			worktree_path: "/Users/x/code/y/hack-ink/decodex/.worktrees/XY-460",
			ownership: "post_review_lane",
			ownership_reason: "Retained for review, landing, or closeout follow-up.",
		},
		{
			issue_id: "issue-xy-499",
			issue_identifier: "XY-499",
			issue_state: "Canceled",
			branch_name: "xy/xy-499-orphan",
			worktree_path: "/Users/x/code/y/hack-ink/decodex/.worktrees/XY-499",
			ownership: "local_cleanup",
			ownership_reason:
				"No active lane, queued recovery, or post-review lane owns this worktree; local cleanup only.",
		},
	];
}

function historyLane(accounts) {
	const run = activeRun({
		accounts,
		attempt: 1,
		issue: "XY-430",
		operation: "completed",
		status: "succeeded",
		title: "Completed dashboard lane",
		processAlive: false,
		activeLease: false,
		childActivity: null,
	});
	run.updated_at = ago(7_200);
	run.last_run_activity_at = ago(7_200);
	run.last_protocol_activity_at = ago(7_260);
	run.last_progress_at = ago(7_260);
	run.process_alive = false;
	run.thread_status = "completed";

	return {
		project_id: "decodex-preview",
		issue_id: "issue-xy-430",
		issue_identifier: "XY-430",
		title: "Completed dashboard lane",
		issue_key: "XY-430",
		attempt_count: 2,
		ledger_outcome: {
			ledger_status: "present",
			final_outcome: "succeeded",
			final_event_type: "issue_closeout_complete",
			final_event_at: ago(7_200),
			summary: "Merged, closed out, and cleaned up.",
			pr_url: "https://github.com/hack-ink/decodex/pull/430",
			commit_sha: "abc123def456",
			branch: "xy/xy-430-dashboard",
			closeout_status: "completed",
			needs_attention_reason: null,
			lifecycle_started_at: ago(12_000),
			lifecycle_finished_at: ago(7_200),
			lifecycle_elapsed_seconds: 4_800,
			record_count: 8,
		},
		latest_run: run,
		attempts: [
			{ ...run, run_id: "xy-430-attempt-1-mock", attempt_number: 1, status: "failed" },
			{ ...run, run_id: "xy-430-attempt-2-mock", attempt_number: 2, status: "succeeded" },
		],
	};
}

function buildSnapshot(accounts) {
	const activeRuns = [
		activeRun({ accounts }),
		activeRun({
			accounts,
			attempt: 2,
			issue: "XY-452",
			title: "Stalled lane requiring attention",
			processAlive: false,
			activeLease: false,
		}),
	];
	const reviewLanes = postReviewLanes();
	const retainedWorktrees = worktrees().filter((item) => item.ownership !== "active_lane");

	return {
		project_id: "decodex-preview",
		run_limit: 25,
		warnings: ["external_observer_status_skipped"],
		connector_backoffs: [
			{
				project_id: "decodex-preview",
				connector: "linear",
				sync_phase: "queued_candidates",
				quota_class: "rate_limit",
				reset_at: later(600),
				reset_unix_epoch: unixLater(600),
				reset_source: "retry_after",
				retry_after_seconds: 600,
				next_action: "serve cached local runtime state until reset",
				warning: "tracker_rate_limited",
			},
		],
		projects: [
			{
				project_id: "decodex-preview",
				config_path: "~/.codex/decodex/projects/decodex/project.toml",
				repo_root: "/Users/x/code/y/hack-ink/decodex",
				enabled: true,
				active_run_count: activeRuns.length,
				queued_candidate_count: queuedCandidates().length,
				post_review_lane_count: reviewLanes.length,
				retained_worktree_count: retainedWorktrees.length,
				waiting_lane_count: 2,
				attention_count: 2,
				connector_state: "degraded",
				last_activity_at: ago(8),
				warning_count: 1,
			},
		],
		active_runs: activeRuns,
		queued_candidates: queuedCandidates(),
		recent_runs: [],
		history_lanes: [historyLane(accounts)],
		worktrees: worktrees(),
		post_review_lanes: reviewLanes,
	};
}

function parseJwtPayload(token) {
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

function accountFingerprint(accountId) {
	const value = String(accountId || "");

	return value ? `...${value.slice(-6)}` : "unknown";
}

function numeric(value) {
	if (value == null) {
		return null;
	}
	const number = Number(value);

	return Number.isFinite(number) ? Math.round(number) : null;
}

function scalarString(value) {
	if (value == null) {
		return null;
	}
	if (typeof value === "string") {
		return value || null;
	}
	if (typeof value === "number" || typeof value === "boolean") {
		return String(value);
	}

	return null;
}

function usageWindow(rateLimit, key) {
	const window = rateLimit?.[key];
	if (!window || typeof window !== "object") {
		return {};
	}
	const usedPercent = numeric(window.used_percent);

	return {
		windowSeconds: numeric(window.limit_window_seconds),
		remainingPercent:
			usedPercent == null ? null : Math.max(0, Math.min(100, 100 - usedPercent)),
		resetsAt: numeric(window.reset_at),
	};
}

function reachedType(payload) {
	const reached = payload?.rate_limit_reached_type;
	if (!reached) {
		return null;
	}
	if (typeof reached === "object") {
		return scalarString(reached.kind) || scalarString(reached.type);
	}

	return scalarString(reached);
}

function scoreAccount(account) {
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

async function codexAuthAccounts(authDir, queryUsage) {
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
			note: queryUsage ? "usage probe pending" : "real auth loaded; usage not queried",
		};

		accounts.push(
			queryUsage && tokens.access_token && tokens.account_id
				? await accountWithUsage(base, tokens.access_token, tokens.account_id)
				: base,
		);
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

async function accountWithUsage(base, accessToken, accountId) {
	const response = await fetch(USAGE_ENDPOINT, {
		headers: {
			Authorization: `Bearer ${accessToken}`,
			"ChatGPT-Account-Id": accountId,
			"User-Agent": CODEX_USER_AGENT,
		},
		signal: AbortSignal.timeout(10_000),
	});

	if (!response.ok) {
		return {
			...base,
			status: "usage_probe_failed",
			note: `usage endpoint HTTP ${response.status}`,
		};
	}

	const payload = await response.json();
	const primary = usageWindow(payload.rate_limit, "primary_window");
	const secondary = usageWindow(payload.rate_limit, "secondary_window");
	const credits = payload.credits || {};

	return {
		...base,
		plan_type: scalarString(payload.plan_type),
		checked_at_unix_epoch: nowUnix(),
		primary_window_seconds: primary.windowSeconds,
		primary_remaining_percent: primary.remainingPercent,
		primary_resets_at_unix_epoch: primary.resetsAt,
		secondary_window_seconds: secondary.windowSeconds,
		secondary_remaining_percent: secondary.remainingPercent,
		secondary_resets_at_unix_epoch: secondary.resetsAt,
		credits_has_credits:
			typeof credits.has_credits === "boolean" ? credits.has_credits : null,
		credits_unlimited: typeof credits.unlimited === "boolean" ? credits.unlimited : null,
		credits_balance: scalarString(credits.balance),
		rate_limit_reached_type: reachedType(payload),
		note: "usage probe ok",
	};
}

function accountUsageStatus(account) {
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

function send(response, statusCode, contentType, body, headers = {}) {
	response.writeHead(statusCode, {
		"content-type": contentType,
		"content-length": Buffer.byteLength(body),
		"cache-control": "no-store",
		...headers,
	});
	response.end(body);
}

async function main() {
	const options = parseArgs(process.argv.slice(2));
	const staticAccounts =
		options.authDir && options.queryUsage
			? null
			: options.authDir
				? await codexAuthAccounts(options.authDir, options.queryUsage)
				: mockAccounts();
	let lastPublishedAt = nowUnix();
	const { host, port } = splitListenAddress(options.listenAddress);
	const server = http.createServer(async (request, response) => {
		try {
			if (request.method !== "GET") {
				send(response, 405, "text/plain; charset=utf-8", "method not allowed");
				return;
			}
			const url = new URL(request.url || "/", `http://${options.listenAddress}`);
			if (url.pathname === "/" || url.pathname === "/dashboard") {
				const html = await fs.readFile(options.dashboardHtml, "utf8");
				send(response, 200, "text/html; charset=utf-8", html);
				return;
			}
			if (url.pathname === "/livez") {
				send(response, 200, "text/plain; charset=utf-8", "ok");
				return;
			}
			if (url.pathname === "/readyz") {
				const stale = nowUnix() - lastPublishedAt > options.readyStaleSeconds;
				send(
					response,
					stale ? 503 : 200,
					"text/plain; charset=utf-8",
					stale ? "snapshot_stale" : "ready",
				);
				return;
			}
			if (url.pathname === "/state") {
				lastPublishedAt = nowUnix();
				const accounts =
					options.authDir && options.queryUsage
						? await codexAuthAccounts(options.authDir, true)
						: staticAccounts;
				const snapshot = buildSnapshot(accounts);
				send(response, 200, "application/json", JSON.stringify(snapshot), {
					"X-Decodex-Snapshot-Unix-Epoch": String(lastPublishedAt),
				});
				return;
			}

			send(response, 404, "text/plain; charset=utf-8", "not found");
		} catch (error) {
			send(response, 500, "text/plain; charset=utf-8", error?.message || "mock server error");
		}
	});

	server.listen(port, host, () => {
		console.log(`operator dashboard mock: http://${host}:${port}/dashboard`);
		console.log(
			options.authDir
				? `accounts: ${options.authDir}${options.queryUsage ? " with live usage per /state" : ` (${staticAccounts.length} loaded)`}`
				: `accounts: ${staticAccounts.length} synthetic fixture accounts`,
		);
	});
}

main().catch((error) => {
	console.error(error?.message || error);
	process.exit(1);
});
