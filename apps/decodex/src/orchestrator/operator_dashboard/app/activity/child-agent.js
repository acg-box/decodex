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

