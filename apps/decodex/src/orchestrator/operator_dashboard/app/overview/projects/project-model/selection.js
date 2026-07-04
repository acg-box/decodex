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
