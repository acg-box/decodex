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
