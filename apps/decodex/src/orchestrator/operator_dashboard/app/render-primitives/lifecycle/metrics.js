
			function lifecycleNumber(value) {
				const number = Number(value ?? 0);

				return Number.isFinite(number) ? Math.max(0, number) : 0;
			}

			function historyLaneLifecycleMetrics(lane) {
				const attempts = Array.isArray(lane?.attempts) && lane.attempts.length
					? lane.attempts
					: lane?.latest_run
						? [lane.latest_run]
						: [];
				const provided = lane?.lifecycle_metrics || {};
				const summaries = attempts.map(childAgentActivity).filter(Boolean);
				const captured = lifecycleNumber(
					provided.captured_attempt_count ?? summaries.length,
				);
				const attemptCount = lifecycleNumber(
					provided.attempt_count ?? lane?.attempt_count ?? attempts.length,
				);

				return {
					...provided,
					attempt_count: attemptCount,
					captured_attempt_count: captured,
					missing_attempt_count: lifecycleNumber(
						provided.missing_attempt_count ?? Math.max(0, attemptCount - captured),
					),
					protocol_event_count: lifecycleNumber(
						provided.protocol_event_count ??
							attempts.reduce((total, run) => total + lifecycleNumber(run?.event_count), 0),
					),
					child_event_count: lifecycleNumber(
						provided.child_event_count ??
							summaries.reduce((total, summary) => total + lifecycleNumber(summary.event_count), 0),
					),
					wall_seconds: lifecycleNumber(
						provided.wall_seconds ??
							summaries.reduce((total, summary) => total + lifecycleNumber(summary.wall_seconds), 0),
					),
					tool_call_count: lifecycleNumber(
						provided.tool_call_count ??
							summaries.reduce((total, summary) => total + lifecycleNumber(summary.tool_call_count), 0),
					),
					input_tokens_cumulative: lifecycleNumber(
						provided.input_tokens_cumulative ??
							summaries.reduce(
								(total, summary) => total + lifecycleNumber(summary.input_tokens_cumulative),
								0,
							),
					),
					output_tokens_cumulative: lifecycleNumber(
						provided.output_tokens_cumulative ??
							summaries.reduce(
								(total, summary) => total + lifecycleNumber(summary.output_tokens_cumulative),
								0,
							),
					),
					buckets: Array.isArray(provided.buckets) ? provided.buckets : [],
					phases: Array.isArray(provided.phases)
						? provided.phases.map(normalizeLifecyclePhaseMetrics)
						: [],
				};
			}

			function normalizeLifecyclePhaseMetrics(phase) {
				return {
					...phase,
					phase: phase?.phase || "unknown",
					label: phase?.label || displayToken(phase?.phase || "unknown"),
					attempt_count: lifecycleNumber(phase?.attempt_count),
					recorded_attempt_count: lifecycleNumber(phase?.recorded_attempt_count),
					recovered_attempt_count: lifecycleNumber(phase?.recovered_attempt_count),
					current_snapshot_attempt_count: lifecycleNumber(phase?.current_snapshot_attempt_count),
					captured_attempt_count: lifecycleNumber(phase?.captured_attempt_count),
					missing_attempt_count: lifecycleNumber(phase?.missing_attempt_count),
					protocol_event_count: lifecycleNumber(phase?.protocol_event_count),
					child_event_count: lifecycleNumber(phase?.child_event_count),
					wall_seconds: lifecycleNumber(phase?.wall_seconds),
					tool_call_count: lifecycleNumber(phase?.tool_call_count),
					input_tokens_cumulative: lifecycleNumber(phase?.input_tokens_cumulative),
					output_tokens_cumulative: lifecycleNumber(phase?.output_tokens_cumulative),
					buckets: Array.isArray(phase?.buckets) ? phase.buckets : [],
				};
			}

			function lifecycleBucket(metrics, bucketName) {
				return (metrics?.buckets || []).find(
					(candidate) => String(candidate?.name || "").toLowerCase() === bucketName.toLowerCase(),
				);
			}

			function lifecycleBucketSeconds(metrics, bucketName) {
				const bucket = lifecycleBucket(metrics, bucketName);

				return lifecycleNumber(bucket?.wall_seconds);
			}

			function lifecycleWallSeconds(metrics) {
				const buckets = Array.isArray(metrics?.buckets) ? metrics.buckets : [];
				return Math.max(
					1,
					lifecycleNumber(metrics?.wall_seconds),
					buckets.reduce((total, bucket) => total + lifecycleNumber(bucket?.wall_seconds), 0),
				);
			}

			function formatRuntimeShare(seconds, totalSeconds) {
				const elapsed = lifecycleNumber(seconds);
				const total = Math.max(1, lifecycleNumber(totalSeconds), elapsed);
				if (elapsed <= 0) {
					return { percent: "-", elapsed: "-", total: "-", ratio: "-", text: "-" };
				}

				const percent = Math.round((elapsed / total) * 100);
				const elapsedText = formatDuration(elapsed);
				const totalText = formatDuration(total);
				const ratio = `${compactRuntimeDuration(elapsedText)}/${compactRuntimeDuration(totalText)}`;
				return {
					percent: `${percent}%`,
					elapsed: elapsedText,
					total: totalText,
					ratio,
					text: `${ratio}(${percent}%)`,
				};
			}

			function compactRuntimeDuration(value) {
				return String(value || "-").replaceAll(" ", "");
			}

			function historyLifecycleTokenSummary(metrics) {
				const input = lifecycleNumber(metrics?.input_tokens_cumulative);
				const output = lifecycleNumber(metrics?.output_tokens_cumulative);

				if (input === 0 && output === 0) {
					return "not captured";
				}

				return `in ${formatCompactCount(input)} / out ${formatCompactCount(output)}`;
			}

			function historyLifecycleCaptureSummary(metrics) {
				const captured = lifecycleNumber(metrics?.captured_attempt_count);
				const attempts = lifecycleNumber(metrics?.attempt_count);
				const missing = lifecycleNumber(metrics?.missing_attempt_count);

				return missing > 0 ? `${captured}/${attempts} captured · ${missing} missing` : `${captured}/${attempts} captured`;
			}
