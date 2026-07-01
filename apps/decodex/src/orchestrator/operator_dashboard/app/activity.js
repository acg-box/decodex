			function recentRunTitle(run) {
				if (run.current_operation && run.current_operation !== "idle") {
					return displayToken(run.current_operation);
				}
				return displayToken(run.run_phase || run.phase || run.status);
			}

	function recentRunSummary(run, lane = null) {
		if ((lane?.attempt_count ?? 1) > 1) {
			return `Latest run ${displayToken(run.status || run.run_phase || run.phase)}; lifecycle cost is grouped by lifecycle bucket.`;
		}
				if (isSuccessfulTerminalRun(run)) {
					return "Finished; no current lane.";
				}
				if (run.status === "interrupted") {
					return "Stopped before completion; replaced after a later success.";
				}
				if (run.status === "terminated") {
					return "Terminated before completion; review may be needed.";
				}
				if (run.status === "failed") {
					return "Failed attempt kept for diagnosis.";
				}
				return "Earlier attempt retained for this session.";
			}

			function runHealthText(run) {
				if (runTelemetryMissingNeedsAttention(run)) {
					if (runHasChildAgentActivity(run)) {
						return "metadata_pending";
					}
					return "telemetry_missing";
				}
				if (runTelemetryMissing(run)) {
					if (runHasChildAgentActivity(run)) {
						return "metadata_pending";
					}
					return "starting_telemetry";
				}
				if (runStoppedProcessNeedsAttention(run)) {
					return "needs_attention";
				}
				if (runProcessStoppedWhileActive(run)) {
					return runPhaseLabel(run);
				}
				if (run.suspected_stall) {
					return "needs_attention";
				}
				if (run.interactive_requested) {
					return "input_requested";
				}
				if (run.continuation_pending) {
					return "continuation_pending";
				}
				if (runWaitReasonShowsExecutionProgress(run)) {
					return displayToken(run.wait_reason);
				}
				if (run.wait_reason) {
					return displayToken(run.wait_reason);
				}
				if (!run.run_lease && runCountsAsRunning(run)) {
					return "live_no_queue_lease";
				}
				if (run.thread_status) {
					return displayToken(run.thread_status);
				}
				return displayToken(run.status || run.run_phase || run.phase);
			}
