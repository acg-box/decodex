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
