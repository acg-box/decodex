
			function historyLaneTimingFacts(lane) {
				const outcome = historyLedgerOutcome(lane);
				const metrics = historyLaneLifecycleMetrics(lane);
				const modelSeconds = lifecycleBucketSeconds(metrics, "Model");
				const activitySeconds = modelSeconds > 0
					? modelSeconds
					: lifecycleNumber(metrics.wall_seconds);
				const activityLabel = modelSeconds > 0 ? "Inference" : "Activity";

				if (!historyLedgerHasRecords(outcome)) {
					const run = lane.latest_run;

					return [
						["Updated", formatTimestampCompact(run.updated_at)],
						["Attempts", String(metrics.attempt_count || lane.attempt_count || 1)],
						[activityLabel, formatDuration(activitySeconds)],
						["Tokens", historyLifecycleTokenSummary(metrics)],
						["Events", formatCompactCount(metrics.protocol_event_count)],
					];
				}

				return [
					["Finished", formatTimestampCompact(outcome.lifecycle_finished_at || outcome.final_event_at)],
					["Elapsed", formatDuration(outcome.lifecycle_elapsed_seconds)],
					["Attempts", String(metrics.attempt_count || lane.attempt_count || 1)],
					[activityLabel, formatDuration(activitySeconds)],
					["Tokens", historyLifecycleTokenSummary(metrics)],
					["Events", formatCompactCount(metrics.protocol_event_count || outcome.record_count || 0)],
				];
			}

			function renderHistoryTimingStrip(lane) {
				return `
					<div class="timing-strip" aria-label="Run outcome timing">
						${historyLaneTimingFacts(lane)
							.map(
								([label, value]) => `
									<div class="timing-cell">
										<div class="timing-label">${escapeHtml(label)}</div>
										<div class="timing-value">${escapeHtml(value)}</div>
									</div>
								`,
							)
							.join("")}
					</div>
				`;
			}

			function capturedHistoryFacts(run) {
				const facts = [];

				if (run.thread_id || run.thread_status) {
					facts.push(["Codex thread", runThreadSummary(run)]);
				}
				if (run.last_event_type || run.last_event_at) {
					facts.push([COPY.protocolEvent, protocolEventSummary(run)]);
				}
				if (run.branch_name) {
					facts.push(["Branch", run.branch_name]);
				}
				if (run.worktree_path) {
					facts.push(["Worktree", run.worktree_path]);
				}
					if (run.effective_model || run.effective_model_provider) {
						facts.push(["Model", runModelSummary(run)]);
					}
					if (codexAccount(run)) {
						facts.push(["Account", codexAccountHistorySummary(codexAccount(run))]);
					}
					if (run.next_retry_at) {
						facts.push(["Next retry", formatTimestamp(run.next_retry_at)]);
					}

				return facts;
			}

			function renderCapturedHistoryFacts(run) {
				const facts = capturedHistoryFacts(run);

				if (!facts.length) {
					return "";
				}

				return `
					<div class="grid">
						${facts.map(([label, value]) => field(label, value)).join("")}
					</div>
				`;
			}

			function historyLedgerFacts(lane) {
				const outcome = historyLedgerOutcome(lane);
				const facts = [];

				if (!historyLedgerWasLoaded(outcome)) {
					return facts;
				}

				if (outcome.pr_url) {
					facts.push(["PR", outcome.pr_url]);
				}
				if (outcome.commit_sha) {
					facts.push(["Commit", outcome.commit_sha]);
				}
				if (outcome.branch) {
					facts.push(["Branch", outcome.branch]);
				}
				if (outcome.needs_attention_reason) {
					facts.push(["Attention", outcome.needs_attention_reason]);
				}

				return facts;
			}

			function renderHistoryLedgerFacts(lane) {
				const facts = historyLedgerFacts(lane);

				if (!facts.length) {
					return "";
				}

				return `
					<div class="grid">
						${facts.map(([label, value]) => field(label, value)).join("")}
					</div>
				`;
			}

			function historyLifecycleFacts(lane) {
				const metrics = historyLaneLifecycleMetrics(lane);
				const facts = [
					["Lifecycle tokens", historyLifecycleTokenSummary(metrics)],
					["Lifecycle activity", formatDuration(lifecycleNumber(metrics.wall_seconds))],
					["Tool calls", formatCompactCount(metrics.tool_call_count)],
					["Captured attempts", historyLifecycleCaptureSummary(metrics)],
				];

				if (metrics.largest_tool_output_bytes != null) {
					facts.push([
						"Largest output",
						formatLargestOutputValue(metrics.largest_tool_output_bytes),
					]);
				}

				return facts;
			}

			function renderHistoryLifecycleFacts(lane) {
				const facts = historyLifecycleFacts(lane);

				if (!facts.length) {
					return "";
				}

				return `
					<div class="grid">
						${facts.map(([label, value]) => field(label, value)).join("")}
					</div>
				`;
			}

