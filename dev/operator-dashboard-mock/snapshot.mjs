import { accountsWithSelection } from "./accounts.mjs";
import {
	activeReviewLifecycleMetrics,
	activeRun,
	childAgentActivity,
	historyLane,
	postReviewLanes,
	queuedCandidates,
	worktrees,
} from "./fixtures.mjs";
import { ago, later, unixLater } from "./time.mjs";

export function buildSnapshot(accounts, fixedAccountSelector) {
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
				repo_root: "/Users/x/code/acg-box/decodex",
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
