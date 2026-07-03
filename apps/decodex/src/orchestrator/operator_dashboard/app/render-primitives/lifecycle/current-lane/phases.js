				function fallbackLifecyclePhaseForRun(run) {
					const status = String(run?.status || "").toLowerCase();
					const operation = String(run?.current_operation || "").toLowerCase();
					const reviewPhase = String(run?.loop_status?.review?.phase || "").toLowerCase();

					if (["cleanup_complete", "closeout", "closeout_pending", "landed"].includes(status)) {
						return { key: "closeout", label: "Closeout" };
					}
					if (
						["manual_attention", "manual_attention_pending", "needs_attention", "terminal_failure"].includes(status) ||
						String(run?.phase || "").toLowerCase() === "needs_attention"
					) {
						return { key: "manual_attention", label: "Manual attention" };
					}
					if (reviewPhase === "repair" || status === "review_repair_pending") {
						return { key: "review_repair", label: "Review repair" };
					}
					if (reviewPhase || status === "review_handoff_pending" || operation === "review_writeback") {
						return { key: "review", label: "Review" };
					}

					return { key: "development", label: "Development" };
				}

				function lifecycleMetricDurationFact(metrics) {
					const modelSeconds = lifecycleBucketSeconds(metrics, "Model");
					const activitySeconds = modelSeconds > 0 ? modelSeconds : lifecycleNumber(metrics?.wall_seconds);

					if (activitySeconds <= 0) {
						return null;
					}

					return [modelSeconds > 0 ? "inference" : "activity", formatDuration(activitySeconds)];
				}
