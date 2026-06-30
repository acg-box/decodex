
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

			function selectedProjectId(snapshot, projects) {
				if (snapshot?.project_id && snapshot.project_id !== "all") {
					return snapshot.project_id;
				}
				if (projects.length === 1 && projects[0].enabled) {
					return projects[0].project_id;
				}

				return null;
			}

			function projectHasActiveWork(project) {
				const workCount =
					(project.current_lane_count ?? 0) +
					(project.queued_candidate_count ?? 0) +
					(project.waiting_lane_count ?? 0) +
					(project.attention_count ?? 0) +
					(project.cleanup_blocked_count ?? 0) +
					(project.cleanup_pending_count ?? 0) +
					(project.post_review_lane_count ?? 0);

				return workCount > 0;
			}

			function projectRunningLaneCount(project) {
				return project.running_lane_count ?? project.current_lane_count ?? 0;
			}

			function activeProjects(projects) {
				return projects.filter(projectHasActiveWork);
			}

			function projectHealth(project) {
				if (!project.enabled) {
					return { label: "disabled", tone: "tone-muted", title: "Disabled in registry" };
				}
				if (projectRunningLaneCount(project) > 0) {
					return { label: "running", tone: "tone-run", title: "Active running lanes" };
				}
				if ((project.attention_count ?? 0) > 0) {
					return { label: "needs attention", tone: "tone-blocked", title: "Operator attention needed" };
				}
				if ((project.waiting_lane_count ?? 0) > 0) {
					return { label: "waiting", tone: "tone-wait", title: "Lanes waiting to resume" };
				}
				if ((project.cleanup_blocked_count ?? 0) > 0) {
					return { label: "cleanup blocked", tone: "tone-wait", title: "Post-land cleanup needs operator action" };
				}
				if ((project.cleanup_pending_count ?? 0) > 0) {
					return { label: "cleanup pending", tone: "tone-retained", title: "Post-land cleanup pending" };
				}
				if (project.connector_state === "backoff") {
					return {
						label: "sync backoff",
						tone: "tone-wait",
						title: "Tracker sync paused by rate limit.",
					};
				}
				if (project.connector_state === "config_error") {
					return {
						label: "config error",
						tone: "tone-wait",
						title: "Registered project config could not be loaded.",
					};
				}
				if (
					(project.warning_count ?? 0) > 0 ||
					["degraded", "stale_cache"].includes(project.connector_state)
				) {
					return { label: "sync degraded", tone: "tone-muted", title: "Tracker sync or retry state degraded" };
				}

				return { label: "ok", tone: "tone-ready", title: "No project warnings" };
			}

			function projectCapacitySummary(project) {
				const cleanup = (project.cleanup_blocked_count ?? 0) + (project.cleanup_pending_count ?? 0);

				return [
					`${projectRunningLaneCount(project)} running`,
					`${project.waiting_lane_count ?? 0} waiting`,
					`${project.attention_count ?? 0} attention`,
					`${cleanup} cleanup`,
				].join(" · ");
			}

			function projectKicker(project) {
				if (!project.enabled) {
					return "Disabled";
				}

				return "";
			}

			function renderProjectStats(project) {
				const running = projectRunningLaneCount(project);
				const waiting = project.waiting_lane_count ?? 0;
				const attention = project.attention_count ?? 0;
				const cleanup = (project.cleanup_blocked_count ?? 0) + (project.cleanup_pending_count ?? 0);

				return `
					<span class="project-work-ratio" role="cell" aria-label="${escapeHtml(`${running} running, ${waiting} waiting, ${attention} attention, ${cleanup} cleanup`)}" title="running / waiting / attention / cleanup">
						<strong>${escapeHtml(running)}</strong><span class="project-work-separator">/</span><strong>${escapeHtml(waiting)}</strong><span class="project-work-separator">/</span><strong>${escapeHtml(attention)}</strong><span class="project-work-separator">/</span><strong>${escapeHtml(cleanup)}</strong>
					</span>
				`;
			}

			function projectAttentionSummary(project) {
				if (!project.enabled) {
					return "disabled";
				}
				if ((project.attention_count ?? 0) > 0) {
					return `${project.attention_count} attention`;
				}
				if ((project.cleanup_blocked_count ?? 0) > 0) {
					return `${project.cleanup_blocked_count} cleanup blocked`;
				}
				if ((project.cleanup_pending_count ?? 0) > 0) {
					return `${project.cleanup_pending_count} cleanup pending`;
				}
				if (project.connector_state === "config_error") {
					return "config error";
				}
				if ((project.warning_count ?? 0) > 0) {
					return pluralize(project.warning_count, "warning");
				}
				if ((project.retained_worktree_count ?? 0) > 0) {
					return `${pluralize(project.retained_worktree_count, "worktree")} retained`;
				}

				return "ok";
			}

			function projectConnectorSummary(project) {
				if (!project.enabled || project.connector_state === "disabled") {
					return "disabled";
				}
				if (project.connector_state === "backoff") {
					return "backoff";
				}
				if (project.connector_state === "stale_cache") {
					return "stale";
				}
				if (project.connector_state === "degraded") {
					return "degraded";
				}
				if (project.connector_state === "config_error") {
					return "config error";
				}

				return "ok";
			}

			function projectPathSummary(project) {
				return project.repo_root || project.config_path || "path unavailable";
			}

			function compactProjectLocation(projectPath) {
				return String(projectPath || "").replace(/^\/Users\/[^/]+/, "~");
			}

			function projectLocationText(projectPath) {
				return projectLocationsHidden ? "-" : compactProjectLocation(projectPath);
			}

			function projectLocationMarkup(projectPath) {
				const projectLocation = projectLocationText(projectPath);
				if (projectLocationsHidden || projectLocation === "-") {
					return `<span class="project-path-tail">-</span>`;
				}

				const slashIndex = projectLocation.lastIndexOf("/");
				if (slashIndex <= 0) {
					return `<span class="project-path-tail">${escapeHtml(projectLocation)}</span>`;
				}

				return `
					<span class="project-path-prefix">${escapeHtml(projectLocation.slice(0, slashIndex + 1))}</span>
					<span class="project-path-tail">${escapeHtml(projectLocation.slice(slashIndex + 1))}</span>
				`;
			}

			function projectWorkInfoMarkup() {
				return `
					<span class="project-work-info-wrap">
						<button class="project-work-info" type="button" data-project-work-info aria-expanded="false" aria-label="Work format: running / waiting / attention / cleanup">
							<svg viewBox="0 0 16 16" aria-hidden="true" focusable="false" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round">
								<circle cx="8" cy="8" r="6"></circle>
								<path d="M8 7.3v3.8"></path>
								<path d="M8 4.8h.01"></path>
							</svg>
						</button>
						<span class="project-work-tooltip" role="tooltip">running / waiting / attention / cleanup</span>
					</span>
				`;
			}

			function renderProjectSortButton([key, label]) {
				const direction = projectSort.key === key ? projectSort.direction : "";
				const activeClass = direction ? ` is-active is-${direction}` : "";
				const current = direction
					? `currently ${direction === "asc" ? "ascending" : "descending"}`
					: "not sorted";

				return `
					<button class="project-table-sort${activeClass}" type="button" data-project-sort-key="${escapeHtml(key)}" aria-label="Sort projects by ${escapeHtml(label)}; ${escapeHtml(current)}" title="Sort by ${escapeHtml(label)}">
						<span class="project-table-sort-label">${escapeHtml(label)}</span>
						<svg class="project-table-sort-icon" aria-hidden="true" viewBox="0 0 8 12" fill="currentColor">
							<path class="project-sort-up" d="M4 1 1 5h6Z"></path>
							<path class="project-sort-down" d="M4 11 1 7h6Z"></path>
						</svg>
					</button>
				`;
			}

			function renderProjectColumnHead(column, options = {}) {
				const [key] = column;
				const direction = projectSort.key === key ? projectSort.direction : "";
				const ariaSort = direction
					? ` aria-sort="${direction === "asc" ? "ascending" : "descending"}"`
					: "";
				const className = `project-column-head${options.className ? ` ${options.className}` : ""}`;

				return `
					<span class="${escapeHtml(className)}" role="columnheader"${ariaSort}>
						${renderProjectSortButton(column)}
						${options.after || ""}
					</span>
				`;
			}

			const projectRegistrationCommand =
				"decodex project add ~/.codex/decodex/projects/<service-id>";

			function renderProjectEmptyState(title, copy, options = {}) {
				const action =
					options.showAction === false
						? ""
						: `<code class="project-empty-action">${escapeHtml(projectRegistrationCommand)}</code>`;
				return `
					<div class="empty-state">
						<strong>${escapeHtml(title)}</strong>
						<div class="empty-copy project-empty-copy">
							<span>${escapeHtml(copy)}</span>
							${action}
						</div>
					</div>
				`;
			}

			function renderProjectEntry(project, selectedId) {
				const health = projectHealth(project);
				const isActive = selectedId === project.project_id;
				const projectPath = projectPathSummary(project);
				const projectLocationTitle = projectLocationsHidden ? "Project location hidden" : projectPath;
				const kicker = projectKicker(project);
				const lastActivity = formatRelativeTimestamp(project.last_activity_at);
				const activityCopy = lastActivity === "none" ? "-" : lastActivity;
				const ariaCurrent = isActive ? ' aria-current="true"' : "";

				return `
					<article class="project-entry ${health.tone}${isActive ? " is-active" : ""}" role="row"${ariaCurrent} aria-label="Project ${escapeHtml(project.project_id)} ${escapeHtml(health.label)} ${escapeHtml(projectCapacitySummary(project))}">
						<div class="project-entry-main" role="cell">
							<div class="project-title-line">
								<strong>${escapeHtml(project.project_id)}</strong>
								${kicker ? `<span class="project-kicker">${escapeHtml(kicker)}</span>` : ""}
							</div>
						</div>
						<div class="project-path" role="cell" title="${escapeHtml(projectLocationTitle)}">${projectLocationMarkup(projectPath)}</div>
						<span class="project-activity" role="cell">${escapeHtml(activityCopy)}</span>
						<div class="project-stat-line" aria-label="Project status summary">
							${renderProjectStats(project)}
						</div>
					</article>
				`;
			}

			function projectFilterRows(projects, activeProjectRows) {
				return projectFilterMode === "all" ? projects : activeProjectRows;
			}

			function projectNumber(value) {
				const number = Number(value ?? 0);
				return Number.isFinite(number) ? number : 0;
			}

			function projectTimestampSortValue(value) {
				if (!value) {
					return null;
				}

				const parsed = new Date(value);
				const time = parsed.getTime();
				return Number.isNaN(time) ? null : time;
			}

			function projectWorkSortValue(project) {
				return [
					projectNumber(projectRunningLaneCount(project)),
					projectNumber(project.waiting_lane_count),
					projectNumber(project.attention_count),
					projectNumber(project.cleanup_blocked_count),
					projectNumber(project.cleanup_pending_count),
				];
			}

			function projectColumnSortValue(project, key) {
				if (key === "project") {
					return String(project.project_id || "").toLowerCase();
				}
				if (key === "location") {
					return compactProjectLocation(projectPathSummary(project)).toLowerCase();
				}
				if (key === "activity") {
					return projectTimestampSortValue(project.last_activity_at);
				}
				if (key === "work") {
					return projectWorkSortValue(project);
				}

				return "";
			}

			function compareProjectSortValues(leftValue, rightValue, direction) {
				const leftMissing = leftValue == null || leftValue === "";
				const rightMissing = rightValue == null || rightValue === "";
				if (leftMissing && rightMissing) {
					return 0;
				}
				if (leftMissing) {
					return 1;
				}
				if (rightMissing) {
					return -1;
				}

				if (Array.isArray(leftValue) && Array.isArray(rightValue)) {
					const count = Math.max(leftValue.length, rightValue.length);
					for (let index = 0; index < count; index += 1) {
						const delta = compareProjectSortValues(
							leftValue[index] ?? 0,
							rightValue[index] ?? 0,
							direction,
						);
						if (delta) {
							return delta;
						}
					}

					return 0;
				}

				const delta =
					typeof leftValue === "number" && typeof rightValue === "number"
						? leftValue === rightValue
							? 0
							: leftValue < rightValue
								? -1
								: 1
						: String(leftValue).localeCompare(String(rightValue));

				return direction === "desc" ? -delta : delta;
			}

			function compareProjectRowsByColumn(left, right, key, direction) {
				return compareProjectSortValues(
					projectColumnSortValue(left, key),
					projectColumnSortValue(right, key),
					direction,
				);
			}

			function projectOperationalRank(project) {
				if (!project.enabled) {
					return 5;
				}
				if (projectRunningLaneCount(project) > 0) {
					return 0;
				}
				if ((project.attention_count ?? 0) > 0) {
					return 1;
				}
				if ((project.waiting_lane_count ?? 0) > 0) {
					return 2;
				}
				if ((project.cleanup_blocked_count ?? 0) > 0) {
					return 3;
				}
				if ((project.cleanup_pending_count ?? 0) > 0) {
					return 4;
				}
				if (projectHasActiveWork(project)) {
					return 5;
				}
				if (["backoff", "config_error", "degraded", "stale_cache"].includes(project.connector_state)) {
					return 6;
				}
				if ((project.warning_count ?? 0) > 0) {
					return 7;
				}

				return 8;
			}

			function compareProjectRowsStable(left, right) {
				const rankDelta = projectOperationalRank(left) - projectOperationalRank(right);
				if (rankDelta) {
					return rankDelta;
				}

				return String(left.project_id || "").localeCompare(String(right.project_id || ""));
			}

			function sortProjectRows(rows) {
				return [...rows].sort((left, right) => {
					if (projectSort.key) {
						const columnDelta = compareProjectRowsByColumn(
							left,
							right,
							projectSort.key,
							projectSort.direction,
						);
						if (columnDelta) {
							return columnDelta;
						}
					}

					return compareProjectRowsStable(left, right);
				});
			}

			function renderProjectTable(projects, activeProjectRows, selectedId) {
				const rows = sortProjectRows(projectFilterRows(projects, activeProjectRows));
				const label = projectFilterMode === "all" ? "All projects" : "Active projects";

				if (!rows.length) {
					return "";
				}

				return `
					<div class="project-table" role="table" aria-label="${escapeHtml(label)}">
						<div class="project-table-guide" role="row">
							${renderProjectColumnHead(PROJECT_SORT_COLUMNS[0])}
							${renderProjectColumnHead(PROJECT_SORT_COLUMNS[1], {
								className: "project-location-head",
								after: projectLocationToggleMarkup(),
							})}
							${renderProjectColumnHead(PROJECT_SORT_COLUMNS[2])}
							${renderProjectColumnHead(PROJECT_SORT_COLUMNS[3], {
								after: projectWorkInfoMarkup(),
							})}
						</div>
						<div class="project-table-list" role="rowgroup">
							${rows.map((project) => renderProjectEntry(project, selectedId)).join("")}
						</div>
					</div>
				`;
			}

			function renderProjects(snapshot, derived) {
				if (!snapshot) {
					renderProjectFilterToggle();
					nodes.projectOverview.classList.add("is-empty", "is-waiting");
					nodes.projectOverview.innerHTML = "";
					return;
				}

				const projects = derived.projects;
				const activeProjectRows = activeProjects(projects);

				if (!projects.length) {
					renderProjectFilterToggle(projects);
					nodes.projectOverview.classList.add("is-empty");
					nodes.projectOverview.classList.remove("has-registered-projects");
					nodes.projectOverview.classList.remove("is-waiting");
					nodes.projectOverview.innerHTML = renderProjectEmptyState(
						"No projects",
						"Register projects explicitly; Decodex does not scan history or repos.",
					);
					return;
				}

				const selectedId = selectedProjectId(snapshot, projects);
				const visibleProjectRows = projectFilterRows(projects, activeProjectRows);

				renderProjectFilterToggle(projects);
				nodes.projectOverview.classList.remove("is-empty", "is-waiting");
				nodes.projectOverview.classList.toggle("is-empty", visibleProjectRows.length === 0);
				nodes.projectOverview.classList.toggle("has-registered-projects", visibleProjectRows.length > 0);
				nodes.projectOverview.innerHTML = renderProjectTable(projects, activeProjectRows, selectedId);
				renderProjectLocationToggle();
				renderProjectWorkInfoState();
			}

			function setFlowCounts(queue, run, review, land) {
				setMetricText(nodes.flowCounts.queue, queue);
				setMetricText(nodes.flowCounts.run, run);
				setMetricText(nodes.flowCounts.review, review);
				setMetricText(nodes.flowCounts.land, land);
			}

			function setFlowActivity(activity) {
				for (const label of nodes.flowStepLabels) {
					label.classList.toggle("active", Boolean(activity[label.dataset.flowStep]));
				}
			}

			function renderFlow(snapshot, derived) {
				if (!snapshot) {
					setFlowCounts("0 issues", "0 lanes", "0 PRs", "0 PRs");
					setFlowActivity({});
					return;
				}

				const retainedCount = (snapshot.post_review_lanes ?? []).length;
				setFlowCounts(
					pluralize(derived.queueBacklogCandidates.length, "issue"),
					pluralize(derived.currentLaneCount, "lane"),
					pluralize(retainedCount, "PR"),
					pluralize(derived.readyCount, "PR"),
				);
				setFlowActivity({
					queue: derived.queueBacklogCandidates.length > 0,
					run: derived.currentLaneCount > 0,
					review:
						derived.reviewBlockerCount > 0 ||
						derived.reviewWaitingCount > 0 ||
						derived.postReviewLanes.length > 0,
					land: derived.readyCount > 0,
				});
			}

			function runningLaneMetaText(derived) {
				const parts = [`${derived.liveRuns ?? 0} running`];
				const attentionCount = derived.runningAttentionCount;

				if (attentionCount) {
					parts.push(
						attentionCount === 1
							? "1 needs attention"
							: `${attentionCount} need attention`,
					);
				}

				return parts.join(" · ");
			}

			function backlogMetaText(snapshot, derived) {
				if (!snapshot) {
					return "0 queued";
				}

				const parts = [`${derived.queueBacklogCandidates.length} queued`];

				if (derived.queuedReady) {
					parts.push(`${derived.queuedReady} ready`);
				}
				if (derived.queuedWaiting) {
					parts.push(`${derived.queuedWaiting} waiting`);
				}
				if (derived.queuedBlocked) {
					parts.push(`${derived.queuedBlocked} blocked`);
				}
				if (derived.queuedActiveOwned) {
					parts.push(
						pluralize(
							derived.queuedActiveOwned,
							COPY.runningInlineMeta,
							COPY.runningInlineMetaPlural,
						),
					);
				}
				if (derived.queuedClosed) {
					parts.push(`${derived.queuedClosed} ${COPY.staleClosed}`);
				}
				if (derived.reviewOwnedQueueCount) {
					parts.push(`${derived.reviewOwnedQueueCount} in review`);
				}

				return parts.join(" · ");
			}

			function programMetaText(snapshot, derived) {
				if (!snapshot) {
					return "0 programs";
				}

				const parts = [pluralize(derived.executionPrograms.length, "program")];

				if (derived.programReadyCount) {
					parts.push(`${derived.programReadyCount} ready`);
				}
				if (derived.programDispatchableCount) {
					parts.push(`${derived.programDispatchableCount} dispatchable`);
				}
				if (derived.programAttentionCount) {
					parts.push(
						derived.programAttentionCount === 1
							? "1 needs attention"
							: `${derived.programAttentionCount} need attention`,
					);
				}

				return parts.join(" · ");
			}

			function toneForProgram(program) {
				switch (program.status) {
					case "ready":
					case "completed":
						return "tone-ready";
					case "queued":
						return "tone-queue";
					case "active":
						return "tone-run";
					case "blocked":
					case "attention":
					case "stale":
						return "tone-blocked";
					case "held":
						return "tone-wait";
					default:
						return "tone-muted";
				}
			}

			function programMappedIssues(program) {
				const issues = program.mapped_issue_identifiers ?? [];

				return issues.length ? issues.join(", ") : "NONE";
			}

			function programProgressFacts(program) {
				return [
					["Ready", String(program.ready_count ?? 0)],
					["Queued", String(program.queued_count ?? 0)],
					["Dispatchable", String(program.dispatchable_count ?? 0)],
					["Active", String(program.active_count ?? 0)],
					["Blocked", String(program.blocked_count ?? 0)],
					["Held", String(program.held_count ?? 0)],
					["Attention", String(program.needs_attention_count ?? 0)],
					["Completed", String(program.completed_count ?? 0)],
					["Stale", String(program.stale_count ?? 0)],
				];
			}

			function programNodeReasons(node) {
				const reasons = node.reasons ?? [];
				if (reasons.length) {
					return reasons.join("; ");
				}

				const reasonCodes = node.reason_codes ?? [];
				return reasonCodes.length ? reasonCodes.map(displayToken).join(", ") : "none";
			}

			function programNodeIssue(node) {
				return node.issue_identifier || "unmapped";
			}

			function renderProgramNodeReadbacks(program) {
				const readbacks = program.node_readbacks ?? [];
				if (!readbacks.length) {
					return "";
				}

				const detailKey = `program:${program.program_id}:nodes`;

				return `
					<details data-detail-key="${escapeHtml(detailKey)}"${detailsOpenAttribute(detailKey)}>
						<summary>${escapeHtml(pluralize(readbacks.length, "node diagnostic"))}</summary>
						<div class="grid debug-grid">
							${readbacks
								.map((node) => {
									const reasonCodes = (node.reason_codes ?? []).map(displayToken).join(", ") || "none";
									return `
										${field("Issue", programNodeIssue(node))}
										${field("Program stage", displayToken(node.program_stage || "unknown"))}
										${field("Lifecycle", displayToken(node.lifecycle_state || "unknown"))}
										${field("Readiness", displayToken(node.readiness_state || "unknown"))}
										${field("Issue state", node.issue_state || "none")}
										${field("Dispatch action", node.dispatch_action || "none")}
										${field("Reason codes", reasonCodes)}
										${field("Reasons", programNodeReasons(node))}
										${field("Next action", node.next_action || "none")}
									`;
								})
								.join("")}
						</div>
					</details>
				`;
			}

			function renderExecutionPrograms(snapshot, derived) {
				const programs = derived.executionPrograms ?? [];
				setPanelMeta(
					nodes.programsMeta,
					programMetaText(snapshot, derived),
					derived.programAttentionCount ? "attention" : "",
				);

				if (!programs.length) {
					renderRoutineEmptyList(nodes.executionPrograms);
					return;
				}

				renderStableList(
					nodes.executionPrograms,
					programs
						.map((program) => {
							const tone = toneForProgram(program);
							const mappedIssues = programMappedIssues(program);
							const warning = program.readback_warning
								? inlineStatusFact("Warning", displayToken(program.readback_warning))
								: "";
							return `
								<article class="action-card ${tone}" data-render-key="program:${escapeHtml(program.program_id)}">
									<div class="row-head">
										<div class="row-title">
											<div class="kicker">
												<span>${escapeHtml(displayToken(program.intake_kind || "program"))}</span>
												<span class="mono">${escapeHtml(program.program_id)}</span>
											</div>
											<h4>${escapeHtml(program.public_summary || program.program_id)}</h4>
										</div>
									</div>
									<div class="status-line">
										${statusLabel(displayToken(program.status || "unknown"), tone)}
										${inlineStatusFact("Dispatchable", String(program.dispatchable_count ?? 0))}
										${warning}
									</div>
									<div class="grid two card-facts">
										${cardField("Mapped issues", mappedIssues, mappedIssues === "NONE" ? "is-muted" : "")}
										${cardField("Source contract", program.source_contract_id || "NONE", program.source_contract_id ? "" : "is-muted")}
										${programProgressFacts(program)
											.map(([label, value]) => cardField(label, value))
											.join("")}
									</div>
									${renderProgramNodeReadbacks(program)}
								</article>
							`;
						})
						.join(""),
				);
			}
