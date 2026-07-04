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
