
			function historyLaneTimingFacts(lane) {
				const outcome = historyLedgerOutcome(lane);
				const metrics = historyLaneLifecycleMetrics(lane);
				const modelSeconds = lifecycleBucketSeconds(metrics, "Model");
				const activitySeconds = modelSeconds > 0
					? modelSeconds
					: lifecycleNumber(metrics.wall_seconds);
				const activityLabel = modelSeconds > 0 ? "Inference" : "Activity";

				if (!historyLedgerHasRecords(outcome)) {
					const run = lane.latest_run;

					return [
						["Updated", formatTimestampCompact(run.updated_at)],
						["Attempts", String(metrics.attempt_count || lane.attempt_count || 1)],
						[activityLabel, formatDuration(activitySeconds)],
						["Tokens", historyLifecycleTokenSummary(metrics)],
						["Events", formatCompactCount(metrics.protocol_event_count)],
					];
				}

				return [
					["Finished", formatTimestampCompact(outcome.lifecycle_finished_at || outcome.final_event_at)],
					["Elapsed", formatDuration(outcome.lifecycle_elapsed_seconds)],
					["Attempts", String(metrics.attempt_count || lane.attempt_count || 1)],
					[activityLabel, formatDuration(activitySeconds)],
					["Tokens", historyLifecycleTokenSummary(metrics)],
					["Events", formatCompactCount(metrics.protocol_event_count || outcome.record_count || 0)],
				];
			}

			function renderHistoryTimingStrip(lane) {
				return `
					<div class="timing-strip" aria-label="Run outcome timing">
						${historyLaneTimingFacts(lane)
							.map(
								([label, value]) => `
									<div class="timing-cell">
										<div class="timing-label">${escapeHtml(label)}</div>
										<div class="timing-value">${escapeHtml(value)}</div>
									</div>
								`,
							)
							.join("")}
					</div>
				`;
			}

			function childAgentActivity(run) {
				return run?.child_agent_activity || null;
			}

			function childAgentCurrentSummary(summary) {
				if (!summary?.current_bucket) {
					return null;
				}

				const label = detailLabel(displayToken(summary.current_detail || summary.current_bucket));

				return `${label} · ${formatDuration(summary.current_elapsed_seconds)}`;
			}

			function childAgentInputWindowsMatch(summary) {
				return (
					summary?.input_tokens_current != null &&
					summary?.input_tokens_max != null &&
					Number(summary.input_tokens_current) === Number(summary.input_tokens_max)
				);
			}

				function childAgentContextFacts(summary) {
					if (!summary) {
						return [];
					}

					const latestInput = formatCompactCount(summary.input_tokens_current);
					const peakInput = formatCompactCount(summary.input_tokens_max);
					const inputWindowsMatch = childAgentInputWindowsMatch(summary);
					const facts = [
						["current window", latestInput, "Current context window from the latest child-agent event."],
					];

					if (!inputWindowsMatch) {
						facts.push([
							"peak window",
							peakInput,
							"Largest observed context window for this lane.",
						]);
					}

					return facts;
				}

			function childAgentLargeOutputWarnings(summary) {
				return Array.isArray(summary?.large_output_warnings)
					? summary.large_output_warnings.filter(Boolean)
					: [];
			}

			function childBucketMetricNumber(bucket, field) {
				const value = Number(bucket?.[field] || 0);

				return Number.isFinite(value) ? Math.max(0, value) : 0;
			}

			function childBucketWallSeconds(bucket) {
				return childBucketMetricNumber(bucket, "wall_seconds");
			}

			function childBucketEventSignals(bucket) {
				const signals = [];
				const events = childBucketMetricNumber(bucket, "event_count");
				const toolCalls = childBucketMetricNumber(bucket, "tool_call_count");
				const inputTokens = childBucketMetricNumber(bucket, "input_tokens");
				const outputTokens = childBucketMetricNumber(bucket, "output_tokens");
				const outputBytes = childBucketMetricNumber(bucket, "output_bytes");

				if (events > 0) {
					signals.push(["events", formatCompactCount(events)]);
				}
				if (toolCalls > 0) {
					signals.push(["tools", formatCompactCount(toolCalls)]);
				}
				if (inputTokens > 0) {
					signals.push(["input", `${formatCompactCount(inputTokens)} tok`]);
				}
				if (outputTokens > 0) {
					signals.push(["output", `${formatCompactCount(outputTokens)} tok`]);
				}
				if (outputBytes > 0) {
					signals.push(["output bytes", formatCompactBytes(outputBytes)]);
				}

				return signals;
			}

			function childBucketEventSummary(bucket) {
				return childBucketSignalSummary(childBucketEventSignals(bucket), "no attributed wall time");
			}

			function childBucketSignalSummary(signals, fallback) {
				return signals.length
					? signals.map(([label, value]) => `${detailLabel(label)} ${value}`).join(", ")
					: fallback;
			}

			function childBucketDisplayName(bucket) {
				return childBucketIsPrimaryShareBucket(bucket) ? "Inference" : displayToken(bucket?.name);
			}

			function renderChildBucketSignalList(signals) {
				return signals
					.map(
						([label, value]) => `
							<span class="child-bucket-signal">
								<span>${escapeHtml(detailLabel(label))}</span>
								<strong>${escapeHtml(value)}</strong>
							</span>
						`,
					)
					.join("");
			}

			function childBucketDiagnosticSignals(bucket) {
				const seconds = childBucketWallSeconds(bucket);
				const signals = [];

				if (seconds > 0) {
					signals.push(["wall", childBucketIsSubsecond(bucket) ? "<1s" : formatDuration(seconds)]);
				}

				return [...signals, ...childBucketEventSignals(bucket)];
			}

			function childBucketDiagnosticSummary(bucket) {
				return childBucketSignalSummary(childBucketDiagnosticSignals(bucket), "no diagnostic activity");
			}

			function renderChildBucketDiagnosticSignals(bucket) {
				return renderChildBucketSignalList(childBucketDiagnosticSignals(bucket));
			}

			function childBucketHasRecordedActivity(bucket) {
				return childBucketEventSignals(bucket).length > 0;
			}

			function childBucketIsSubsecond(bucket) {
				const seconds = childBucketWallSeconds(bucket);

				return seconds > 0 && seconds < 1;
			}

			function childBucketIsEventOnly(bucket) {
				return childBucketWallSeconds(bucket) === 0 && childBucketHasRecordedActivity(bucket);
			}

			function childBucketWallShare(bucket, totalWall) {
				return childBucketWallSeconds(bucket) / Math.max(1, totalWall);
			}

			function childBucketSharePercent(bucket, totalWall) {
				return Math.round(childBucketWallShare(bucket, totalWall) * 100);
			}

			function childBucketHasMeaningfulWallShare(bucket, totalWall) {
				return childBucketWallSeconds(bucket) >= 5 && childBucketWallShare(bucket, totalWall) >= 0.02;
			}

			function childBucketShareLabel(bucket, totalWall) {
				const seconds = childBucketWallSeconds(bucket);

				return `${childBucketSharePercent(bucket, totalWall)}% · ${formatDuration(seconds)} / ${formatDuration(totalWall)}`;
			}

			function childBucketRenderKey(bucket) {
				return `child-bucket:${bucket?.name || "unknown"}`;
			}

			function childBucketNormalizedName(bucket) {
				return String(bucket?.name || "").toLowerCase();
			}

			function childBucketIsPrimaryShareBucket(bucket) {
				return childBucketNormalizedName(bucket) === "model";
			}

			function childBucketIsLifecycleTotalBucket(bucket) {
				const name = childBucketNormalizedName(bucket);

				return name === "protocol" || name === "tracker";
			}

			function childDiagnosticBucketRank(bucket) {
				const name = childBucketNormalizedName(bucket);
				if (name.includes("protocol")) {
					return 0;
				}
				if (name.includes("tracker")) {
					return 1;
				}
				if (name.includes("tool")) {
					return 2;
				}

				return 3;
			}

			function childBucketDuration(bucket) {
				const seconds = childBucketWallSeconds(bucket);

				if (childBucketIsEventOnly(bucket)) {
					return childBucketEventSummary(bucket);
				}

				if (childBucketIsSubsecond(bucket)) {
					return "<1s";
				}

				if (seconds > 0) {
					return formatDuration(seconds);
				}

				return "0s";
			}

			function childBucketWidth(bucket, totalWall) {
				if (!childBucketHasMeaningfulWallShare(bucket, totalWall)) {
					return 0;
				}

				const seconds = childBucketWallSeconds(bucket);

				return seconds > 0 ? Math.max(1, Math.min(100, Math.round((seconds / totalWall) * 100))) : 0;
			}

			function childAgentBuckets(summary) {
				return [...(summary?.buckets || [])].sort((left, right) => {
					const wallDelta = (right.wall_seconds || 0) - (left.wall_seconds || 0);
					if (wallDelta !== 0) {
						return wallDelta;
					}

					const eventDelta =
						childBucketMetricNumber(right, "event_count") - childBucketMetricNumber(left, "event_count");
					if (eventDelta !== 0) {
						return eventDelta;
					}

					return String(left.name || "").localeCompare(String(right.name || ""));
				});
			}

			function renderChildContextFacts(facts) {
				return facts
					.map(([label, value, title]) => {
						const titleAttribute = title ? ` title="${escapeHtml(title)}"` : "";

						return `<span${titleAttribute}>${escapeHtml(detailLabel(label))} <strong>${escapeHtml(value)}</strong></span>`;
					})
					.join("");
			}

			function runProjectSummary(run) {
				return displayToken(run?.project_display_name || run?.project_id || "project");
			}

			function lifecycleMetricSegment(value, label = "") {
				if (value == null || value === "") {
					return null;
				}

				return {
					value: String(value),
					label: label ? String(label) : "",
				};
			}

			function appendLifecycleMetricSegment(segments, value, label = "") {
				const segment = lifecycleMetricSegment(value, label);
				if (segment) {
					segments.push(segment);
				}
			}

			function formatLargestOutputValue(bytes) {
				if (bytes == null) {
					return "-";
				}

				return formatCompactBytes(bytes);
			}

				function renderLifecycleMetricSegment(segment, slotIndex) {
					if (!segment) {
						return `
							<span class="child-total-segment is-empty" aria-hidden="true">
								<strong class="child-total-primary" data-slot="${slotIndex}">-</strong>
								<span class="child-total-secondary">-</span>
							</span>
						`;
					}

					const label = segment.label
						? `<span class="child-total-secondary">${escapeHtml(segment.label)}</span>`
						: `<span class="child-total-secondary"></span>`;

					return `<span class="child-total-segment"><strong class="child-total-primary" data-slot="${slotIndex}">${escapeHtml(segment.value)}</strong>${label}</span>`;
				}

			function renderLifecycleOverviewRows(rows) {
				const visibleRows = rows.filter((row) => row.segments.length);
				if (!visibleRows.length) {
					return "";
				}

				const cells = visibleRows
					.flatMap((row) => {
						const paddedSegments = [
							...row.segments.slice(0, 4),
							...Array.from({ length: Math.max(0, 4 - row.segments.length) }, () => null),
						];

							return [
								`<span class="child-total-label">${escapeHtml(row.label)}</span>`,
								...paddedSegments.map((segment, slotIndex) =>
									renderLifecycleMetricSegment(segment, slotIndex),
								),
							];
					})
					.join("");

				return `
					<div class="child-total-overview" aria-label="Lifecycle total metrics">
						${cells}
					</div>
				`;
			}

				function renderChildLifecycleOverview(lifecycle, contextFacts) {
					const rows = [];

					const contextSegments = [];
					if (lifecycleNumber(lifecycle?.input_tokens_cumulative) > 0) {
						appendLifecycleMetricSegment(
							contextSegments,
							formatCompactCount(lifecycle.input_tokens_cumulative),
							"input",
						);
					}
					if (lifecycleNumber(lifecycle?.output_tokens_cumulative) > 0) {
						appendLifecycleMetricSegment(
							contextSegments,
							formatCompactCount(lifecycle.output_tokens_cumulative),
							"output",
						);
					}
					for (const [label, value] of contextFacts) {
						const detail = detailLabel(label);
						appendLifecycleMetricSegment(
							contextSegments,
							value,
							detail,
						);
					}
					if (contextSegments.length) {
						rows.push({ label: "Context", segments: contextSegments });
					}

					const tracker = lifecycleBucket(lifecycle, "Tracker");
					if (tracker) {
						const trackerSegments = [];
						if (lifecycleNumber(tracker.event_count) > 0) {
							appendLifecycleMetricSegment(trackerSegments, formatCompactCount(tracker.event_count), "events");
						}
						if (lifecycleNumber(tracker.tool_call_count) > 0) {
							appendLifecycleMetricSegment(trackerSegments, formatCompactCount(tracker.tool_call_count), "tools");
						}
						if (lifecycleNumber(tracker.output_bytes) > 0) {
							appendLifecycleMetricSegment(trackerSegments, formatCompactBytes(tracker.output_bytes), "output bytes");
						}
						if (trackerSegments.length) {
							rows.push({ label: "Tracker", segments: trackerSegments });
						}
					}

					const protocolEvents = lifecycleNumber(lifecycle?.protocol_event_count);
					const childEvents = lifecycleNumber(lifecycle?.child_event_count);
					if (protocolEvents || childEvents) {
						const protocolSegments = [];
						if (protocolEvents) {
							appendLifecycleMetricSegment(protocolSegments, formatCompactCount(protocolEvents), "events");
						}
						if (childEvents) {
							appendLifecycleMetricSegment(protocolSegments, formatCompactCount(childEvents), "child events");
						}
						rows.push({ label: "Protocol", segments: protocolSegments });
					}

					return renderLifecycleOverviewRows(rows);
				}

				function renderChildLifecyclePhaseTable(phases) {
					const rows = (phases || [])
						.map((phase) => {
							const modelSeconds = lifecycleBucketSeconds(phase, "Model");
							const phaseWall = lifecycleWallSeconds(phase);
							const runtime = formatRuntimeShare(modelSeconds, phaseWall);
							const inputTokens = lifecycleNumber(phase.input_tokens_cumulative);
							const outputTokens = lifecycleNumber(phase.output_tokens_cumulative);
							const toolCalls = lifecycleNumber(phase.tool_call_count);
							const largestOutput = phase?.largest_tool_output_bytes != null
								? formatLargestOutputValue(phase.largest_tool_output_bytes)
								: "-";

							return [
								phase.label || displayToken(phase.phase),
								lifecycleNumber(phase.attempt_count) > 0
									? formatCompactCount(phase.attempt_count)
									: "-",
								runtime.text,
								inputTokens > 0 ? formatCompactCount(inputTokens) : "-",
								outputTokens > 0 ? formatCompactCount(outputTokens) : "-",
								toolCalls > 0 ? formatCompactCount(toolCalls) : "-",
								largestOutput,
							];
						});

					if (!rows.length) {
						return "";
					}

					const header = ["Lifecycle bucket", "attempts", "inference", "input", "output", "tools", "max output"];
					const alignRight = new Set([1, 2, 3, 4, 5, 6]);
					const cells = [
						...header.map(
							(label, index) =>
								`<span class="child-phase-table-cell is-header" data-align="${alignRight.has(index) ? "right" : "left"}">${escapeHtml(label)}</span>`,
						),
						...rows.flatMap((row) =>
							row.map(
								(value, index) =>
									renderChildLifecyclePhaseCell(value, index, alignRight.has(index)),
							),
						),
					].join("");

					return `<div class="child-phase-table" role="table" aria-label="Lifecycle bucket metrics">${cells}</div>`;
				}

				function renderChildLifecyclePhaseCell(value, index, alignRight) {
					const align = alignRight ? "right" : "left";
					return `<span class="child-phase-table-cell is-value" data-align="${align}">${escapeHtml(String(value))}</span>`;
				}

				function childAgentContextRows(run, summary, lifecycle = currentLaneLifecycleMetrics(run, summary)) {
					const rows = [];
					const contextFacts = childAgentContextFacts(summary);
					const overview = renderChildLifecycleOverview(lifecycle, contextFacts);
					const phaseTable = renderChildLifecyclePhaseTable(lifecycle.phases || []);

					if (overview) {
						rows.push(overview);
					}
					if (phaseTable) {
						rows.push(phaseTable);
					}

					return rows;
				}

			function renderChildAgentBreakdown(run) {
				const summary = childAgentActivity(run);

				if (!summary) {
					return "";
				}

				const lifecycle = currentLaneLifecycleMetrics(run, summary);
				const buckets = (lifecycle.buckets || []).length ? lifecycle.buckets : childAgentBuckets(summary);
				const totalWall = Math.max(
					1,
					Number(lifecycle.wall_seconds || 0),
					buckets.reduce((total, bucket) => total + Number(bucket.wall_seconds || 0), 0),
				);
				const current = childAgentCurrentSummary(summary) || "none";
				const contextRows = childAgentContextRows(run, summary, lifecycle);
				const shareBuckets = buckets.filter(
					(bucket) =>
						childBucketIsPrimaryShareBucket(bucket) &&
						childBucketHasMeaningfulWallShare(bucket, totalWall),
				);
				const diagnosticBuckets = buckets
					.filter(
						(bucket) =>
							!childBucketIsPrimaryShareBucket(bucket) &&
							!childBucketIsLifecycleTotalBucket(bucket) &&
							!childBucketHasMeaningfulWallShare(bucket, totalWall),
					)
					.sort(
						(left, right) =>
							childDiagnosticBucketRank(left) - childDiagnosticBucketRank(right) ||
							String(left.name || "").localeCompare(String(right.name || "")),
					);

				return `
					<div class="child-activity" aria-label="Child agent timing breakdown">
						<div class="child-activity-head is-project">
							<span>Project</span>
							<strong>${escapeHtml(runProjectSummary(run))}</strong>
						</div>
						<div class="child-activity-head">
							<span>Activity</span>
							<strong>${escapeHtml(current)}</strong>
						</div>
						<div class="child-activity-body">
							${
								shareBuckets.length
									? `<div class="child-share-list">
											${shareBuckets
												.slice(0, 3)
												.map((bucket) => {
													const width = childBucketWidth(bucket, totalWall);

														return `
															<div class="child-bucket is-share" data-render-key="${escapeHtml(childBucketRenderKey(bucket))}" data-duration="wall-share">
																<span class="child-bucket-name">${escapeHtml(childBucketDisplayName(bucket))}</span>
																<span class="child-bucket-bar" aria-hidden="true" style="--bucket-width: ${width}%"><span></span></span>
																<span class="child-bucket-value">${escapeHtml(childBucketShareLabel(bucket, totalWall))}</span>
															</div>
													`;
												})
												.join("")}
										</div>`
									: ""
							}
							${
								diagnosticBuckets.length
									? `<div class="child-diagnostic-grid">
											${diagnosticBuckets
												.slice(0, 4)
												.map((bucket) => {
													const eventOnly = childBucketIsEventOnly(bucket);
													const bucketClass = eventOnly
														? "child-bucket is-event-only"
														: "child-bucket is-diagnostic";
													const bucketState = eventOnly
														? ' data-duration="event-diagnostics"'
														: ' data-duration="diagnostic"';
													const signalSummary = childBucketDiagnosticSummary(bucket);

													return `
														<div class="${bucketClass}" data-render-key="${escapeHtml(childBucketRenderKey(bucket))}"${bucketState}>
															<span class="child-bucket-name">${escapeHtml(displayToken(bucket.name))}</span>
															<span class="child-bucket-signals" aria-label="${escapeHtml(signalSummary)}">
																${renderChildBucketDiagnosticSignals(bucket)}
															</span>
														</div>
													`;
												})
												.join("")}
										</div>`
									: ""
							}
							${
								contextRows.length
									? `<div class="child-context-group" aria-label="Context lifecycle metrics">
											${contextRows.join("")}
										</div>`
									: ""
							}
						</div>
					</div>
				`;
			}

			function childAgentDebugSummary(summary) {
				if (!summary) {
					return "none";
				}

				const buckets = childAgentBuckets(summary)
					.slice(0, 5)
					.map((bucket) => `${displayToken(bucket.name)} ${childBucketDuration(bucket)}`)
					.join(", ");

				return `current ${childAgentCurrentSummary(summary) || "none"}; buckets ${buckets || "none"}`;
			}

			function childAgentContextSummary(summary) {
				if (!summary) {
					return "none";
				}

				const latestInput = formatCompactCount(summary.input_tokens_current);
				const peakInput = formatCompactCount(summary.input_tokens_max);
				const peakSummary = childAgentInputWindowsMatch(summary)
					? ""
					: `, ${peakInput} peak window`;

				return `input ${latestInput} current window${peakSummary}, cumulative input ${formatCompactCount(summary.input_tokens_cumulative)}, max output ${formatLargestOutputValue(summary.largest_tool_output_bytes)}`;
			}

			function childAgentLargeOutputSummary(summary) {
				const warnings = childAgentLargeOutputWarnings(summary);

				return warnings.length ? warnings.join(" | ") : "none";
			}

			function recentRunTitle(run) {
				if (run.current_operation && run.current_operation !== "idle") {
					return displayToken(run.current_operation);
				}
				return displayToken(run.run_phase || run.phase || run.status);
			}

	function recentRunSummary(run, lane = null) {
		if ((lane?.attempt_count ?? 1) > 1) {
			return `Latest run ${displayToken(run.status || run.run_phase || run.phase)}; lifecycle cost is grouped by lifecycle bucket.`;
		}
				if (isSuccessfulTerminalRun(run)) {
					return "Finished; no current lane.";
				}
				if (run.status === "interrupted") {
					return "Stopped before completion; replaced after a later success.";
				}
				if (run.status === "terminated") {
					return "Terminated before completion; review may be needed.";
				}
				if (run.status === "failed") {
					return "Failed attempt kept for diagnosis.";
				}
				return "Earlier attempt retained for this session.";
			}

			function capturedHistoryFacts(run) {
				const facts = [];

				if (run.thread_id || run.thread_status) {
					facts.push(["Codex thread", runThreadSummary(run)]);
				}
				if (run.last_event_type || run.last_event_at) {
					facts.push([COPY.protocolEvent, protocolEventSummary(run)]);
				}
				if (run.branch_name) {
					facts.push(["Branch", run.branch_name]);
				}
				if (run.worktree_path) {
					facts.push(["Worktree", run.worktree_path]);
				}
					if (run.effective_model || run.effective_model_provider) {
						facts.push(["Model", runModelSummary(run)]);
					}
					if (codexAccount(run)) {
						facts.push(["Account", codexAccountHistorySummary(codexAccount(run))]);
					}
					if (run.next_retry_at) {
						facts.push(["Next retry", formatTimestamp(run.next_retry_at)]);
					}

				return facts;
			}

			function renderCapturedHistoryFacts(run) {
				const facts = capturedHistoryFacts(run);

				if (!facts.length) {
					return "";
				}

				return `
					<div class="grid">
						${facts.map(([label, value]) => field(label, value)).join("")}
					</div>
				`;
			}

			function historyLedgerFacts(lane) {
				const outcome = historyLedgerOutcome(lane);
				const facts = [];

				if (!historyLedgerWasLoaded(outcome)) {
					return facts;
				}

				if (outcome.pr_url) {
					facts.push(["PR", outcome.pr_url]);
				}
				if (outcome.commit_sha) {
					facts.push(["Commit", outcome.commit_sha]);
				}
				if (outcome.branch) {
					facts.push(["Branch", outcome.branch]);
				}
				if (outcome.needs_attention_reason) {
					facts.push(["Attention", outcome.needs_attention_reason]);
				}

				return facts;
			}

			function renderHistoryLedgerFacts(lane) {
				const facts = historyLedgerFacts(lane);

				if (!facts.length) {
					return "";
				}

				return `
					<div class="grid">
						${facts.map(([label, value]) => field(label, value)).join("")}
					</div>
				`;
			}

			function historyLifecycleFacts(lane) {
				const metrics = historyLaneLifecycleMetrics(lane);
				const facts = [
					["Lifecycle tokens", historyLifecycleTokenSummary(metrics)],
					["Lifecycle activity", formatDuration(lifecycleNumber(metrics.wall_seconds))],
					["Tool calls", formatCompactCount(metrics.tool_call_count)],
					["Captured attempts", historyLifecycleCaptureSummary(metrics)],
				];

				if (metrics.largest_tool_output_bytes != null) {
					facts.push([
						"Largest output",
						formatLargestOutputValue(metrics.largest_tool_output_bytes),
					]);
				}

				return facts;
			}

			function renderHistoryLifecycleFacts(lane) {
				const facts = historyLifecycleFacts(lane);

				if (!facts.length) {
					return "";
				}

				return `
					<div class="grid">
						${facts.map(([label, value]) => field(label, value)).join("")}
					</div>
				`;
			}

			function runHealthText(run) {
				if (runTelemetryMissingNeedsAttention(run)) {
					if (runHasChildAgentActivity(run)) {
						return "metadata_pending";
					}
					return "telemetry_missing";
				}
				if (runTelemetryMissing(run)) {
					if (runHasChildAgentActivity(run)) {
						return "metadata_pending";
					}
					return "starting_telemetry";
				}
				if (runStoppedProcessNeedsAttention(run)) {
					return "needs_attention";
				}
				if (runProcessStoppedWhileActive(run)) {
					return runPhaseLabel(run);
				}
				if (run.suspected_stall) {
					return "needs_attention";
				}
				if (run.interactive_requested) {
					return "input_requested";
				}
				if (run.continuation_pending) {
					return "continuation_pending";
				}
				if (runWaitReasonShowsExecutionProgress(run)) {
					return displayToken(run.wait_reason);
				}
				if (run.wait_reason) {
					return displayToken(run.wait_reason);
				}
				if (!run.run_lease && runCountsAsRunning(run)) {
					return "live_no_queue_lease";
				}
				if (run.thread_status) {
					return displayToken(run.thread_status);
				}
				return displayToken(run.status || run.run_phase || run.phase);
			}

			function snapshotIsIdle(snapshot) {
				if (!snapshot) {
					return false;
				}

				return (
					snapshotCurrentLaneCards(snapshot).length === 0 &&
					(snapshot.recent_runs?.length ?? 0) === 0 &&
					(snapshot.worktrees?.length ?? 0) === 0 &&
					(snapshot.post_review_lanes?.length ?? 0) === 0
				);
			}

			function connectorBackoffs(snapshot) {
				return Array.isArray(snapshot?.connector_backoffs) ? snapshot.connector_backoffs : [];
			}

			function hasConnectorBackoff(snapshot) {
				return connectorBackoffs(snapshot).length > 0 || (snapshot?.warnings ?? []).includes("tracker_rate_limited");
			}

			function connectorBackoffNotice(backoff) {
				const project = backoff.project_id || "project";
				const connector = displayToken(backoff.connector || "tracker");
				const phase = displayToken(backoff.sync_phase || "external sync");
				const quota = displayToken(backoff.quota_class || "api quota");
				const retryAfter = backoff.retry_after_seconds == null ? "unknown" : formatDuration(backoff.retry_after_seconds);
				const resetAt = formatTimestamp(backoff.reset_at);
				const nextAction = backoff.next_action || "Monitor local lanes.";

				return {
					tone: "warning",
					title: `Sync backoff · ${project}`,
					copy: `${connector} ${phase} paused by ${quota}. Retry in ${retryAfter} at ${resetAt}. ${nextAction}`,
				};
			}

			function summarizeReadiness(snapshotError, snapshot) {
				const warnings = snapshot?.warnings ?? [];
				const trackerBackoff = hasConnectorBackoff(snapshot);

				if (!dashboardStreamState.supported) {
					return {
						tone: "danger",
						label: "WebSocket unavailable",
						copy: "This browser cannot open the dashboard WebSocket.",
					};
				}

				if (dashboardStreamState.error) {
					return {
						tone: "danger",
						label: "WebSocket disconnected",
						copy: "Dashboard stream disconnected; reconnecting.",
					};
				}

				if (snapshot) {
					if (trackerBackoff && !snapshotError) {
						return {
							tone: "warning",
							label: "Tracker sync paused",
							copy: "Serving local state; Linear sync is paused.",
						};
					}

					return {
						tone: snapshotError || warnings.length ? "warning" : "success",
						label: snapshotError || warnings.length ? "State degraded" : "Snapshot ready",
						copy: snapshotError
							? "WebSocket did not deliver a usable snapshot."
							: warnings.length
								? `warnings: ${warnings.map(displayToken).join(", ")}`
								: "Fresh operator snapshot published.",
					};
				}

				return {
					tone: "warning",
					label: "No snapshot",
					copy: dashboardStreamState.connected
						? "WebSocket connected; waiting for operator snapshot."
						: "Connecting to dashboard WebSocket.",
				};
			}

			function dashboardNotices(readiness, snapshotError, snapshot) {
				const notices = [];
				const warnings = snapshot?.warnings ?? [];
				const backoffs = connectorBackoffs(snapshot);

				if (readiness.tone === "danger") {
					notices.push({
						tone: "danger",
						title: readiness.label,
						copy: snapshotError
							? `${readiness.copy} Snapshot stream also failed: ${snapshotError}`
							: readiness.copy,
					});
				} else if (snapshotError) {
					notices.push({
						tone: "danger",
						title: "Snapshot stream",
						copy: snapshotError,
					});
				}

				if (
					readiness.tone === "warning" &&
					!snapshotError &&
					warnings.length === 0 &&
					backoffs.length === 0
				) {
					notices.push({
						tone: "warning",
						title: readiness.label,
						copy: readiness.copy,
					});
				}

				for (const backoff of backoffs) {
					notices.push(connectorBackoffNotice(backoff));
				}

				for (const warning of warnings) {
					if (warning === "tracker_rate_limited" && backoffs.length) {
						continue;
					}
					if (
						warning === "external_observer_status_skipped" &&
						backoffs.length &&
						warnings.includes("tracker_rate_limited")
					) {
						continue;
					}
					const message = warningNotice(warning, snapshot);
					notices.push({
						tone: message.tone,
						title: message.title,
						copy: message.copy,
					});
				}

				for (const accountNotice of codexAccountNotices(snapshot)) {
					notices.push(accountNotice);
				}

				for (const controlEvent of dashboardControlEvents) {
					notices.push({
						tone: controlEvent.accepted ? "warning" : "danger",
						title: controlEvent.accepted ? "Control accepted" : "Control failed",
						copy: `${dashboardControlActionLabel(controlEvent.action)}: ${controlEvent.message}`,
						ackKey: controlEvent.key,
					});
				}

				return notices;
			}

				function codexAccountHasNotice(account) {
					if (!account) {
						return false;
					}

					const status = String(account.status || "").toLowerCase();
					const note = codexAccountNote(account);
					return Boolean(
						codexAccountRefreshFailed(account) ||
							codexAccountNoteLooksError(note) ||
							status.includes("failed") ||
							status.includes("unusable"),
					);
				}

				function codexAccountNoticeTitle(account) {
					if (codexAccountRefreshFailed(account)) {
						return "Codex account token";
					}
					if (codexAccountNoteLooksError(codexAccountNote(account))) {
						return "Codex account usage";
					}

					return "Codex account";
				}

				function codexAccountNoticeCopy(account) {
					const note = codexAccountNote(account);
					const parts = [];
					const noteIncludesRefreshFailure = /refresh failed|token refresh failed/i.test(note);
					if (note && !codexAccountNoteLooksRoutine(note)) {
						parts.push(codexAccountPrivacyText(account, note));
					}
					if (codexAccountRefreshFailed(account) && !noteIncludesRefreshFailure) {
						parts.unshift(codexAccountTokenLabel(account.refresh_status));
					}
					if (!parts.length) {
						parts.push(codexAccountStatusLabel(account));
					}

					return `${codexAccountPrivacyLabel(account)}: ${parts.join("; ")}`;
				}

				function codexAccountNotices(snapshot) {
					const notices = [];
					const seen = new Set();
					for (const account of codexAccountPoolAccounts(snapshot)) {
						if (!codexAccountHasNotice(account)) {
							continue;
						}
						const notice = {
							tone: "danger",
							title: codexAccountNoticeTitle(account),
							copy: codexAccountNoticeCopy(account),
						};
						const key = `${notice.title}:${notice.copy}`;
						if (seen.has(key)) {
							continue;
						}
						seen.add(key);
						notices.push(notice);
					}

					return notices;
				}

				function warningDetailsFor(warning, snapshot) {
					return (snapshot?.warning_details ?? []).filter((detail) => detail?.warning === warning);
				}

				function warningNotice(warning, snapshot) {
					const details = warningDetailsFor(warning, snapshot);
					if (warning === "worktree_hygiene_unavailable" && details.length) {
						return {
							tone: "warning",
							title: "Worktree hygiene unavailable",
							copy: details.map(worktreeHygieneWarningCopy).join(" "),
						};
					}

					return {
						tone: "warning",
						title: "Snapshot warning",
						copy: displayToken(warning),
					};
				}

				function worktreeHygieneWarningCopy(detail) {
					const project = detail.project_id || "project";
					const repo = detail.repo_root ? ` Repo: ${detail.repo_root}.` : "";
					const reason = detail.reason || "Worktree hygiene scan failed.";
					const nextAction = detail.next_action ? ` ${detail.next_action}` : "";

					return `${project}: ${reason}.${repo}${nextAction}`;
				}

			function renderNoticeDock(notices) {
				const hasNotices = notices.length > 0;
				nodes.noticeDock.classList.toggle("visible", hasNotices);
				nodes.noticeDock.setAttribute("aria-hidden", hasNotices ? "false" : "true");

				if (!hasNotices) {
					nodes.noticeDock.removeAttribute("open");
					delete nodes.noticeDock.dataset.tone;
					nodes.noticeCount.textContent = "0";
					nodes.noticeLabel.textContent = "notices";
					nodes.noticeList.innerHTML = "";

					return;
				}

				const tone = notices.some((notice) => notice.tone === "danger") ? "danger" : "warning";
				const dangerCount = notices.filter((notice) => notice.tone === "danger").length;
				nodes.noticeDock.dataset.tone = tone;
				nodes.noticeCount.textContent = String(notices.length);
				nodes.noticeLabel.textContent =
					dangerCount > 0
						? pluralLabel(notices.length, "alert")
						: pluralLabel(notices.length, "warning");
				nodes.noticeList.innerHTML = notices
					.map(
						(notice) => `
							<article class="notice-item ${notice.tone}">
								<strong>${escapeHtml(notice.title)}</strong>
								<p>${escapeHtml(notice.copy)}</p>
								${notice.ackKey ? `<button class="control-button" type="button" data-notice-ack="${escapeHtml(notice.ackKey)}">Ack</button>` : ""}
							</article>
						`,
					)
					.join("");
			}
