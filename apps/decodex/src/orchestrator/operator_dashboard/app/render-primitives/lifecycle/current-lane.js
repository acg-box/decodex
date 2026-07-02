
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

				function fallbackLifecyclePhaseForRun(run) {
					const status = String(run?.status || "").toLowerCase();
					const operation = String(run?.current_operation || "").toLowerCase();
					const reviewPhase = String(run?.loop_status?.review?.phase || "").toLowerCase();

					if (["cleanup_complete", "closeout", "closeout_pending", "landed"].includes(status)) {
						return { key: "closeout", label: "Closeout" };
					}
					if (
						["manual_attention", "manual_attention_pending", "needs_attention", "terminal_failure"].includes(status) ||
						String(run?.phase || "").toLowerCase() === "needs_attention"
					) {
						return { key: "manual_attention", label: "Manual attention" };
					}
					if (reviewPhase === "repair" || status === "review_repair_pending") {
						return { key: "review_repair", label: "Review repair" };
					}
					if (reviewPhase || status === "review_handoff_pending" || operation === "review_writeback") {
						return { key: "review", label: "Review" };
					}

					return { key: "development", label: "Development" };
				}

				function lifecycleMetricDurationFact(metrics) {
					const modelSeconds = lifecycleBucketSeconds(metrics, "Model");
					const activitySeconds = modelSeconds > 0 ? modelSeconds : lifecycleNumber(metrics?.wall_seconds);

					if (activitySeconds <= 0) {
						return null;
					}

					return [modelSeconds > 0 ? "inference" : "activity", formatDuration(activitySeconds)];
				}

				function lifecycleMetricFacts(metrics, { includeAttempts = false } = {}) {
					if (!metrics) {
						return [];
					}

					const facts = [];
					const durationFact = lifecycleMetricDurationFact(metrics);
					const tokenSummary = historyLifecycleTokenSummary(metrics);
					const attempts = lifecycleNumber(metrics.attempt_count);
					const captured = lifecycleNumber(metrics.captured_attempt_count);
					const missing = lifecycleNumber(metrics.missing_attempt_count);
					const childEvents = lifecycleNumber(metrics.child_event_count);
					const protocolEvents = lifecycleNumber(metrics.protocol_event_count);

					if (includeAttempts && (attempts > 1 || missing > 0)) {
						facts.push([
							"attempts",
							missing > 0 ? `${captured}/${attempts} captured` : formatCompactCount(attempts),
						]);
					}
					if (durationFact) {
						facts.push(durationFact);
					}
					if (tokenSummary !== "not captured") {
						facts.push(["tokens", tokenSummary]);
					}
					if (lifecycleNumber(metrics.tool_call_count) > 0) {
						facts.push(["tools", formatCompactCount(metrics.tool_call_count)]);
					}
					if (childEvents || protocolEvents) {
						facts.push([
							"events",
							childEvents && protocolEvents
								? `${formatCompactCount(childEvents)} child / ${formatCompactCount(protocolEvents)} protocol`
								: formatCompactCount(childEvents || protocolEvents),
						]);
					}
					if (metrics.largest_tool_output_bytes != null) {
						facts.push([
							"max output",
							formatLargestOutputValue(metrics.largest_tool_output_bytes),
						]);
					}

					return facts;
				}

				function lifecycleRecoveryDebugSummary(metrics) {
					if (!metrics) {
						return "none";
					}

					const recorded = lifecycleNumber(metrics.recorded_attempt_count);
					const recovered = lifecycleNumber(metrics.recovered_attempt_count);
					const currentSnapshot = lifecycleNumber(metrics.current_snapshot_attempt_count);
					const gaps = Array.isArray(metrics.recovery_gaps)
						? metrics.recovery_gaps.filter(Boolean)
						: [];
					const parts = [
						`recorded ${formatCompactCount(recorded)}`,
						`recovered ${formatCompactCount(recovered)}`,
						`current snapshot ${formatCompactCount(currentSnapshot)}`,
					];

					if (gaps.length) {
						parts.push(`gaps ${gaps.join(", ")}`);
					}

					return parts.join("; ");
				}

				function lifecycleEvidenceDebugSummary(metrics) {
					const attempts = Array.isArray(metrics?.attempt_evidence)
						? metrics.attempt_evidence.filter(Boolean)
						: [];

					if (!attempts.length) {
						return "none";
					}

					return attempts
						.map((attempt) => {
							const evidence = Array.isArray(attempt.evidence) && attempt.evidence.length
								? attempt.evidence.join(",")
								: "none";
							const gaps = Array.isArray(attempt.gaps) && attempt.gaps.length
								? attempt.gaps.join(",")
								: "none";

							return `${attempt.run_id || "unknown"}#${attempt.attempt_number || "?"} ${attempt.phase || "unknown"} ${attempt.source || "unknown"} evidence=${evidence} gaps=${gaps}`;
						})
						.join("; ");
				}
