			function issueDisplayKey(item) {
				if (!item) {
					return "unknown";
				}
				if (item.issue_identifier) {
					return item.issue_identifier;
				}
				const runIssueIdentifier = issueIdentifierFromRunId(item.run_id);
				if (runIssueIdentifier) {
					return runIssueIdentifier;
				}
				for (const value of [item.worktree_path, item.branch_name]) {
					const identifier = issueIdentifierInText(value);
					if (identifier) {
						return identifier;
					}
				}

				return item.issue_id || "unknown";
			}

			function canonicalIssueIdentityKey(value) {
				const key = String(value ?? "").trim();

				if (!key || key.toLowerCase() === "unknown") {
					return "";
				}

				return key.toUpperCase();
			}

			function issueIdentityKeys(item) {
				if (!item) {
					return [];
				}

				const keys = [item.issue_id, item.issue_identifier, issueDisplayKey(item)]
					.map(canonicalIssueIdentityKey)
					.filter(Boolean);

				return [...new Set(keys)];
			}

			function issueMatchesKeySet(item, keySet) {
				return issueIdentityKeys(item).some((key) => keySet.has(key));
			}

			function currentLaneFreshness(run) {
				if (run?.last_run_activity_at) {
					return {
						label: "Lane activity",
						source: "last_run_activity_at",
						sourceLabel: "live activity",
						timestamp: run.last_run_activity_at,
					};
				}
				if (run?.last_progress_at) {
					return {
						label: "Last progress",
						source: "last_progress_at",
						sourceLabel: "progress",
						timestamp: run.last_progress_at,
					};
				}
				if (run?.last_protocol_activity_at) {
					return {
						label: "Protocol activity",
						source: "last_protocol_activity_at",
						sourceLabel: "protocol activity",
						timestamp: run.last_protocol_activity_at,
					};
				}
				return {
					label: "Lane activity",
					source: "none",
					sourceLabel: "activity",
					timestamp: null,
				};
			}

			function currentLaneFreshnessFact(run, formatter = formatTimestamp) {
				const freshness = currentLaneFreshness(run);
				return [
					freshness.label,
					freshness.timestamp ? formatter(freshness.timestamp) : "not captured",
				];
			}

			function historyRunTimingFacts(run) {
				return [
					["Updated", formatTimestampCompact(run.updated_at)],
					["Attempt", String(run.attempt_number ?? "none")],
					["Status", displayToken(run.status)],
					["Events", String(run.event_count ?? 0)],
				];
			}

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

			function currentLaneTelemetryFacts(run) {
				const facts = [];
				const freshness = currentLaneFreshness(run);
				const focus = protocolActivityFocus(run);

				facts.push(["run phase", displayToken(run.run_phase || run.phase || run.status)]);
				if (freshness.timestamp) {
					facts.push([freshness.sourceLabel, formatRelativeTimestamp(freshness.timestamp)]);
				}
				if (focus) {
					facts.push(["focus", detailLabel(focus)]);
				}
				if (run.idle_for_seconds != null) {
					facts.push(["lane idle", formatDuration(run.idle_for_seconds)]);
				}
				if (run.protocol_idle_for_seconds != null) {
					facts.push(["agent idle", formatDuration(run.protocol_idle_for_seconds)]);
				}
				return facts;
			}

			function currentLaneReadbackValues(run) {
				return [
					run?.run_phase || run?.phase || run?.status,
					run?.current_operation,
					run?.active_goal_phase,
					run?.public_progress_phase,
				].filter(Boolean);
			}

			function currentLaneVisibleSummary(card, run) {
				const summary = String(card?.detail || "").trim();
				if (!summary) {
					return "";
				}

				return currentLaneReadbackValues(run).some((value) => displayTextRepeats(summary, value))
					? ""
					: summary;
			}

			function renderRunMetaFact(label, value, valueClass = "", title = "") {
				const classAttribute = valueClass ? ` class="${escapeHtml(valueClass)}"` : "";
				const titleAttribute = title ? ` title="${escapeHtml(title)}"` : "";

				return `
					<span class="run-meta-item">
						<span class="run-meta-label">${escapeHtml(detailLabel(label))}</span>
						<strong${classAttribute}${titleAttribute}>${escapeHtml(value)}</strong>
					</span>
				`;
			}

			function renderRunTelemetryMetaItems(run) {
				const facts = currentLaneTelemetryFacts(run);

				if (!facts.length) {
					return "";
				}

				return facts.map(([label, value]) => renderRunMetaFact(label, value)).join("");
			}

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
