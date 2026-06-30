
			function renderQueuedCandidates(container, items) {
				if (!items.length) {
					renderRoutineEmptyList(container);
					return;
				}

				container.innerHTML = items
					.map((candidate) => {
						const tone = toneForQueuedCandidate(candidate);
						const blockers = candidate.blocker_identifiers.length
							? candidate.blocker_identifiers.join(", ")
							: "NONE";
						const summary = summarizeQueuedCandidate(candidate);
						const reason = queuedCandidateInlineReason(candidate);

						return `
							<article class="action-card ${tone}">
								<div class="row-head">
									<div class="row-title">
										<div class="kicker">
											<span>Issue</span>
											<span class="mono">${escapeHtml(candidate.issue_identifier)}</span>
										</div>
										<h4>${escapeHtml(candidate.title)}</h4>
									</div>
								</div>
								${summary ? `<p class="row-summary">${escapeHtml(summary)}</p>` : ""}
								<div class="status-line">
									${statusLabel(queuedCandidateStatusText(candidate), tone)}
									${reason ? inlineStatusFact("Reason", reason) : ""}
								</div>
								${renderAttentionFacts(candidate)}
								<div class="grid two card-facts">
									${cardField("State", formatDetailToken(candidate.state))}
									${cardField("Priority", formatPriority(candidate.priority))}
									${cardField("Created", formatTimestampCompact(candidate.created_at), "is-time")}
									${cardField("Blockers", blockers, blockers === "NONE" ? "is-muted" : "")}
								</div>
							</article>
						`;
					})
					.join("");
			}

			function renderActionCards(container, items) {
				if (!items.length) {
					renderRoutineEmptyList(container);
					return;
				}

				container.innerHTML = items
					.map(
						(item) => `
							<article class="action-card ${item.tone}">
								<div class="row-head">
									<div class="row-title">
										<div class="kicker">
											<span>${escapeHtml(item.scope)}</span>
											<span class="mono">${escapeHtml(item.issue)}</span>
										</div>
										<h4>${escapeHtml(item.title)}</h4>
									</div>
								</div>
								${item.summary ? `<p class="row-summary">${escapeHtml(item.summary)}</p>` : ""}
								<div class="status-line">
									${statusLabel(item.status, item.tone)}
								</div>
								<div class="grid two card-facts">
									${item.facts.map(([label, value, valueClass]) => cardField(label, value, cardFactValueClass(value, valueClass))).join("")}
								</div>
							</article>
						`,
					)
					.join("");
			}

			function reviewLaneItems(derived) {
				const rankedItems = [
					...derived.attentionItems
						.filter((item) => ["Review", "Closeout", "Cleanup"].includes(item.scope))
						.map((item) => ({ ...item, sortRank: 0 })),
					...derived.readyItems.map((item) => ({ ...item, sortRank: 1 })),
					...derived.waitingItems
						.filter((item) => item.scope === "Review")
						.map((item) => ({ ...item, sortRank: 2 })),
				];

				return rankedItems.sort(
					(left, right) => left.sortRank - right.sortRank || left.issue.localeCompare(right.issue),
				);
			}

			function renderCurrentLanes(snapshot, derived) {
				const cards = derived.currentLaneCards ?? snapshotCurrentLaneCards(snapshot);
				setPanelMeta(
					nodes.currentLanesMeta,
					runningLaneMetaText(derived),
					derived.runningAttentionCount ? "attention" : "",
				);

				if (!cards.length) {
					renderRoutineEmptyList(nodes.currentLanes);
					return;
				}

				renderStableList(
					nodes.currentLanes,
					cards
						.map((card, index) => {
							const run = card.run || {};
							const tone = currentLaneCardToneClass(card);
							const statusBits = [statusLabel(runPhaseLabel(run), tone)];
							const detailKey = runDetailKey(run);
							const renderKey = card.id || card.run_id || `presentation-card:${index}`;
							const issueKey = card.issue_identifier || "unknown";
							const issueTitle = card.title || "Run";
							const summary = currentLaneVisibleSummary(card, run);
							if (run.ownership_state && run.ownership_state !== "leased_run") {
								statusBits.push(inlineStatusFact("Owner", displayToken(run.ownership_state)));
							}
							if (run.policy_state && !["allowed", "review_findings", "review_pending"].includes(run.policy_state)) {
								statusBits.push(inlineStatusFact("Policy", displayToken(run.policy_state)));
							}

							if (run.wait_reason && !runWaitReasonShowsExecutionProgress(run)) {
								const waitReason = displayToken(run.wait_reason);
								if (!displayTextRepeats(summary, waitReason)) {
									statusBits.push(inlineStatusFact("Wait", waitReason));
								}
							}
							if (runTelemetryMissing(run)) {
								const telemetryFact = runHasChildAgentActivity(run)
									? ["Metadata", "Pending"]
									: ["Telemetry", "Missing"];
								if (!displayTextRepeats(summary, telemetryFact.join(" "))) {
									statusBits.push(inlineStatusFact(telemetryFact[0], telemetryFact[1]));
								}
							}
							if (
								!runStoppedProcessNeedsAttention(run) &&
								runProcessStoppedWhileActive(run)
							) {
								statusBits.push(inlineStatusFact("Agent", "Done"));
							}
							if (run.interactive_requested && !runStoppedProcessNeedsAttention(run)) {
								if (!displayTextRepeats(summary, "operator input")) {
									statusBits.push(inlineStatusFact("Operator", "Input"));
								}
							}
							if (run.continuation_pending) {
								if (!displayTextRepeats(summary, "continuation pending")) {
									statusBits.push(inlineStatusFact("Continuation", "Pending"));
								}
							}
							const loopInline = loopStatusInline(run.loop_status);
							if (loopInline && !displayTextRepeats(summary, loopInline)) {
								statusBits.push(inlineStatusFact("Loop", loopInline));
							}
							const attemptNumber = attemptNumberFromRun(run);
							return `
							<article class="run-card ${tone}" data-render-key="${escapeHtml(renderKey)}">
								<div class="row-head">
									<div class="run-title-stack">
										<div class="run-subtitle">
											<span>Issue</span>
											<span class="mono">${escapeHtml(issueKey)}</span>
										</div>
										<h3 class="run-title">${escapeHtml(issueTitle)}</h3>
									</div>
									<div class="row-aside">
										${attemptNumber ? `<span>Attempt ${escapeHtml(attemptNumber)}</span>` : ""}
										${runNeedsAttention(run) ? `<span>${escapeHtml(runHealthText(run))}</span>` : ""}
									</div>
								</div>
									<div class="status-line">${statusBits.join("")}</div>
									${summary ? `<p class="row-summary">${escapeHtml(summary)}</p>` : ""}
									${renderRunMetaLine(run, snapshot)}
									${renderChildAgentBreakdown(run)}
									<details data-detail-key="${escapeHtml(detailKey)}"${detailsOpenAttribute(detailKey)}>
										<summary>Debug Details</summary>
									<div class="grid debug-grid">
										${field("Run", run.run_id)}
										${field("Attempt status", run.attempt_status || run.status)}
										${field("Run phase", capturedValue(run.run_phase || run.phase))}
										${field("Current operation", capturedValue(run.current_operation))}
										${field("Active goal phase", capturedValue(run.active_goal_phase))}
										${field("Public progress phase", capturedValue(run.public_progress_phase))}
										${field("Updated", formatTimestamp(run.updated_at))}
										${field("Codex thread", runThreadSummary(run))}
										${field("Thread flags", runThreadFlagSummary(run))}
										${field(COPY.protocolEvent, protocolEventSummary(run))}
										${field("Protocol activity", protocolActivityDebugSummary(run))}
										${field("Branch", run.branch_name || "none")}
										${field("Worktree", run.worktree_path || "none")}
										${field("Queue lease", runQueueLeaseSummary(run))}
										${field("Execution liveness", runExecutionLivenessSummary(run))}
										${field("Ownership", runOwnershipSummary(run))}
										${field("Liveness state", runLivenessStateSummary(run))}
										${field("Policy state", runPolicyStateSummary(run))}
										${field("Terminalization", runTerminalizationSummary(run))}
										${field("Lane next action", capturedValue(run.lane_control_next_action))}
										${field("Lane conditions", runLaneControlConditionsSummary(run))}
										${field("Continuation recovery", runContinuationRecoverySummary(run))}
										${field("Loop", run.loop_status?.summary || "none")}
										${field("Review loop", loopStatusFacts(run.loop_status).map(([label, value]) => `${label}: ${value}`).join("; ") || "none")}
										${field("Autonomy readback", autonomyReadbackSummary(run.loop_status))}
										${field("Model", runModelSummary(run))}
										${field("Child agent", childAgentDebugSummary(childAgentActivity(run)))}
										${field("Context pressure", childAgentContextSummary(childAgentActivity(run)))}
										${field("Lifecycle recovery", lifecycleRecoveryDebugSummary(currentLaneLifecycleMetrics(run)))}
										${field("Lifecycle evidence", lifecycleEvidenceDebugSummary(currentLaneLifecycleMetrics(run)))}
										${field("Large outputs", childAgentLargeOutputSummary(childAgentActivity(run)))}
										${field("Next retry", formatTimestamp(run.next_retry_at))}
										${field("Turn", run.turn_id || "none")}
										${field("Event count", String(run.event_count))}
										${field("Process", runProcessSummary(run))}
										${field("Effective cwd", capturedValue(run.effective_cwd))}
										${field("Approvals", runApprovalSummary(run))}
										${field("Sandbox", capturedValue(run.effective_sandbox_mode))}
									</div>
								</details>
							</article>
						`;
						})
						.join(""),
				);
			}

			function renderPhaseBreakdown(lane) {
				const phases = historyLaneLifecycleMetrics(lane).phases || [];

				if (!phases.length) {
					return "";
				}

				return `
					<details class="phase-timeline" data-detail-key="${escapeHtml(`history-phases:${lane.issue_key}`)}"${detailsOpenAttribute(`history-phases:${lane.issue_key}`)}>
						<summary>${escapeHtml(pluralize(phases.length, "lifecycle bucket"))}</summary>
						<div class="phase-list">
							${phases
								.map(
									(phase) => `
										<div class="phase-row">
											<span class="phase-name">${escapeHtml(phase.label)}</span>
											<span class="phase-facts">
												<span><strong>${escapeHtml(String(phase.attempt_count))}</strong> ${escapeHtml(phase.attempt_count === 1 ? "attempt" : "attempts")}</span>
												<span><strong>${escapeHtml(historyLifecycleTokenSummary(phase))}</strong></span>
												<span><strong>${escapeHtml(formatDuration(phase.wall_seconds))}</strong></span>
												<span><strong>${escapeHtml(formatCompactCount(phase.tool_call_count))}</strong> tools</span>
												<span><strong>${escapeHtml(formatCompactCount(phase.protocol_event_count))}</strong> events</span>
											</span>
										</div>
									`,
								)
								.join("")}
						</div>
					</details>
				`;
			}

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

			function worktreeRoleMeta(worktree, snapshot) {
				const hygiene = worktree.hygiene;
				const currentLaneMatch = snapshotCurrentLaneRuns(snapshot).find(
					(run) =>
						(run.worktree_path === worktree.worktree_path ||
							run.branch_name === worktree.branch_name ||
							run.issue_id === worktree.issue_id),
				);
				const reviewMatch = (snapshot?.post_review_lanes ?? []).find(
					(lane) =>
						lane.worktree_path === worktree.worktree_path ||
						lane.branch_name === worktree.branch_name ||
						lane.issue_id === worktree.issue_id ||
						lane.issue_identifier === worktree.issue_id,
				);

				if (currentLaneMatch) {
					return {
						sortRank: 0,
						tone: "tone-run",
						label: "current lane",
						summary: worktree.ownership_reason || "Leased by a running lane.",
					};
				}
				if (reviewMatch) {
					if (hygiene) {
						const isDirty = hygiene.classification === "merged_dirty_worktree" || hygiene.dirty === true;

						return {
							sortRank: 1,
							tone: isDirty ? "tone-wait" : "tone-retained",
							label: isDirty ? "post-review cleanup blocked" : "post-review cleanup",
							summary:
								hygiene.reason ||
								worktree.ownership_reason ||
								"Post-review cleanup pending.",
						};
					}

					return {
						sortRank: 1,
						tone: toneForLane(reviewMatch),
						label: `post-review ${displayToken(reviewMatch.classification)}`,
						summary:
							worktree.ownership_reason ||
							"Retained for review, landing, or closeout.",
					};
				}
				const queueMatch = (snapshot?.queued_candidates ?? []).find((candidate) => {
					const attentionWorktree = candidate.attention?.worktree_path;

					return (
						(candidate.reason === "issue_needs_attention" ||
							candidate.reason === "linear_active_label_present") &&
						(attentionWorktree === worktree.worktree_path ||
							candidate.issue_id === worktree.issue_id ||
							candidate.issue_identifier === worktree.issue_id)
					);
				});

				if (queueMatch) {
					return {
						sortRank: 0,
						tone: "tone-blocked",
						label: "queued attention",
						summary:
							worktree.ownership_reason ||
							"Owned by Intake Queue attention; recover there before cleanup.",
					};
				}
				if (worktree.ownership === "current_lane") {
					return {
						sortRank: 0,
						tone: "tone-run",
						label: "current lane",
						summary: worktree.ownership_reason || "Leased by a running lane.",
					};
				}
				if (worktree.ownership === "queued_attention") {
					return {
						sortRank: 0,
						tone: "tone-blocked",
						label: "queued attention",
						summary:
							worktree.ownership_reason ||
							"Owned by Intake Queue attention; recover there before cleanup.",
					};
				}
				if (worktree.ownership === "post_review_lane") {
					if (hygiene) {
						const isDirty = hygiene.classification === "merged_dirty_worktree" || hygiene.dirty === true;

						return {
							sortRank: 1,
							tone: isDirty ? "tone-wait" : "tone-retained",
							label: isDirty ? "post-review cleanup blocked" : "post-review cleanup",
							summary:
								hygiene.reason ||
								worktree.ownership_reason ||
								"Post-review cleanup pending.",
						};
					}

					return {
						sortRank: 1,
						tone: "tone-retained",
						label: "post-review retained",
						summary:
							worktree.ownership_reason ||
							"Retained for review, landing, or closeout.",
					};
				}
				if (hygiene) {
					const isDirty = hygiene.classification === "merged_dirty_worktree" || hygiene.dirty === true;

					return {
						sortRank: 2,
						tone: isDirty ? "tone-wait" : "tone-retained",
						label: isDirty ? "post-land cleanup blocked" : "post-land cleanup",
						summary:
							hygiene.reason ||
							worktree.ownership_reason ||
							"Post-land cleanup pending.",
					};
				}
				if (worktree.ownership === "post_land_cleanup") {
					return {
						sortRank: 2,
						tone: "tone-retained",
						label: "post-land cleanup",
						summary:
							worktree.ownership_reason ||
							"Post-land cleanup pending.",
					};
				}
				if (worktree.provenance?.audit_required) {
					return {
						sortRank: 2,
						tone: "tone-blocked",
						label: "legacy cleanup audit",
						summary:
							worktree.ownership_reason ||
							"Legacy worktree provenance is missing; verify terminal state before cleanup.",
					};
				}
				return {
					sortRank: 2,
					tone: "tone-recovery",
					label: "local cleanup",
					summary:
						worktree.ownership_reason ||
						"No lane owns this worktree; inspect before cleanup.",
				};
			}

			function renderWorktreeHygieneFields(worktree) {
				const hygiene = worktree.hygiene;
				if (!hygiene) {
					return "";
				}

				return `
					${field("Cleanup state", displayToken(hygiene.classification || "cleanup_pending"))}
					${field("Default branch", hygiene.default_branch || "unknown")}
					${field("Uncommitted changes", hygiene.dirty ? "yes" : "no")}
				`;
			}

			function renderWorktreeProvenanceFields(worktree) {
				const provenance = worktree.provenance;
				if (!provenance) {
					return "";
				}

				const createdAt = unixEpochSecondsToIso(provenance.created_at_unix) || "unknown";
				const updatedAt = unixEpochSecondsToIso(provenance.updated_at_unix) || "unknown";
				const audit = provenance.audit_required ? field("Audit", "required") : "";
				const nextAction = worktree.recovery_next_action
					? field("Next action", worktree.recovery_next_action)
					: "";

				return `
					${field("Provenance", displayToken(provenance.source || "unknown"))}
					${field("Recorded", createdAt)}
					${field("Refreshed", updatedAt)}
					${audit}
					${nextAction}
				`;
			}

			function recoveryWorktreeShouldDefaultOpen(renderedWorktree) {
				const role = renderedWorktree.role;

				return role.tone === "tone-blocked";
			}

			function renderWorktrees(snapshot) {
				const worktrees = snapshot?.worktrees ?? [];
				const renderedWorktrees = worktrees
					.map((worktree) => ({ worktree, role: worktreeRoleMeta(worktree, snapshot) }))
					.sort((left, right) => {
						const rankDelta = left.role.sortRank - right.role.sortRank;
						if (rankDelta) {
							return rankDelta;
						}
						return (
							String(left.worktree.issue_id).localeCompare(String(right.worktree.issue_id)) ||
							String(left.worktree.branch_name).localeCompare(String(right.worktree.branch_name)) ||
							String(left.worktree.worktree_path).localeCompare(String(right.worktree.worktree_path))
						);
					});
				const retainedWorktrees = renderedWorktrees.filter(({ role }) => role.sortRank > 0);
				setFoldPanelEmpty(nodes.panels.worktrees, !retainedWorktrees.length);
				syncDefaultDetailOpenState(
					nodes.panels.worktrees,
					retainedWorktrees.some(recoveryWorktreeShouldDefaultOpen),
				);

				setPanelMeta(
					nodes.worktreesMeta,
					retainedWorktrees.length
						? pluralize(retainedWorktrees.length, "worktree")
						: "0 worktrees",
				);

				if (!retainedWorktrees.length) {
					nodes.worktrees.innerHTML = "";
					return;
				}

				nodes.worktrees.innerHTML = retainedWorktrees
					.map(({ worktree, role }) => {
						const issueKey = issueDisplayKey(worktree);

						return `
							<article class="worktree-card ${role.tone}">
								<div class="row-head">
									<div class="row-title">
										<div class="kicker">
											<span>Issue</span>
											<span class="mono">${escapeHtml(issueKey)}</span>
										</div>
										<h4>${escapeHtml(worktree.branch_name)}</h4>
									</div>
								</div>
								<div class="status-line">
									${statusLabel(role.label, role.tone)}
								</div>
								<p class="row-summary">${escapeHtml(role.summary)}</p>
								<div class="grid two">
									${field("Issue state", worktree.issue_state || "unknown")}
									${field("Ownership", displayToken(worktree.ownership || role.label))}
									${field("Branch", worktree.branch_name)}
									${field("Worktree path", worktree.worktree_path)}
									${renderWorktreeProvenanceFields(worktree)}
									${renderWorktreeHygieneFields(worktree)}
								</div>
							</article>
						`;
					})
					.join("");
			}
