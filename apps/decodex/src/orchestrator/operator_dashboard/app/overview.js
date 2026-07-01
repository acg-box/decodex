
			function buildDerivedState(snapshot) {
				const currentLaneCards = snapshotCurrentLaneCards(snapshot);
				const currentLanes = currentLaneRunsFromCards(currentLaneCards);
				const recentRuns = snapshot?.recent_runs ?? [];
				const historyRuns = sessionHistoryRuns(snapshot);
				const executionPrograms = snapshot?.execution_programs ?? [];
				const postReviewLanes = snapshot?.post_review_lanes ?? [];
				const postReviewIssueKeys = new Set(postReviewLanes.flatMap(issueIdentityKeys));
				const currentLaneByIssue = new Map();
				for (const run of currentLanes) {
					for (const key of issueIdentityKeys(run)) {
						if (!currentLaneByIssue.has(key)) {
							currentLaneByIssue.set(key, run);
						}
					}
				}
				const queuedCandidates = [...(snapshot?.queued_candidates ?? [])]
					.map((candidate) => {
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
					})
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
				const worktrees = snapshot?.worktrees ?? [];
				const cleanupIssueKeys = new Set();
				for (const worktree of worktrees) {
					if (worktree.hygiene) {
						cleanupIssueKeys.add(issueDisplayKey(worktree));
					}
				}
				const waitingItems = [];
				const readyItems = [];
				const attentionItems = [];

				for (const run of currentLanes) {
					const tone = toneForRun(run);
					const issueKey = issueDisplayKey(run);

					if (runNeedsAttention(run)) {
						attentionItems.push({
							tone: "tone-blocked",
							scope: "Running",
							issue: issueKey,
							title: runStoppedProcessNeedsAttention(run)
								? "Stopped agent process"
								: displayToken(run.run_phase || run.phase || run.status),
							summary: runStoppedProcessNeedsAttention(run)
								? `Agent process stopped while lane remains active. ${COPY.protocolEvent} ${run.last_event_type || "none"}.`
								: runStaleWithoutKnownProcessNeedsAttention(run)
									? `No live agent process after ${formatDuration(runStaleWithoutKnownProcessAgeSeconds(run))}; lane remains active.`
									: `No confirmed progress for ${formatDuration(run.idle_for_seconds)}. ${COPY.protocolEvent} ${run.last_event_type || "none"}.`,
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
						});
						continue;
					}

					const runIsWaiting =
						run.phase === "retry_backoff" ||
						run.phase === "waiting_continuation" ||
						(run.wait_reason && !runWaitReasonShowsExecutionProgress(run));

					if (runIsWaiting) {
						waitingItems.push({
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
						});
					}
				}

				for (const lane of postReviewLanes) {
					const tone = toneForLane(lane);
					const currentLane = issueIdentityKeys(lane)
						.map((key) => currentLaneByIssue.get(key))
						.find(Boolean);
					const shadowedByCurrentLane = lane.shadowed_by_current_lane === true;
					const issueKey = issueDisplayKey(lane);

					if (shadowedByCurrentLane) {
						const currentLaneFacts = currentLane
							? [
								["Run", currentLane.run_id],
								[
									"Operation",
									displayToken(currentLane.current_operation || currentLane.run_phase || currentLane.phase),
								],
							]
							: [["Run", "active"]];

						waitingItems.push({
							tone: "tone-run",
							scope: "Review",
							issue: issueKey,
							title: "Repair running",
							summary: "",
							status: currentLane
								? `run phase ${displayToken(currentLane.run_phase || currentLane.phase)}`
								: "current lane",
							facts: [
								...currentLaneFacts,
								["Checks", compactStateToken(lane.check_state)],
								["Threads", reviewThreadToken(lane.unresolved_review_threads)],
								["PR", optionalCardToken(lane.pr_url)],
								["Branch", optionalCardToken(lane.branch_name)],
								...loopStatusFacts(lane.loop_status),
								...postReviewReadbackFacts(lane),
							],
						});
						continue;
					}

					if (isPostReviewBlocker(lane)) {
						const blockerScope = postReviewBlockerScope(lane);
						if (blockerScope === "Cleanup") {
							cleanupIssueKeys.add(issueKey);
						}
						attentionItems.push({
							tone,
							scope: blockerScope,
							issue: issueKey,
							title: postReviewBlockerTitle(lane),
							summary: "",
							status: postReviewBlockerStatus(lane, blockerScope),
							facts: [
								["Checks", compactStateToken(lane.check_state)],
								["Threads", reviewThreadToken(lane.unresolved_review_threads)],
								["PR", optionalCardToken(lane.pr_url)],
								["Branch", optionalCardToken(lane.branch_name)],
								...loopStatusFacts(lane.loop_status),
								...postReviewReadbackFacts(lane),
							],
						});
						continue;
					}

					if (lane.classification === "wait_for_review") {
						waitingItems.push({
							tone,
							scope: "Review",
							issue: issueKey,
							title: "Wait for review",
							summary: "",
							status: lane.check_state ? `checks ${compactStateToken(lane.check_state)}` : "waiting",
							facts: [
								["Review decision", compactStateToken(lane.review_decision)],
								["Threads", reviewThreadToken(lane.unresolved_review_threads)],
								["PR", optionalCardToken(lane.pr_url)],
								["Branch", optionalCardToken(lane.branch_name)],
								...loopStatusFacts(lane.loop_status),
								...postReviewReadbackFacts(lane),
							],
						});
						continue;
					}

					if (lane.classification === "ready_to_land") {
						readyItems.push({
							tone,
							scope: "Review",
							issue: issueKey,
							title: "Ready to land",
							summary: "",
							status: lane.mergeable ? `merge ${compactStateToken(lane.mergeable)}` : "ready",
							facts: [
								["Review decision", compactStateToken(lane.review_decision)],
								["Checks", compactStateToken(lane.check_state)],
								["Threads", reviewThreadToken(lane.unresolved_review_threads)],
								["PR", optionalCardToken(lane.pr_url)],
								["Branch", optionalCardToken(lane.branch_name)],
								...loopStatusFacts(lane.loop_status),
								...postReviewReadbackFacts(lane),
							],
						});
					}
				}

				attentionItems.sort((left, right) => left.issue.localeCompare(right.issue));
				waitingItems.sort((left, right) => left.issue.localeCompare(right.issue));
				readyItems.sort((left, right) => left.issue.localeCompare(right.issue));
				const reviewWaitingCount = waitingItems.filter(
					(item) => item.scope === "Review",
				).length;
				const reviewBlockerCount = attentionItems.filter((item) =>
					["Review", "Closeout"].includes(item.scope),
				).length;
				const cleanupCount = cleanupIssueKeys.size;
				const runningAttentionCount = attentionItems.filter((item) => item.scope === "Running").length;
				const liveRuns = currentLanes.filter(runCountsAsRunning).length;
				const intakeAttentionCount = queueBacklogCandidates.filter(queuedCandidateNeedsAttention).length;
				const programAttentionCount = executionPrograms.filter((program) =>
					["attention", "blocked", "stale"].includes(program.status),
				).length;
				const programReadyCount = executionPrograms.reduce(
					(total, program) => total + Number(program.ready_count || 0),
					0,
				);
				const programDispatchableCount = executionPrograms.reduce(
					(total, program) => total + Number(program.dispatchable_count || 0),
					0,
				);

				return {
					projects: snapshot?.projects ?? [],
					currentLaneCards,
					liveRuns,
					currentLaneCount: currentLanes.length,
					liveLeases: currentLanes.filter((run) => run.run_lease).length,
					readyCount: readyItems.length,
					queuedCandidates,
					queueBacklogCandidates,
					queuedReady: queueBacklogCandidates.filter((candidate) => candidate.classification === "ready").length,
					queuedWaiting: queueBacklogCandidates.filter((candidate) => candidate.classification === "waiting").length,
					queuedActiveOwned: queuedCandidates.filter((candidate) => candidate.display_classification === "leased_run").length,
					queuedBlocked: queueBacklogCandidates.filter((candidate) => candidate.classification === "blocked").length,
					queuedBlockedWithoutAttention: queueBacklogCandidates.filter(
						(candidate) =>
							candidate.classification === "blocked" && !queuedCandidateNeedsAttention(candidate),
					).length,
					intakeAttentionCount,
					queuedClosed: staleClosedCandidates.length,
					reviewOwnedQueueCount,
					attentionItems,
					waitingItems,
					readyItems,
					reviewWaitingCount,
					reviewBlockerCount,
					cleanupCount,
					runningAttentionCount,
					executionPrograms,
					programAttentionCount,
					programReadyCount,
					programDispatchableCount,
					sessionHistoryRuns: historyRuns,
					worktrees,
					postReviewLanes,
				};
			}

			function renderHeader(snapshot, readiness, notices, snapshotPublishedAt, snapshotError) {
				nodes.projectTitle.textContent = "Decodex";
				document.title = snapshot
					? `${snapshot.project_id} · Decodex`
					: "Decodex";
				const snapshotFreshness = snapshotFreshnessMeta(
					snapshotPublishedAt,
					readiness,
					snapshotError,
				);
				const snapshotFreshnessRow = snapshotFreshness
					? `
						<span class="transport-meta" data-kind="snapshot" data-tone="${escapeHtml(snapshotFreshness.tone)}" title="${escapeHtml(snapshotFreshness.title)}">
							<span>Snapshot</span><strong>${escapeHtml(snapshotFreshness.label)}</strong>
						</span>
					`
					: "";
				const stream = dashboardStreamMeta();

				nodes.transportHealth.innerHTML = `
					<span class="status-pill ${readiness.tone}">${escapeHtml(topbarReadinessLabel(readiness.label))}</span>
					<span class="transport-meta" data-kind="endpoint" data-tone="${escapeHtml(stream.tone)}" title="${escapeHtml(stream.title)}">
						<span>Transport</span><strong>${renderValueLink("WebSocket", dashboardSocketUrl(), "transport-link") || escapeHtml(dashboardSocketUrl())}</strong>
					</span>
					${snapshotFreshnessRow}
				`;
				renderNoticeDock(notices);
			}
