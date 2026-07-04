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
