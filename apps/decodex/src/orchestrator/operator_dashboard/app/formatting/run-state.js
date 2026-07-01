			function capturedValue(value) {
				return value || "not captured";
			}

			function numericSeconds(value) {
				if (value == null) {
					return 0;
				}

				const parsed = Number(value);
				return Number.isFinite(parsed) ? parsed : 0;
			}

			function runHasProtocolTelemetry(run) {
				return Boolean(
					run.thread_id ||
						run.turn_id ||
						run.last_event_type ||
						run.last_event_at ||
						run.last_protocol_activity_at ||
						run.effective_model ||
						run.effective_model_provider ||
						run.effective_cwd ||
						run.process_id != null ||
						Number(run.event_count ?? 0) > 0,
				);
			}

			function runTelemetryMissing(run) {
				return run.status === "running" && run.phase === "executing" && !runHasProtocolTelemetry(run);
			}

			function runHasChildAgentActivity(run) {
				const activity = childAgentActivity(run);

				return Boolean(
					activity?.current_bucket ||
						(activity?.buckets?.length ?? 0) > 0 ||
						Number(activity?.event_count ?? 0) > 0,
				);
			}

			function runTelemetryMissingNeedsAttention(run) {
				return runTelemetryMissing(run) && numericSeconds(run.idle_for_seconds) >= RUN_ATTENTION_IDLE_SECONDS;
			}

			function runHasFreshExecution(run) {
				if (typeof run?.has_fresh_execution === "boolean") {
					return run.has_fresh_execution;
				}

				return (
					run?.thread_status === "active" ||
					(run?.thread_active_flags?.length ?? 0) > 0 ||
					["thread_active", "protocol_observed"].includes(run?.execution_liveness) ||
					run?.process_alive === true ||
					(run?.protocol_idle_for_seconds != null &&
						numericSeconds(run.protocol_idle_for_seconds) < RUN_STALE_NO_PROCESS_SECONDS)
				);
			}

			function runProcessStoppedWhileActive(run) {
				return (
					["starting", "running"].includes(run.status) &&
					run.phase === "executing" &&
					run.process_alive === false
				);
			}

			function runStoppedProcessNeedsAttention(run) {
				return runProcessStoppedWhileActive(run);
			}

				function runPhaseLabel(run) {
					if (runStoppedProcessNeedsAttention(run)) {
						return run.process_liveness_reason || "process_stopped";
					}

					return displayToken(run.run_phase || run.phase || run.status);
				}

			function runStaleWithoutKnownProcessAgeSeconds(run) {
				return Math.max(
					numericSeconds(run.idle_for_seconds),
					numericSeconds(run.protocol_idle_for_seconds),
				);
			}

			function runStaleWithoutKnownProcessNeedsAttention(run) {
				return (
					["starting", "running"].includes(run.status) &&
					run.phase === "executing" &&
					!run.wait_reason &&
					run.process_alive !== true &&
					!runHasFreshExecution(run) &&
					runStaleWithoutKnownProcessAgeSeconds(run) >= RUN_STALE_NO_PROCESS_SECONDS
				);
			}

			function runNeedsAttention(run) {
				if (typeof run?.needs_attention === "boolean") {
					return run.needs_attention;
				}

				return (
					run.suspected_stall ||
					run.phase === "stalled" ||
					runStoppedProcessNeedsAttention(run) ||
					runStaleWithoutKnownProcessNeedsAttention(run)
				);
			}

			function runCountsAsRunning(run) {
				if (typeof run?.counts_as_running === "boolean") {
					return run.counts_as_running;
				}

				return (
					["starting", "running"].includes(run.status) &&
					run.phase === "executing" &&
					run.process_alive !== false &&
					!runNeedsAttention(run)
				);
			}

			function runWaitReasonShowsExecutionProgress(run) {
				return (
					run.phase === "executing" &&
					["model_execution", "tool_execution", "protocol_activity"].includes(run.wait_reason)
				);
			}

			function toneForRun(run) {
				if (
					runNeedsAttention(run) ||
					runTelemetryMissingNeedsAttention(run) ||
					["stalled", "failed", "terminated", "interrupted"].includes(run.status)
				) {
					return "tone-blocked";
				}
				if (runTelemetryMissing(run) || runProcessStoppedWhileActive(run)) {
					return "tone-wait";
				}
				if (runWaitReasonShowsExecutionProgress(run)) {
					return "tone-run";
				}
				if (
					(run.wait_reason && !runWaitReasonShowsExecutionProgress(run)) ||
					run.phase === "retry_backoff" ||
					run.phase === "waiting_continuation"
				) {
					return "tone-wait";
				}
				if (run.status === "running" || run.phase === "executing") {
					return "tone-run";
				}
				if (run.status === "completed" || run.status === "merged" || run.status === "succeeded") {
					return "tone-land";
				}
				return "tone-muted";
			}

			function toneForLane(lane) {
				switch (lane.classification) {
					case "ready_to_land":
					case "continue":
						return "tone-land";
					case "wait_for_review":
						return "tone-review";
					case "cleanup_blocked":
						return "tone-wait";
					case "closeout_blocked":
					case "blocked":
						return "tone-blocked";
					case "needs_review_repair":
						return "tone-repair";
					default:
						return "tone-retained";
				}
			}

			function isPostReviewBlocker(lane) {
				if (lane?.shadowed_by_current_lane === true) {
					return false;
				}

				return ["blocked", "needs_review_repair", "closeout_blocked", "cleanup_blocked"].includes(
					lane.classification,
				);
			}

			function postReviewBlockerScope(lane) {
				if (lane.classification === "cleanup_blocked") {
					return "Cleanup";
				}
				if (lane.classification === "closeout_blocked") {
					return "Closeout";
				}
				return "Review";
			}

			function postReviewBlockerTitle(lane) {
				return displayToken(lane.classification);
			}

			function postReviewBlockerStatus(lane, blockerScope) {
				if (lane.review_decision && blockerScope === "Review") {
					return `review ${compactStateToken(lane.review_decision)}`;
				}

				return "needs_attention";
			}
