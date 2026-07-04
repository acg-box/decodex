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
