			function buildDerivedState(snapshot) {
				const currentLaneCards = snapshotCurrentLaneCards(snapshot);
				const currentLanes = currentLaneRunsFromCards(currentLaneCards);
				const historyRuns = sessionHistoryRuns(snapshot);
				const executionPrograms = snapshot?.execution_programs ?? [];
				const postReviewLanes = snapshot?.post_review_lanes ?? [];
				const postReviewIssueKeys = new Set(postReviewLanes.flatMap(issueIdentityKeys));
				const currentLaneByIssue = currentLaneByIssueKey(currentLanes);
				const {
					queuedCandidates,
					queueBacklogCandidates,
					reviewOwnedQueueCount,
					staleClosedCandidates,
				} = queueDerivedState(snapshot, currentLaneByIssue, postReviewIssueKeys);
				const worktrees = snapshot?.worktrees ?? [];
				const cleanupIssueKeys = cleanupIssueKeySet(worktrees);
				const waitingItems = [];
				const readyItems = [];
				const attentionItems = [];

				addRunningLaneDerivedItems(currentLanes, waitingItems, attentionItems);
				addPostReviewLaneDerivedItems(
					postReviewLanes,
					currentLaneByIssue,
					waitingItems,
					readyItems,
					attentionItems,
					cleanupIssueKeys,
				);
				sortDerivedActionItems(attentionItems, waitingItems, readyItems);

				const counts = derivedStateCounts({
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
				});

				return {
					projects: snapshot?.projects ?? [],
					currentLaneCards,
					...counts,
					queuedCandidates,
					queueBacklogCandidates,
					attentionItems,
					waitingItems,
					readyItems,
					executionPrograms,
					sessionHistoryRuns: historyRuns,
					worktrees,
					postReviewLanes,
				};
			}
