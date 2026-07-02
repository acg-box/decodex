
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
