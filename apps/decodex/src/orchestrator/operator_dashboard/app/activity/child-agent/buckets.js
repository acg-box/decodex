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
