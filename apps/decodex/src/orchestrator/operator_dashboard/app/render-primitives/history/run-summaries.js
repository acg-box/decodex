			function runThreadSummary(run) {
				if (run.thread_id) {
					return `${run.thread_id} (${run.thread_status || "unknown"})`;
				}
				return run.thread_status || "not captured";
			}

			function runThreadFlagSummary(run) {
				const flags = run.thread_active_flags ?? [];
				return flags.length ? flags.join(", ") : "none";
			}

			function runModelSummary(run) {
				const parts = [run.effective_model_provider, run.effective_model].filter(Boolean);
				return parts.length ? parts.join(" / ") : "not captured";
			}

			function runProcessSummary(run) {
				if (run.process_id == null) {
					return "not captured";
				}
				if (run.process_alive == null) {
					return `${run.process_id} (unknown)`;
				}
				const reason = run.process_alive
					? "process_alive"
					: run.process_liveness_reason || "process_stopped";
				return `${run.process_id} (${processLivenessReasonLabel(reason)})`;
			}

			function processLivenessReasonLabel(reason) {
				return displayToken(reason || "unknown");
			}

			function runExecutionLivenessSummary(run) {
				return displayToken(run.execution_liveness || "liveness_unknown");
			}

			function runOwnershipSummary(run) {
				return displayToken(run.ownership_state || (runCountsAsRunning(run) ? "leased_run" : "unknown"));
			}

			function runLivenessStateSummary(run) {
				return displayToken(run.liveness_state || "unknown");
			}

			function runPolicyStateSummary(run) {
				return displayToken(run.policy_state || "allowed");
			}

			function runTerminalizationSummary(run) {
				return displayToken(run.terminalization_state || "none");
			}

			function runLaneControlConditionsSummary(run) {
				const conditions = run.lane_control_conditions ?? [];
				return conditions.length ? conditions.map(displayToken).join(", ") : "none";
			}

			function runContinuationRecoverySummary(run) {
				const recovery = run.continuation_recovery;
				if (!recovery) {
					return "none";
				}

				const count = `${recovery.recovery_count ?? 0}/${recovery.automatic_continuation_limit ?? 0}`;
				const exceeded = recovery.budget_exceeded ? "budget exceeded" : "within budget";
				const message = recovery.source_error_message
					? `; ${recovery.source_error_message}`
					: "";

				return `${displayToken(recovery.state)} · ${displayToken(recovery.source_phase)} -> ${displayToken(recovery.next_phase)} · ${displayToken(recovery.source_error_class)} · ${count} · ${exceeded}${message}`;
			}

			function runQueueLeaseSummary(run) {
				const leaseState = run.queue_lease_state || (run.run_lease ? "held" : "not_held");

				if (leaseState === "held") {
					return "held";
				}

				return `${leaseState}; ${displayToken(run.execution_liveness || "liveness_unknown")}`;
			}

			function runApprovalSummary(run) {
				const policy = run.effective_approval_policy || "not captured";
				return run.effective_approvals_reviewer
					? `${policy} / ${run.effective_approvals_reviewer}`
					: policy;
			}

			function protocolEventSummary(run) {
				if (run.last_event_type && run.last_event_at) {
					return `${run.last_event_type} @ ${formatTimestamp(run.last_event_at)}`;
				}
				if (run.last_event_type) {
					return run.last_event_type;
				}
				if (run.last_event_at) {
					return formatTimestamp(run.last_event_at);
				}

				return "not captured";
			}

			function protocolActivity(run) {
				return run?.protocol_activity || null;
			}

			function protocolActivityWaitReason(run) {
				const activity = protocolActivity(run);
				if (activity?.waiting_reason) {
					return activity.waiting_reason;
				}
				if (run?.wait_reason) {
					return run.wait_reason;
				}

				return "";
			}

			function protocolActivityFocus(run) {
				switch (protocolActivityWaitReason(run)) {
					case "model_execution":
						return "model execution";
					case "tool_execution":
					case "protocol_activity":
						return "tools";
					case "approval_or_user_input":
						return "approval/user input";
					case "protocol_idleness":
						return "protocol idleness";
					default:
						return "";
				}
			}

			function protocolActivityRecentSummary(run) {
				const events = protocolActivity(run)?.recent_events || [];
				if (!events.length) {
					return "not captured";
				}
				return events
					.slice(-5)
					.reverse()
					.map((event) => {
						const detail = event.detail ? `:${event.detail}` : "";
						return `${event.event_type || "event"}${detail}`;
					})
					.join(", ");
			}

			function protocolActivityDebugSummary(run) {
				const activity = protocolActivity(run);
				if (!activity) {
					return "none";
				}
				const parts = [
					`turn ${activity.turn_status || "none"}`,
					`waiting ${activity.waiting_reason || "none"}`,
					`recent ${protocolActivityRecentSummary(run)}`,
				];

				return parts.join("; ");
			}
