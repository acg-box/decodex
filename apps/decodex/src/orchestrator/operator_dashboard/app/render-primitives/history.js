			function pluralLabel(count, singular, plural = `${singular}s`) {
				return count === 1 ? singular : plural;
			}

			function pluralize(count, singular, plural = `${singular}s`) {
				return `${count} ${pluralLabel(count, singular, plural)}`;
			}

			function rawSessionHistoryRuns(snapshot) {
				const currentLaneRuns = snapshotCurrentLaneRuns(snapshot);
				const currentLaneIds = new Set(currentLaneRuns.map((run) => run.run_id));
				const currentLaneIssueKeys = new Set(currentLaneRuns.flatMap(issueIdentityKeys));
				return (snapshot?.recent_runs ?? []).filter(
					(run) => !currentLaneIds.has(run.run_id) && !issueMatchesKeySet(run, currentLaneIssueKeys),
				);
			}

			function issueIdentifierFromRunId(runId) {
				const match = String(runId || "").match(/^([a-z][a-z0-9]*-\d+)-attempt-\d+(?:-\d+)?$/i);
				if (match) {
					return match[1].toUpperCase();
				}

				const recovered = String(runId || "").match(/^recovered-([a-z][a-z0-9]*-\d+)$/i);
				return recovered ? recovered[1].toUpperCase() : "";
			}

			function attemptNumberFromRun(run) {
				if (run?.attempt_number != null) {
					return String(run.attempt_number);
				}

				const match = String(run?.run_id || "").match(/-attempt-(\d+)(?:-\d+)?$/i);
				return match ? match[1] : "";
			}

			function issueIdentifierInText(value) {
				const match = String(value || "").match(/(?:^|[^A-Za-z0-9])([A-Za-z]+-\d+)(?=$|[^A-Za-z0-9])/);
				return match ? match[1].toUpperCase() : "";
			}

			function runGroupKey(run) {
				return canonicalIssueIdentityKey(run?.issue_id) || issueDisplayKey(run);
			}

			function isSuccessfulTerminalRun(run) {
				return ["succeeded", "completed", "merged"].includes(run?.status);
			}

			function sessionHistoryRuns(snapshot) {
				return sessionHistoryLanes(snapshot).map((lane) => lane.latest_run).filter(Boolean);
			}

			function normalizeHistoryLane(lane) {
				if (!lane?.latest_run) {
					return null;
				}

				const attempts = Array.isArray(lane.attempts) && lane.attempts.length
					? lane.attempts
					: [lane.latest_run];

				return {
					...lane,
					issue_key: lane.issue_key || issueDisplayKey(lane.latest_run),
					attempt_count: Number(lane.attempt_count ?? attempts.length),
					ledger_outcome: historyLedgerOutcome(lane),
					attempts,
				};
			}

			function historyLedgerOutcome(lane) {
				return lane?.ledger_outcome || {
					ledger_status: "not_loaded",
					final_outcome: "local_attempt_history",
					summary: "Linear history not loaded for this snapshot.",
					record_count: 0,
				};
			}

			function historyLedgerHasRecords(outcome) {
				return ["present", "partial"].includes(outcome?.ledger_status);
			}

			function historyLedgerWasLoaded(outcome) {
				return (outcome?.ledger_status || "not_loaded") !== "not_loaded";
			}

			function toneForHistoryLedgerOutcome(outcome, run) {
				if (
					[
						"needs_attention",
						"terminal_failure",
						"ledger_unavailable",
						"execution_ledger_missing",
					].includes(outcome?.final_outcome)
				) {
					return "tone-blocked";
				}
				if (["unavailable", "partial", "missing"].includes(outcome?.ledger_status)) {
					return "tone-wait";
				}
				if (["closeout", "cleanup_complete", "landed"].includes(outcome?.final_outcome)) {
					return "tone-land";
				}
				if (["review_handoff", "repair_handoff"].includes(outcome?.final_outcome)) {
					return "tone-review";
				}

				return toneForRun(run);
			}

			function historyLaneTitle(lane) {
				const outcome = historyLedgerOutcome(lane);

				if (historyLedgerWasLoaded(outcome)) {
					if (outcome.final_outcome === "ledger_unavailable") {
						return "Run history unavailable";
					}
					if (outcome.final_outcome === "execution_ledger_missing") {
						return "Execution ledger missing";
					}
					return displayToken(outcome.final_outcome || outcome.ledger_status);
				}

				return recentRunTitle(lane.latest_run);
			}

			function historyLaneSummary(lane) {
				const outcome = historyLedgerOutcome(lane);

				if (historyLedgerWasLoaded(outcome)) {
					return outcome.summary || `Latest recorded run event is ${displayToken(outcome.final_outcome)}.`;
				}

				return recentRunSummary(lane.latest_run, lane);
			}

			function historyLaneStatusBits(lane, tone) {
				const outcome = historyLedgerOutcome(lane);
				const run = lane.latest_run;

				if (!historyLedgerWasLoaded(outcome)) {
					const bits = [statusLabel(displayToken(run.status), tone)];

					if (run.wait_reason) {
						const waitReason = displayToken(run.wait_reason);
						if (!displayTextRepeats(recentRunSummary(run, lane), waitReason)) {
							bits.push(inlineStatusFact("Wait", waitReason));
						}
					}
					if (run.continuation_pending) {
						bits.push(inlineStatusFact("Continuation", "Pending"));
					}
					if (run.retry_kind) {
						bits.push(inlineStatusFact("Retry", displayToken(run.retry_kind)));
					}

					return bits;
				}

				const bits = [statusLabel(displayToken(outcome.final_outcome), tone)];

				bits.push(inlineStatusFact("History", displayToken(outcome.ledger_status)));
				if (outcome.closeout_status) {
					bits.push(inlineStatusFact("Closeout", displayToken(outcome.closeout_status)));
				}
				if (outcome.needs_attention_reason) {
					bits.push(inlineStatusFact("Attention", "Recorded"));
				}

				return bits;
			}

			function groupedHistoryLanesFromRuns(runs) {
				const lanes = [];
				const laneIndexes = new Map();

				for (const run of runs) {
					const key = runGroupKey(run);
					const index = laneIndexes.get(key);

					if (index != null) {
						const lane = lanes[index];
						lane.attempt_count += 1;
						lane.attempts.push(run);
						continue;
					}

					laneIndexes.set(key, lanes.length);
					lanes.push({
						issue_id: run.issue_id || key,
						issue_key: issueDisplayKey(run),
						attempt_count: 1,
						latest_run: run,
						attempts: [run],
					});
				}

				return lanes;
			}

			function sessionHistoryLanes(snapshot) {
				if (Array.isArray(snapshot?.history_lanes)) {
					return snapshot.history_lanes.map(normalizeHistoryLane).filter(Boolean);
				}

				return groupedHistoryLanesFromRuns(rawSessionHistoryRuns(snapshot));
			}

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
