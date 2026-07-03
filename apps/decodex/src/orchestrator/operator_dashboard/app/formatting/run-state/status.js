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
