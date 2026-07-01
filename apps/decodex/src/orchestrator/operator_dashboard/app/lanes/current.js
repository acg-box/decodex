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
