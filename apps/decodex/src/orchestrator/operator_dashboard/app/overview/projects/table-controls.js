
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
