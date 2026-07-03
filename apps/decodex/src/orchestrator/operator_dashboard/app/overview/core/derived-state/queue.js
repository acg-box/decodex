			function currentLaneByIssueKey(currentLanes) {
				const currentLaneByIssue = new Map();
				for (const run of currentLanes) {
					for (const key of issueIdentityKeys(run)) {
						if (!currentLaneByIssue.has(key)) {
							currentLaneByIssue.set(key, run);
						}
					}
				}
				return currentLaneByIssue;
			}

			function queueDerivedState(snapshot, currentLaneByIssue, postReviewIssueKeys) {
				const queuedCandidates = [...(snapshot?.queued_candidates ?? [])]
					.map((candidate) => queueCandidateWithCurrentLane(candidate, currentLaneByIssue))
					.sort(compareQueuedCandidates);
				const queueBacklogCandidates = queuedCandidates.filter(
					(candidate) =>
						candidate.display_classification !== "leased_run" &&
						candidate.classification !== "closed" &&
						!issueMatchesKeySet(candidate, postReviewIssueKeys),
				);
				const reviewOwnedQueueCount = queuedCandidates.filter(
					(candidate) =>
						candidate.display_classification !== "leased_run" &&
						candidate.classification !== "closed" &&
						issueMatchesKeySet(candidate, postReviewIssueKeys),
				).length;
				const staleClosedCandidates = queuedCandidates.filter(
					(candidate) => candidate.classification === "closed",
				);

				return {
					queuedCandidates,
					queueBacklogCandidates,
					reviewOwnedQueueCount,
					staleClosedCandidates,
				};
			}

			function queueCandidateWithCurrentLane(candidate, currentLaneByIssue) {
				const currentLane = issueIdentityKeys(candidate)
					.map((key) => currentLaneByIssue.get(key))
					.find(Boolean);

				if (currentLane) {
					return {
						...candidate,
						display_classification: "leased_run",
						current_lane_run_id: currentLane.run_id,
					};
				}

				return candidate;
			}

			function cleanupIssueKeySet(worktrees) {
				const cleanupIssueKeys = new Set();
				for (const worktree of worktrees) {
					if (worktree.hygiene) {
						cleanupIssueKeys.add(issueDisplayKey(worktree));
					}
				}
				return cleanupIssueKeys;
			}
