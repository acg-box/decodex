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
