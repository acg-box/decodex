			function addRunningLaneDerivedItems(currentLanes, waitingItems, attentionItems) {
				for (const run of currentLanes) {
					const tone = toneForRun(run);
					const issueKey = issueDisplayKey(run);

					if (runNeedsAttention(run)) {
						attentionItems.push(runningAttentionItem(run, issueKey));
						continue;
					}

					if (runningLaneIsWaiting(run)) {
						waitingItems.push(runningWaitingItem(run, tone, issueKey));
					}
				}
			}

			function runningLaneIsWaiting(run) {
				return (
					run.phase === "retry_backoff" ||
					run.phase === "waiting_continuation" ||
					(run.wait_reason && !runWaitReasonShowsExecutionProgress(run))
				);
			}

			function runningAttentionItem(run, issueKey) {
				return {
					tone: "tone-blocked",
					scope: "Running",
					issue: issueKey,
					title: runStoppedProcessNeedsAttention(run)
						? "Stopped agent process"
						: displayToken(run.run_phase || run.phase || run.status),
					summary: runningAttentionSummary(run),
					status: runStoppedProcessNeedsAttention(run)
						? "attention stopped"
						: `run phase ${displayToken(run.run_phase || run.phase)}`,
					facts: [
						["Codex thread", runThreadSummary(run)],
						["Thread flags", runThreadFlagSummary(run)],
						["Process", runProcessSummary(run)],
						currentLaneFreshnessFact(run),
						["Protocol idle", formatDuration(run.protocol_idle_for_seconds)],
						["Last progress", formatTimestamp(run.last_progress_at)],
						[COPY.protocolEvent, protocolEventSummary(run)],
						["Branch", run.branch_name || "none"],
						["Next retry", formatTimestamp(run.next_retry_at)],
					],
				};
			}

			function runningAttentionSummary(run) {
				if (runStoppedProcessNeedsAttention(run)) {
					return `Agent process stopped while lane remains active. ${COPY.protocolEvent} ${run.last_event_type || "none"}.`;
				}
				if (runStaleWithoutKnownProcessNeedsAttention(run)) {
					return `No live agent process after ${formatDuration(runStaleWithoutKnownProcessAgeSeconds(run))}; lane remains active.`;
				}
				return `No confirmed progress for ${formatDuration(run.idle_for_seconds)}. ${COPY.protocolEvent} ${run.last_event_type || "none"}.`;
			}

			function runningWaitingItem(run, tone, issueKey) {
				return {
					tone,
					scope: "Running",
					issue: issueKey,
					title: displayToken(run.run_phase || run.phase || run.status),
					summary: run.next_retry_at
						? `Retry scheduled for ${formatTimestamp(run.next_retry_at)}.`
						: `Waiting on ${displayToken(run.wait_reason || run.run_phase || run.phase)}.`,
					status: "waiting",
					facts: [
						["Codex thread", runThreadSummary(run)],
						["Thread flags", runThreadFlagSummary(run)],
						currentLaneFreshnessFact(run),
						["Lane idle", formatDuration(run.idle_for_seconds)],
						["Protocol idle", formatDuration(run.protocol_idle_for_seconds)],
						[COPY.protocolEvent, protocolEventSummary(run)],
						["Branch", run.branch_name || "none"],
						["Worktree", run.worktree_path || "none"],
					],
				};
			}
