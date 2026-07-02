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
