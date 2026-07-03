			function sortDerivedActionItems(attentionItems, waitingItems, readyItems) {
				attentionItems.sort((left, right) => left.issue.localeCompare(right.issue));
				waitingItems.sort((left, right) => left.issue.localeCompare(right.issue));
				readyItems.sort((left, right) => left.issue.localeCompare(right.issue));
			}

			function derivedStateCounts({
				currentLanes,
				queueBacklogCandidates,
				queuedCandidates,
				staleClosedCandidates,
				reviewOwnedQueueCount,
				attentionItems,
				waitingItems,
				readyItems,
				cleanupIssueKeys,
				executionPrograms,
			}) {
				const cleanupCount = cleanupIssueKeys.size;

				return {
					liveRuns: currentLanes.filter(runCountsAsRunning).length,
					currentLaneCount: currentLanes.length,
					liveLeases: currentLanes.filter((run) => run.run_lease).length,
					readyCount: readyItems.length,
					queuedReady: queueBacklogCandidates.filter((candidate) => candidate.classification === "ready").length,
					queuedWaiting: queueBacklogCandidates.filter((candidate) => candidate.classification === "waiting").length,
					queuedActiveOwned: queuedCandidates.filter((candidate) => candidate.display_classification === "leased_run").length,
					queuedBlocked: queueBacklogCandidates.filter((candidate) => candidate.classification === "blocked").length,
					queuedBlockedWithoutAttention: queueBacklogCandidates.filter(
						(candidate) =>
							candidate.classification === "blocked" && !queuedCandidateNeedsAttention(candidate),
					).length,
					intakeAttentionCount: queueBacklogCandidates.filter(queuedCandidateNeedsAttention).length,
					queuedClosed: staleClosedCandidates.length,
					reviewOwnedQueueCount,
					reviewWaitingCount: waitingItems.filter((item) => item.scope === "Review").length,
					reviewBlockerCount: attentionItems.filter((item) =>
						["Review", "Closeout"].includes(item.scope),
					).length,
					cleanupCount,
					runningAttentionCount: attentionItems.filter((item) => item.scope === "Running").length,
					programAttentionCount: executionPrograms.filter((program) =>
						["attention", "blocked", "stale"].includes(program.status),
					).length,
					programReadyCount: executionPrograms.reduce(
						(total, program) => total + Number(program.ready_count || 0),
						0,
					),
					programDispatchableCount: executionPrograms.reduce(
						(total, program) => total + Number(program.dispatchable_count || 0),
						0,
					),
				};
			}
