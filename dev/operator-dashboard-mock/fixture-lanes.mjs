import { ago, later } from "./time.mjs";

import { activeRunLifecycleMetrics, childAgentActivity } from "./fixture-lifecycle.mjs";

export function activeRun({
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
		effective_cwd: `/Users/x/code/acg-box/decodex/.worktrees/${issue}`,
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
		worktree_path: `/Users/x/code/acg-box/decodex/.worktrees/${issue}`,
	};
}

export function queuedCandidates() {
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
				worktree_path: "/Users/x/code/acg-box/decodex/.worktrees/XY-452",
				worktree_has_tracked_changes: true,
			},
			blocker_identifiers: ["XY-399"],
		},
	];
}
export function postReviewLanes() {
	return [
		{
			issue_id: "issue-xy-460",
			issue_identifier: "XY-460",
			issue_state: "In Review",
			branch_name: "xy/xy-460-ready",
			worktree_path: "/Users/x/code/acg-box/decodex/.worktrees/XY-460",
			classification: "ready_to_land",
			reason: "",
			pr_url: "https://github.com/acg-box/decodex/pull/460",
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
			worktree_path: "/Users/x/code/acg-box/decodex/.worktrees/XY-461",
			classification: "wait_for_review",
			reason: "",
			pr_url: "https://github.com/acg-box/decodex/pull/461",
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
			worktree_path: "/Users/x/code/acg-box/decodex/.worktrees/XY-462",
			classification: "closeout_blocked",
			reason: "",
			pr_url: "https://github.com/acg-box/decodex/pull/462",
			pr_state: "MERGED",
			review_decision: "APPROVED",
			mergeable: "MERGEABLE",
			check_state: "SUCCESS",
			unresolved_review_threads: 0,
		},
	];
}
export function worktrees() {
	return [
		{
			issue_id: "issue-xy-445",
			issue_identifier: "XY-445",
			issue_state: "In Progress",
			branch_name: "xy/xy-445-mock",
			worktree_path: "/Users/x/code/acg-box/decodex/.worktrees/XY-445",
			ownership: "active_lane",
			ownership_reason: "Currently leased by a running lane.",
		},
		{
			issue_id: "issue-xy-460",
			issue_identifier: "XY-460",
			issue_state: "In Review",
			branch_name: "xy/xy-460-ready",
			worktree_path: "/Users/x/code/acg-box/decodex/.worktrees/XY-460",
			ownership: "post_review_lane",
			ownership_reason: "Retained for review, landing, or closeout follow-up.",
		},
		{
			issue_id: "issue-xy-499",
			issue_identifier: "XY-499",
			issue_state: "Canceled",
			branch_name: "xy/xy-499-orphan",
			worktree_path: "/Users/x/code/acg-box/decodex/.worktrees/XY-499",
			ownership: "local_cleanup",
			ownership_reason:
				"No active lane, queued recovery, or post-review lane owns this worktree; local cleanup only.",
		},
	];
}
