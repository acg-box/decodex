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
