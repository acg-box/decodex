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
