
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
