#!/usr/bin/env node

import http from "node:http";
import crypto from "node:crypto";
import fs from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const DEFAULT_LISTEN_ADDRESS = "127.0.0.1:57399";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

function parseArgs(argv) {
	const options = {
			authDir: null,
			dashboardHtml: path.join(repoRoot, "apps/decodex/src/orchestrator/operator_dashboard.html"),
			listenAddress: DEFAULT_LISTEN_ADDRESS,
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
		throw new Error(`Unknown argument: ${arg}`);
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

Serves the real operator dashboard HTML, /api/accounts, and mock dashboard WebSocket
snapshot/activity events from one local base URL. Use the same mock base URL for the
browser dashboard and Decodex App previews; do not start a second mock server for the
App. The dashboard authority is ws://HOST:PORT/dashboard/control.

Options:
  --listen-address HOST:PORT   Bind address (default ${DEFAULT_LISTEN_ADDRESS})
  --dashboard-html PATH        Dashboard HTML path
  --use-codex-auth             Load auth*.json accounts from ~/.codex
  --codex-auth-dir DIR         Load auth*.json accounts from DIR
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

function usageDate(daysAgo) {
	const date = new Date(Date.now() - daysAgo * 86_400_000);

	return date.toISOString().slice(0, 10);
}

function profileDailyUsage(values = []) {
	const days = values.length;
	return values.map((tokens, index) => ({
		date: usageDate(days - index - 1),
		tokens,
	}));
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
	sevenDayUsed = 18,
	previousSevenDayUsed = Math.max(0, sevenDayUsed - 4),
	profileDisplayName = null,
	profileUsername = null,
	profileLifetimeTokens = 47_200_000_000,
	profilePeakDailyTokens = 1_500_000_000,
	profileLongestTaskSeconds = 10_080,
	profileCurrentStreakDays = 12,
	profileLongestStreakDays = 68,
	profileUsage = [
		0, 420_000, 880_000, 1_120_000, 0, 1_760_000, 2_220_000, 2_000_000,
		3_140_000, 1_540_000, 0, 920_000, 2_760_000, 3_800_000,
	],
}) {
	return {
		account_email: email,
		email,
		account_fingerprint: fingerprint,
		selector: email || fingerprint,
		plan_type: plan,
		status: selected ? "selected" : status,
		selected,
		codex_active: false,
		disabled: false,
		refresh_token_present: true,
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
		seven_day_used_percent: sevenDayUsed,
		seven_day_daily_average_percent: sevenDayUsed / 7,
		profile_display_name: profileDisplayName,
		profile_username: profileUsername,
		profile_checked_at_unix_epoch: nowUnix() - 30,
		profile_lifetime_tokens: profileLifetimeTokens,
		profile_peak_daily_tokens: profilePeakDailyTokens,
		profile_longest_task_seconds: profileLongestTaskSeconds,
		profile_current_streak_days: profileCurrentStreakDays,
		profile_longest_streak_days: profileLongestStreakDays,
		profile_daily_usage: profileDailyUsage(profileUsage),
		usage_records: [
			{
				date: usageDate(1),
				used_percent: previousSevenDayUsed,
				checked_at_unix_epoch: nowUnix() - 86_400,
			},
			{
				date: usageDate(0),
				used_percent: sevenDayUsed,
				checked_at_unix_epoch: nowUnix() - 30,
			},
		],
	};
}

function mockAccounts() {
	return [
		account({
			email: "mock-primary@decodex.test",
			fingerprint: "...acct01",
			primary: 96,
			secondary: 92,
			selected: true,
			sevenDayUsed: 22,
			previousSevenDayUsed: 17,
			profileDisplayName: "Primary mock",
			profileUsername: "mock-primary",
		}),
		account({
			email: "mock-weekly-limited@decodex.test",
			fingerprint: "...acct02",
			status: "usage_limited",
			primary: 100,
			secondary: 0,
			creditsBalance: "0",
			creditsHasCredits: false,
			sevenDayUsed: 100,
			previousSevenDayUsed: 92,
			profileLifetimeTokens: 8_900_000_000,
			profilePeakDailyTokens: 640_000_000,
			profileLongestTaskSeconds: 5_820,
			profileCurrentStreakDays: 0,
			profileLongestStreakDays: 21,
			profileUsage: [0, 0, 180_000, 440_000, 0, 0, 900_000, 120_000, 0, 0, 0, 240_000, 0, 0],
		}),
		account({
			email: "mock-nightly@decodex.test",
			fingerprint: "...acct03",
			primary: 44,
			secondary: 78,
			creditsBalance: "4.20",
			sevenDayUsed: 37,
			previousSevenDayUsed: 35,
			profileLifetimeTokens: 19_400_000_000,
			profilePeakDailyTokens: 980_000_000,
			profileLongestTaskSeconds: 7_140,
			profileCurrentStreakDays: 5,
			profileLongestStreakDays: 44,
			profileUsage: [
				220_000, 700_000, 1_300_000, 0, 1_600_000, 1_920_000, 2_100_000,
				2_400_000, 1_800_000, 2_250_000, 1_760_000, 0, 2_900_000, 3_100_000,
			],
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

function lifecycleMetrics({
	attemptCount,
	capturedAttemptCount = attemptCount,
	protocolEventCount,
	childEventCount,
	wallSeconds,
	toolCallCount,
	inputTokens,
	outputTokens,
	buckets = [],
}) {
	return {
		attempt_count: attemptCount,
		run_count: attemptCount,
		captured_attempt_count: capturedAttemptCount,
		missing_attempt_count: Math.max(0, attemptCount - capturedAttemptCount),
		protocol_event_count: protocolEventCount,
		child_event_count: childEventCount,
		wall_seconds: wallSeconds,
		tool_call_count: toolCallCount,
		input_tokens_cumulative: inputTokens,
		output_tokens_cumulative: outputTokens,
		largest_tool_output_bytes: 180_000,
		largest_tool_output_tool: "view_image",
		buckets,
	};
}

function lifecyclePhaseMetrics({ phase, label, ...metrics }) {
	return {
		phase,
		label,
		...lifecycleMetrics(metrics),
	};
}

function activeRunLifecycleMetrics(childActivity, { attemptCount = 1, phase = "development", label = "Development" } = {}) {
	if (!childActivity) {
		return lifecycleMetrics({
			attemptCount: 0,
			capturedAttemptCount: 0,
			protocolEventCount: 0,
			childEventCount: 0,
			wallSeconds: 0,
			toolCallCount: 0,
			inputTokens: 0,
			outputTokens: 0,
			buckets: [],
		});
	}
	const phaseMetrics = lifecyclePhaseMetrics({
		phase,
		label,
		attemptCount,
		protocolEventCount: childActivity.event_count,
		childEventCount: childActivity.event_count,
		wallSeconds: childActivity.wall_seconds,
		toolCallCount: childActivity.tool_call_count,
		inputTokens: childActivity.input_tokens_cumulative,
		outputTokens: childActivity.output_tokens_cumulative,
		buckets: childActivity.buckets,
	});
	const total = {
		...phaseMetrics,
		phase: undefined,
		label: undefined,
		phases: [phaseMetrics],
	};

	total.input_tokens_current = childActivity.input_tokens_current;
	total.input_tokens_peak = childActivity.input_tokens_max;
	total.large_output_warnings = childActivity.large_output_warnings || [];
	total.largest_tool_output_bytes = childActivity.largest_tool_output_bytes;
	total.largest_tool_output_tool = childActivity.largest_tool_output_tool;
	phaseMetrics.input_tokens_current = childActivity.input_tokens_current;
	phaseMetrics.input_tokens_peak = childActivity.input_tokens_max;
	phaseMetrics.large_output_warnings = childActivity.large_output_warnings || [];
	phaseMetrics.largest_tool_output_bytes = childActivity.largest_tool_output_bytes;
	phaseMetrics.largest_tool_output_tool = childActivity.largest_tool_output_tool;

	delete total.phase;
	delete total.label;

	return total;
}

function activeReviewLifecycleMetrics(currentActivity) {
	const developmentPhase = lifecyclePhaseMetrics({
		phase: "development",
		label: "Development",
		attemptCount: 1,
		protocolEventCount: 18,
		childEventCount: 24,
		wallSeconds: 910,
		toolCallCount: 7,
		inputTokens: 2_850_000,
		outputTokens: 8_500,
		buckets: [
			{
				name: "Model",
				wall_seconds: 620,
				event_count: 11,
				tool_call_count: 0,
				input_tokens: 2_850_000,
				output_tokens: 8_500,
				output_bytes: 0,
			},
			{
				name: "Shell",
				wall_seconds: 220,
				event_count: 9,
				tool_call_count: 5,
				input_tokens: 0,
				output_tokens: 0,
				output_bytes: 34_000,
			},
			{
				name: "Tracker",
				wall_seconds: 0,
				event_count: 4,
				tool_call_count: 2,
				input_tokens: 0,
				output_tokens: 0,
				output_bytes: 3_200,
			},
		],
	});
	developmentPhase.largest_tool_output_bytes = 34_000;
	developmentPhase.largest_tool_output_tool = "shell";
	developmentPhase.large_output_warnings = [];
	const reviewPhase = activeRunLifecycleMetrics(currentActivity, {
		attemptCount: 1,
		phase: "review",
		label: "Review",
	}).phases[0];
	const phases = [developmentPhase, reviewPhase];
	const bucketTotals = new Map();

	for (const phase of phases) {
		for (const bucket of phase.buckets || []) {
			const total =
				bucketTotals.get(bucket.name) ||
				{
					name: bucket.name,
					wall_seconds: 0,
					event_count: 0,
					tool_call_count: 0,
					input_tokens: 0,
					output_tokens: 0,
					output_bytes: 0,
				};
			total.wall_seconds += bucket.wall_seconds || 0;
			total.event_count += bucket.event_count || 0;
			total.tool_call_count += bucket.tool_call_count || 0;
			total.input_tokens += bucket.input_tokens || 0;
			total.output_tokens += bucket.output_tokens || 0;
			total.output_bytes += bucket.output_bytes || 0;
			bucketTotals.set(bucket.name, total);
		}
	}

	const total = lifecycleMetrics({
		attemptCount: 2,
		protocolEventCount: phases.reduce((count, phase) => count + phase.protocol_event_count, 0),
		childEventCount: phases.reduce((count, phase) => count + phase.child_event_count, 0),
		wallSeconds: phases.reduce((count, phase) => count + phase.wall_seconds, 0),
		toolCallCount: phases.reduce((count, phase) => count + phase.tool_call_count, 0),
		inputTokens: phases.reduce((count, phase) => count + phase.input_tokens_cumulative, 0),
		outputTokens: phases.reduce((count, phase) => count + phase.output_tokens_cumulative, 0),
		buckets: Array.from(bucketTotals.values()).sort((left, right) => right.wall_seconds - left.wall_seconds),
	});

	total.input_tokens_current = currentActivity.input_tokens_current;
	total.input_tokens_peak = Math.max(currentActivity.input_tokens_max, 128_000);
	total.large_output_warnings = currentActivity.large_output_warnings || [];
	total.largest_tool_output_bytes = currentActivity.largest_tool_output_bytes;
	total.largest_tool_output_tool = currentActivity.largest_tool_output_tool;
	total.phases = phases;

	return total;
}

function activeRun({
	accounts,
	accountIndex = 0,
	assignedAccount = null,
	attempt = 1,
	issue = "XY-445",
	operation = "agent_run",
	status = "running",
		title = "Account pool dashboard polish",
		processAlive = true,
		activeLease = true,
		childActivity = childAgentActivity(),
		lifecycleMetrics = null,
	}) {
	const selectedAccount =
		assignedAccount ||
		accounts[accountIndex] ||
		accounts.find((item) => item.status === "selected") ||
		accounts[0] ||
		null;
	const lifecyclePhase =
		status === "review_handoff_pending" || operation === "review_writeback"
			? { phase: "review", label: "Review" }
			: status === "closeout_pending" || operation === "closeout"
				? { phase: "closeout", label: "Closeout" }
				: status === "needs_attention"
					? { phase: "manual_attention", label: "Manual attention" }
					: { phase: "development", label: "Development" };

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
			lifecycle_metrics:
				lifecycleMetrics ||
				activeRunLifecycleMetrics(childActivity, {
					attemptCount: attempt,
					phase: lifecyclePhase.phase,
					label: lifecyclePhase.label,
				}),
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
			title: "Ready follow-up lane",
			state: "Todo",
			priority: 3,
			created_at: ago(6_200),
			classification: "ready",
			reason: "eligible_for_dispatch",
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
			reason: "",
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
			reason: "",
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
			reason: "",
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

	const developmentPhase = lifecyclePhaseMetrics({
		phase: "development",
		label: "Development",
		attemptCount: 2,
		protocolEventCount: 42,
		childEventCount: 78,
		wallSeconds: 2_940,
		toolCallCount: 31,
		inputTokens: 6_800_000,
		outputTokens: 21_000,
		buckets: [
			{
				name: "Model",
				wall_seconds: 2_320,
				event_count: 38,
				tool_call_count: 0,
				input_tokens: 6_800_000,
				output_tokens: 21_000,
				output_bytes: 0,
			},
			{
				name: "Shell",
				wall_seconds: 410,
				event_count: 28,
				tool_call_count: 24,
				input_tokens: 0,
				output_tokens: 0,
				output_bytes: 58_000,
			},
			{
				name: "Tracker",
				wall_seconds: 0,
				event_count: 12,
				tool_call_count: 7,
				input_tokens: 0,
				output_tokens: 0,
				output_bytes: 8_200,
			},
		],
	});
	const reviewPhase = lifecyclePhaseMetrics({
		phase: "review",
		label: "Review",
		attemptCount: 1,
		protocolEventCount: 16,
		childEventCount: 24,
		wallSeconds: 1_080,
		toolCallCount: 9,
		inputTokens: 2_100_000,
		outputTokens: 8_400,
		buckets: [
			{
				name: "Model",
				wall_seconds: 820,
				event_count: 14,
				tool_call_count: 0,
				input_tokens: 2_100_000,
				output_tokens: 8_400,
				output_bytes: 0,
			},
			{
				name: "GitHub",
				wall_seconds: 130,
				event_count: 5,
				tool_call_count: 4,
				input_tokens: 0,
				output_tokens: 0,
				output_bytes: 11_000,
			},
			{
				name: "Shell",
				wall_seconds: 130,
				event_count: 5,
				tool_call_count: 5,
				input_tokens: 0,
				output_tokens: 0,
				output_bytes: 16_000,
			},
		],
	});
	const closeoutPhase = lifecyclePhaseMetrics({
		phase: "closeout",
		label: "Closeout",
		attemptCount: 1,
		protocolEventCount: 8,
		childEventCount: 12,
		wallSeconds: 480,
		toolCallCount: 5,
		inputTokens: 520_000,
		outputTokens: 2_100,
		buckets: [
			{
				name: "Model",
				wall_seconds: 300,
				event_count: 6,
				tool_call_count: 0,
				input_tokens: 520_000,
				output_tokens: 2_100,
				output_bytes: 0,
			},
			{
				name: "Shell",
				wall_seconds: 180,
				event_count: 6,
				tool_call_count: 5,
				input_tokens: 0,
				output_tokens: 0,
				output_bytes: 9_000,
			},
		],
	});
	const phases = [developmentPhase, reviewPhase, closeoutPhase];
	const lifecycle = lifecycleMetrics({
		attemptCount: 4,
		capturedAttemptCount: 4,
		protocolEventCount: phases.reduce((total, phase) => total + phase.protocol_event_count, 0),
		childEventCount: phases.reduce((total, phase) => total + phase.child_event_count, 0),
		wallSeconds: phases.reduce((total, phase) => total + phase.wall_seconds, 0),
		toolCallCount: phases.reduce((total, phase) => total + phase.tool_call_count, 0),
		inputTokens: phases.reduce((total, phase) => total + phase.input_tokens_cumulative, 0),
		outputTokens: phases.reduce((total, phase) => total + phase.output_tokens_cumulative, 0),
		buckets: [
			{
				name: "Model",
				wall_seconds: 3_440,
				event_count: 58,
				tool_call_count: 0,
				input_tokens: 9_420_000,
				output_tokens: 31_500,
				output_bytes: 0,
			},
			{
				name: "Shell",
				wall_seconds: 720,
				event_count: 39,
				tool_call_count: 34,
				input_tokens: 0,
				output_tokens: 0,
				output_bytes: 83_000,
			},
			{
				name: "GitHub",
				wall_seconds: 130,
				event_count: 5,
				tool_call_count: 4,
				input_tokens: 0,
				output_tokens: 0,
				output_bytes: 11_000,
			},
			{
				name: "Tracker",
				wall_seconds: 0,
				event_count: 12,
				tool_call_count: 7,
				input_tokens: 0,
				output_tokens: 0,
				output_bytes: 8_200,
			},
		],
	});
	lifecycle.phases = phases;

	return {
		project_id: "decodex-preview",
		issue_id: "issue-xy-430",
		issue_identifier: "XY-430",
		title: "Completed dashboard lane",
		issue_key: "XY-430",
		attempt_count: 4,
		lifecycle_metrics: lifecycle,
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
			{ ...run, run_id: "xy-430-review-1-mock", attempt_number: 3, status: "succeeded" },
			{ ...run, run_id: "xy-430-closeout-1-mock", attempt_number: 4, status: "succeeded" },
		],
	};
}

function accountSelector(account) {
	return account.account_email || account.account_fingerprint || "";
}

function accountMatchesSelector(account, selector) {
	const value = String(selector || "").trim();

	return Boolean(value && [account.account_email, account.account_fingerprint].includes(value));
}

function selectedAccountForControl(accounts, fixedAccountSelector) {
	if (fixedAccountSelector) {
		const fixedAccount = accounts.find((item) => accountMatchesSelector(item, fixedAccountSelector));
		if (fixedAccount) {
			return fixedAccount;
		}
	}

	return accounts.find((item) => item.status === "selected") || accounts[0] || null;
}

function accountsWithSelection(accounts, fixedAccountSelector) {
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

function usageEstimate(accounts) {
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

function buildSnapshot(accounts, fixedAccountSelector) {
	const controlledAccounts = accountsWithSelection(accounts, fixedAccountSelector);
	const primaryActivity = childAgentActivity();
	const activeRuns = [
		activeRun({
			accounts: controlledAccounts,
			accountIndex: 0,
			attempt: 2,
			childActivity: primaryActivity,
			lifecycleMetrics: activeReviewLifecycleMetrics(primaryActivity),
		}),
		activeRun({
			accounts: controlledAccounts,
			accountIndex: Math.min(2, controlledAccounts.length - 1),
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
		account_control: {
			mode: fixedAccountSelector ? "fixed" : "balanced",
			account_selector: fixedAccountSelector || null,
		},
		accounts: controlledAccounts,
		active_runs: activeRuns,
		queued_candidates: queuedCandidates(),
		recent_runs: [],
		history_lanes: [historyLane(controlledAccounts)],
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

async function codexAuthAccounts(authDir) {
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

function websocketAcceptValue(key) {
	return crypto
		.createHash("sha1")
		.update(`${key}258EAFA5-E914-47DA-95CA-C5AB0DC85B11`)
		.digest("base64");
}

function encodeWebSocketText(payload) {
	const body = Buffer.from(JSON.stringify(payload), "utf8");
	if (body.length <= 125) {
		return Buffer.concat([Buffer.from([0x81, body.length]), body]);
	}
	if (body.length <= 65_535) {
		const header = Buffer.alloc(4);
		header[0] = 0x81;
		header[1] = 126;
		header.writeUInt16BE(body.length, 2);
		return Buffer.concat([header, body]);
	}

	const header = Buffer.alloc(10);
	header[0] = 0x81;
	header[1] = 127;
	header.writeBigUInt64BE(BigInt(body.length), 2);
	return Buffer.concat([header, body]);
}

function sendWebSocketJson(socket, payload) {
	if (socket.destroyed) {
		return;
	}
	socket.write(encodeWebSocketText(payload));
}

function decodeWebSocketFrames(buffer) {
	const messages = [];
	let offset = 0;
	let closed = false;

	while (buffer.length - offset >= 2) {
		const first = buffer[offset];
		const second = buffer[offset + 1];
		const opcode = first & 0x0f;
		const masked = (second & 0x80) === 0x80;
		let length = second & 0x7f;
		let headerLength = 2;

		if (length === 126) {
			if (buffer.length - offset < 4) {
				break;
			}
			length = buffer.readUInt16BE(offset + 2);
			headerLength = 4;
		} else if (length === 127) {
			if (buffer.length - offset < 10) {
				break;
			}
			length = Number(buffer.readBigUInt64BE(offset + 2));
			headerLength = 10;
		}

		const maskLength = masked ? 4 : 0;
		const frameLength = headerLength + maskLength + length;
		if (buffer.length - offset < frameLength) {
			break;
		}

		const mask = masked
			? buffer.subarray(offset + headerLength, offset + headerLength + 4)
			: null;
		const payloadStart = offset + headerLength + maskLength;
		const payload = Buffer.from(buffer.subarray(payloadStart, payloadStart + length));
		if (mask) {
			for (let index = 0; index < payload.length; index += 1) {
				payload[index] ^= mask[index % 4];
			}
		}

		if (opcode === 0x8) {
			closed = true;
			offset += frameLength;
			break;
		}
		if (opcode === 0x1) {
			messages.push(payload.toString("utf8"));
		}
		offset += frameLength;
	}

	return {
		closed,
		messages,
		remaining: buffer.subarray(offset),
	};
}

function dashboardControlAck(message, accepted, status, copy) {
	return {
		type: "controlAck",
		payload: {
			requestId: message.requestId || null,
			action: message.action || message.type || "control",
			accepted,
			status,
			message: copy,
			projectId: message.projectId || null,
			issueId: message.issueId || null,
			runId: message.runId || null,
		},
	};
}

async function main() {
	const options = parseArgs(process.argv.slice(2));
	const staticAccounts = options.authDir
		? await codexAuthAccounts(options.authDir)
		: mockAccounts();
	let fixedAccountSelector =
		staticAccounts.find((item) => item.status === "selected")?.account_email ||
		staticAccounts[0]?.account_email ||
		null;
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
			if (url.pathname === "/api/accounts") {
				const controlledAccounts = accountsWithSelection(staticAccounts, fixedAccountSelector);
				send(
					response,
					200,
					"application/json; charset=utf-8",
					JSON.stringify({
						accounts_path: "/tmp/decodex-mock/accounts.jsonl",
						global_config_path: "/tmp/decodex-mock/config.toml",
						codex_auth_path: "/tmp/decodex-mock/auth.json",
						codex_auth: null,
						control: {
							mode: fixedAccountSelector ? "fixed" : "balanced",
							account_selector: fixedAccountSelector || null,
						},
						accounts: controlledAccounts,
						usage_estimate: usageEstimate(controlledAccounts),
						usage_probe_error: null,
					}),
				);
				return;
			}
				if (url.pathname === "/livez") {
					send(response, 200, "text/plain; charset=utf-8", "ok");
					return;
				}

				send(response, 404, "text/plain; charset=utf-8", "not found");
		} catch (error) {
			send(response, 500, "text/plain; charset=utf-8", error?.message || "mock server error");
		}
	});

	server.on("upgrade", (request, socket) => {
		const url = new URL(request.url || "/", `http://${options.listenAddress}`);
		if (url.pathname !== "/dashboard/control") {
			socket.destroy();
			return;
		}

		const key = request.headers["sec-websocket-key"];
		if (!key) {
			socket.destroy();
			return;
		}

		socket.write(
			[
				"HTTP/1.1 101 Switching Protocols",
				"Upgrade: websocket",
				"Connection: Upgrade",
				`Sec-WebSocket-Accept: ${websocketAcceptValue(key)}`,
				"",
				"",
			].join("\r\n"),
		);
		sendWebSocketJson(socket, {
			type: "controlReady",
			payload: {
				supportedActions: [
					"subscribe",
					"focus",
					"clearFocus",
					"pauseProject",
					"resumeProject",
					"interruptRun",
					"selectAccount",
					"clearAccountSelection",
					"ack",
				],
				subscription: {},
			},
		});
		sendWebSocketJson(socket, {
			type: "snapshot",
			payload: {
				snapshot: buildSnapshot(staticAccounts, fixedAccountSelector),
				snapshotPublishedAtUnixEpoch: lastPublishedAt,
			},
		});

		let buffered = Buffer.alloc(0);
		socket.on("data", (chunk) => {
			buffered = Buffer.concat([buffered, chunk]);
			const decoded = decodeWebSocketFrames(buffered);
			buffered = decoded.remaining;
			if (decoded.closed) {
				socket.end();
				return;
			}

			for (const text of decoded.messages) {
				let message;
				try {
					message = JSON.parse(text);
				} catch (_error) {
					sendWebSocketJson(socket, {
						type: "controlAck",
						payload: {
							requestId: null,
							action: "control",
							accepted: false,
							status: "invalid_json",
							message: "Mock dashboard control received invalid JSON.",
						},
					});
					continue;
				}

				if (message.type === "subscribe") {
					sendWebSocketJson(
						socket,
						dashboardControlAck(message, true, "accepted", "Mock subscription accepted."),
					);
					continue;
				}

				if (message.type !== "control") {
					sendWebSocketJson(
						socket,
						dashboardControlAck(
							message,
							false,
							"unsupported",
							"Mock dashboard control type is unsupported.",
						),
					);
					continue;
				}

				if (message.action === "selectAccount") {
					const selector = String(message.accountSelector || "").trim();
					if (!staticAccounts.some((item) => accountMatchesSelector(item, selector))) {
						sendWebSocketJson(
							socket,
							dashboardControlAck(
								message,
								false,
								"unknown_account",
								"Mock account selector was not found.",
							),
						);
						continue;
					}
					fixedAccountSelector = selector;
					lastPublishedAt = nowUnix();
					sendWebSocketJson(
						socket,
						dashboardControlAck(message, true, "accepted", "Mock account selection updated."),
					);
					sendWebSocketJson(socket, {
						type: "snapshot",
						payload: {
							snapshot: buildSnapshot(staticAccounts, fixedAccountSelector),
							snapshotPublishedAtUnixEpoch: lastPublishedAt,
						},
					});
					continue;
				}

				if (message.action === "clearAccountSelection") {
					fixedAccountSelector = null;
					lastPublishedAt = nowUnix();
					sendWebSocketJson(
						socket,
						dashboardControlAck(
							message,
							true,
							"accepted",
							"Mock account selection returned to balanced mode.",
						),
					);
					sendWebSocketJson(socket, {
						type: "snapshot",
						payload: {
							snapshot: buildSnapshot(staticAccounts, fixedAccountSelector),
							snapshotPublishedAtUnixEpoch: lastPublishedAt,
						},
					});
					continue;
				}

				sendWebSocketJson(
					socket,
					dashboardControlAck(
						message,
						false,
						"unsupported",
						"Mock dashboard control action is unsupported.",
					),
				);
			}
		});
	});

	server.listen(port, host, () => {
		const baseUrl = `http://${host}:${port}`;
		const webSocketUrl = `ws://${host}:${port}/dashboard/control`;
		console.log(`operator dashboard mock: ${baseUrl}/dashboard`);
		console.log(`operator dashboard websocket: ${webSocketUrl}`);
		console.log(`Decodex App mock base: DECODEX_APP_SERVER_URL=${baseUrl}`);
		console.log("preview invariant: browser dashboard and Decodex App must use this same mock server");
		console.log(
			options.authDir
				? `accounts: ${options.authDir} (${staticAccounts.length} loaded)`
				: `accounts: ${staticAccounts.length} synthetic fixture accounts`,
		);
	});
}

main().catch((error) => {
	console.error(error?.message || error);
	process.exit(1);
});
