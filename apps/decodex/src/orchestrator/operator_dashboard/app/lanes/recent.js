			function renderRecentRuns(snapshot) {
				const rawRuns = rawSessionHistoryRuns(snapshot);
				const lanes = sessionHistoryLanes(snapshot);
				const hiddenActiveCount = Math.max((snapshot?.recent_runs?.length ?? 0) - rawRuns.length, 0);
				const attemptCount = lanes.reduce((total, lane) => total + (lane.attempt_count ?? lane.attempts?.length ?? 1), 0);
				setFoldPanelEmpty(nodes.panels.recent, !lanes.length);
				syncDefaultDetailOpenState(nodes.panels.recent, false);
				setPanelMeta(
					nodes.recentRunsMeta,
					snapshot
						? `${pluralize(lanes.length, "issue")} · ${pluralize(attemptCount, "attempt")}${
								hiddenActiveCount
									? ` · ${pluralize(hiddenActiveCount, COPY.runningInlineMeta, COPY.runningInlineMetaPlural)}`
									: ""
							}`
						: "0 issues · 0 attempts",
				);

				if (!lanes.length) {
					nodes.recentRuns.innerHTML = renderEmptyState(
						hiddenActiveCount ? "No separate run history" : "No run history",
						hiddenActiveCount
							? `Current records are already shown in ${COPY.runningLane}.`
							: "Completed or interrupted lanes appear here.",
					);
					return;
				}

				nodes.recentRuns.innerHTML = lanes
					.map((lane) => {
						const run = lane.latest_run;
						const outcome = historyLedgerOutcome(lane);
						const tone = toneForHistoryLedgerOutcome(outcome, run);
						const statusBits = historyLaneStatusBits(lane, tone);
						const detailKey = runDetailKey(run);
						const issueKey = lane.issue_key || issueDisplayKey(run);
						const title = historyLaneTitle(lane);
						const summary = historyLaneSummary(lane);
						const finishedAt = outcome.lifecycle_finished_at || outcome.final_event_at || run.updated_at;

						return `
							<article class="run-card ${tone}">
								<div class="row-head">
									<div class="row-title">
										<div class="kicker">
											<span>Issue</span>
											<span class="mono">${escapeHtml(issueKey)}</span>
										</div>
										<h4>${escapeHtml(title)}</h4>
									</div>
									<div class="row-aside">
										<span>${escapeHtml(titleCaseLabel(pluralize(lane.attempt_count ?? 1, "attempt")))}</span>
										<span>${escapeHtml(formatTimestamp(finishedAt))}</span>
									</div>
								</div>
								<p class="row-summary">${escapeHtml(summary)}</p>
								<div class="status-line">${statusBits.join("")}</div>
								${renderHistoryTimingStrip(lane)}
								${renderHistoryLifecycleFacts(lane)}
								${renderHistoryLedgerFacts(lane)}
								${renderPhaseBreakdown(lane)}
								<details data-detail-key="${escapeHtml(detailKey)}"${detailsOpenAttribute(detailKey)}>
									<summary>Latest Run Details</summary>
									<div class="grid debug-grid">
										${field("Run", run.run_id)}
										${field("Issue id", run.issue_id)}
										${field("Updated", formatTimestamp(run.updated_at))}
										${field("Codex thread", runThreadSummary(run))}
										${field(COPY.protocolEvent, protocolEventSummary(run))}
										${field("Protocol activity", protocolActivityDebugSummary(run))}
										${field("Lifecycle totals", historyLifecycleTokenSummary(historyLaneLifecycleMetrics(lane)))}
										${field("Branch", run.branch_name || "none")}
										${field("Worktree", run.worktree_path || "none")}
										${field("Model", runModelSummary(run))}
										${field("Account", codexAccountDebugSummary(codexAccount(run, snapshot)))}
										${field("Child agent", childAgentDebugSummary(childAgentActivity(run)))}
										${field("Context pressure", childAgentContextSummary(childAgentActivity(run)))}
										${field("Large outputs", childAgentLargeOutputSummary(childAgentActivity(run)))}
										${field("Next retry", formatTimestamp(run.next_retry_at))}
										${field("Thread flags", runThreadFlagSummary(run))}
										${field("Turn", run.turn_id || "none")}
										${field("Event count", String(run.event_count))}
										${field("Last protocol activity", formatTimestamp(run.last_protocol_activity_at))}
										${field("Protocol idle", formatDuration(run.protocol_idle_for_seconds))}
										${field("Process", runProcessSummary(run))}
										${field("Effective cwd", run.effective_cwd || "none")}
										${field("Approvals", runApprovalSummary(run))}
										${field("Sandbox", run.effective_sandbox_mode || "none")}
									</div>
								</details>
							</article>
						`;
					})
					.join("");
			}
