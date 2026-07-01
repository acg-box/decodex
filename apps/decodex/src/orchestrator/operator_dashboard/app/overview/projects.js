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

