
				function currentLaneLifecycleMetrics(run, summary = childAgentActivity(run)) {
					const provided = run?.lifecycle_metrics || {};
					const providedPhases = Array.isArray(provided.phases)
						? provided.phases.map(normalizeLifecyclePhaseMetrics)
						: [];
					const fallbackCaptured = summary ? 1 : 0;
					const attemptCount = lifecycleNumber(provided.attempt_count ?? (summary ? 1 : 0));
					const capturedAttemptCount = lifecycleNumber(
						provided.captured_attempt_count ?? fallbackCaptured,
					);
					const buckets = Array.isArray(provided.buckets) && provided.buckets.length
						? provided.buckets
						: childAgentBuckets(summary);
					const metrics = {
						...provided,
						attempt_count: attemptCount,
						run_count: lifecycleNumber(provided.run_count ?? attemptCount),
						recorded_attempt_count: lifecycleNumber(provided.recorded_attempt_count),
						recovered_attempt_count: lifecycleNumber(provided.recovered_attempt_count),
						current_snapshot_attempt_count: lifecycleNumber(provided.current_snapshot_attempt_count),
						captured_attempt_count: capturedAttemptCount,
						missing_attempt_count: lifecycleNumber(
							provided.missing_attempt_count ??
								Math.max(0, attemptCount - capturedAttemptCount),
						),
						protocol_event_count: lifecycleNumber(provided.protocol_event_count ?? run?.event_count),
						child_event_count: lifecycleNumber(provided.child_event_count ?? summary?.event_count),
						wall_seconds: lifecycleNumber(provided.wall_seconds ?? summary?.wall_seconds),
						tool_call_count: lifecycleNumber(provided.tool_call_count ?? summary?.tool_call_count),
						input_tokens_current: provided.input_tokens_current ?? summary?.input_tokens_current ?? null,
						input_tokens_peak: provided.input_tokens_peak ?? summary?.input_tokens_max ?? null,
						input_tokens_cumulative: lifecycleNumber(
							provided.input_tokens_cumulative ?? summary?.input_tokens_cumulative,
						),
						output_tokens_cumulative: lifecycleNumber(
							provided.output_tokens_cumulative ?? summary?.output_tokens_cumulative,
						),
						largest_tool_output_bytes:
							provided.largest_tool_output_bytes ?? summary?.largest_tool_output_bytes ?? null,
						largest_tool_output_tool:
							provided.largest_tool_output_tool ?? summary?.largest_tool_output_tool ?? null,
						large_output_warnings: Array.isArray(provided.large_output_warnings)
							? provided.large_output_warnings
							: childAgentLargeOutputWarnings(summary),
						buckets,
						phases: providedPhases,
					};

					if (!metrics.phases.length && summary) {
						const phase = fallbackLifecyclePhaseForRun(run);
						metrics.phases = [
							normalizeLifecyclePhaseMetrics({
								phase: phase.key,
								label: phase.label,
								attempt_count: attemptCount || 1,
								run_count: 1,
								captured_attempt_count: 1,
								missing_attempt_count: 0,
								protocol_event_count: lifecycleNumber(run?.event_count),
								child_event_count: lifecycleNumber(summary.event_count),
								wall_seconds: lifecycleNumber(summary.wall_seconds),
								tool_call_count: lifecycleNumber(summary.tool_call_count),
								input_tokens_current: summary.input_tokens_current ?? null,
								input_tokens_peak: summary.input_tokens_max ?? null,
								input_tokens_cumulative: lifecycleNumber(summary.input_tokens_cumulative),
								output_tokens_cumulative: lifecycleNumber(summary.output_tokens_cumulative),
								largest_tool_output_bytes: summary.largest_tool_output_bytes ?? null,
								largest_tool_output_tool: summary.largest_tool_output_tool ?? null,
								large_output_warnings: childAgentLargeOutputWarnings(summary),
								buckets: childAgentBuckets(summary),
							}),
						];
					}

					return metrics;
				}
