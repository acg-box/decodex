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
