			nodes.projectOverview.addEventListener("click", (event) => {
				if (!(event.target instanceof Element)) {
					return;
				}

				const sortButton = event.target.closest("[data-project-sort-key]");
				if (sortButton) {
					event.preventDefault();
					const key = sortButton.dataset.projectSortKey;
					if (!isProjectSortKey(key)) {
						return;
					}

					projectSort = {
						key,
						direction:
							projectSort.key === key
								? projectSort.direction === "asc"
									? "desc"
									: "asc"
								: projectSortDefaultDirection(key),
					};
					persistProjectSort();
					if (lastDashboardRender) {
						renderDashboardState(lastDashboardRender);
					}
					return;
				}

				const locationToggle = event.target.closest("[data-project-location-toggle]");
				if (locationToggle) {
					event.preventDefault();
					projectLocationsHidden = !projectLocationsHidden;
					persistProjectLocationPrivacy(projectLocationsHidden);
					if (lastDashboardRender) {
						renderDashboardState(lastDashboardRender);
						return;
					}
					renderProjectLocationToggle();
					return;
				}

				const workInfo = event.target.closest("[data-project-work-info]");
				if (!workInfo) {
					return;
				}

				event.preventDefault();
				projectWorkInfoOpen = !projectWorkInfoOpen;
				renderProjectWorkInfoState();
			});
