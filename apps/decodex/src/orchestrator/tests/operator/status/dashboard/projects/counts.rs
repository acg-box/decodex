use crate::orchestrator::tests::operator::status::dashboard;

#[test]
fn operator_dashboard_empty_lane_meta_uses_counts() {
	let response = dashboard::dashboard_response();

	assert!(!response.contains("Snapshot pending"));
	assert!(!response.contains("COPY.waitingSnapshot"));
	assert!(response.contains("runningLaneMetaText(derived),"));
	assert!(response.contains(": \"0 issues · 0 attempts\","));
	assert!(response.contains(": \"0 PRs · 0 need attention · 0 ready · 0 waiting · 0 cleanup\","));
	assert!(response.contains("const parts = [`${derived.liveRuns ?? 0} running`];"));
	assert!(
		response.contains("const parts = [`${derived.queueBacklogCandidates.length} queued`];")
	);
	assert!(response.contains("return \"0 queued\";"));
	assert!(
		response.contains("setPanelMeta(nodes.queuedMeta, backlogMetaText(snapshot, derived));")
	);
	assert!(response.contains(": \"0 worktrees\","));
	assert!(!response.contains("queue empty"));
	assert!(!response.contains("No running lanes"));
	assert!(!response.contains("No queued issues"));
	assert!(!response.contains("No PR lanes"));
	assert!(!response.contains("No recovery worktrees"));
}

#[test]
fn operator_dashboard_flow_counts_distinguish_intake_attention() {
	let response = dashboard::dashboard_response();

	assert!(response.contains("queuedCandidateNeedsAttention"));
	assert!(response.contains("intakeAttentionCount"));
	assert!(response.contains("queuedBlockedWithoutAttention"));
	assert!(
		response.contains("attention.thread_status && attention.thread_status !== \"systemError\"")
	);
	assert!(
		response.contains("queueBacklogCandidates.filter(queuedCandidateNeedsAttention).length")
	);
	assert!(response.contains(
		"${pluralize(derived.postReviewLanes.length, \"PR\")} · ${pluralize(derived.reviewBlockerCount, \"needs attention\", \"need attention\")} · ${derived.readyItems.length} ready · ${derived.reviewWaitingCount} waiting · ${derived.cleanupCount} cleanup"
	));
	assert!(response.contains("const cleanupIssueKeys = new Set();"));
	assert!(response.contains("const cleanupCount = cleanupIssueKeys.size;"));
	assert!(response.contains("? pluralize(retainedWorktrees.length, \"worktree\")"));
	assert!(!response.contains("retained or cleanup"));
	assert!(response.contains("function recoveryWorktreeShouldDefaultOpen(renderedWorktree)"));
	assert!(response.contains("role.tone === \"tone-blocked\""));
	assert!(!response.contains("role.label.includes(\"cleanup\")"));
	assert!(
		response
			.contains("label: isDirty ? \"post-review cleanup blocked\" : \"post-review cleanup\"")
	);
	assert!(response.contains("retainedWorktrees.some(recoveryWorktreeShouldDefaultOpen)"));
	assert!(!response.contains(
		"syncDefaultDetailOpenState(nodes.panels.worktrees, retainedWorktrees.length > 0);"
	));
	assert!(!response.contains("claimed without local lane"));
	assert!(!response.contains("const repairCount = attentionItems.length;"));
}

#[test]
fn operator_dashboard_does_not_hide_claimed_queue_without_local_lane() {
	let response = dashboard::dashboard_response();

	assert!(response.contains("const currentLaneByIssue = new Map();"));
	assert!(response.contains("for (const key of issueIdentityKeys(run))"));
	assert!(response.contains("const currentLane = issueIdentityKeys(candidate)"));
	assert!(response.contains("if (currentLane) {"));
	assert!(!response.contains("currentLane && candidate.classification === \"claimed\""));
	assert!(!response.contains("candidate.classification !== \"claimed\" &&"));
}

#[test]
fn operator_dashboard_prioritizes_needs_attention_reason_over_retry_count() {
	let response = dashboard::dashboard_response();
	let reason_text = response
		.split("function queuedCandidateReasonText(candidate)")
		.nth(1)
		.expect("queued candidate reason function should exist")
		.split("function queuedCandidateNeedsAttention(candidate)")
		.next()
		.expect("queued candidate reason function should have an end");

	assert!(reason_text.contains("return displayToken(candidate.reason);"));
	assert!(
		response
			.contains("facts.push([\"Attempt status\", displayToken(attention.attempt_status)]);")
	);
	assert!(response.contains(
		"facts.push([\"Failed attempts\", `${attention.retry_budget_attempt_count}${retryMax}`]);"
	));
	assert!(response.contains(
		"facts.push([\"Auto retry\", autoRetryBlockedReasonText(attention.auto_retry_blocked_reason)]);"
	));
	assert!(response.contains("return displayToken(reason);"));
	assert!(reason_text.contains("return \"retry_budget_attempt_count\";"));
	assert!(response.contains("function queuedCandidateInlineReason(candidate)"));
	assert!(response.contains(
		"displayTextRepeats(reason, displayToken(candidate.attention.attention_error_class))"
	));
	assert!(response.contains("displayTextRepeats(reason, \"worktree_has_tracked_changes\")"));
	assert!(!response.contains("return \"blocked by needs-attention\";"));
	assert!(!reason_text.contains("return \"Retry budget held\";"));
	assert!(
		!response
			.contains("facts.push([\"Retry\", String(attention.retry_budget_attempt_count)]);")
	);
	assert!(
		reason_text
			.find("if (candidate.attention?.attention_error_class)")
			.expect("attention error-class reason should exist")
			< reason_text
				.find("return \"retry_budget_attempt_count\";")
				.expect("retry-budget reason should exist")
	);
}
