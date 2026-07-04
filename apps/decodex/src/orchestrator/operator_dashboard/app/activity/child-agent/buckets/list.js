			function childAgentBuckets(summary) {
				return [...(summary?.buckets || [])].sort((left, right) => {
					const wallDelta = (right.wall_seconds || 0) - (left.wall_seconds || 0);
					if (wallDelta !== 0) {
						return wallDelta;
					}

					const eventDelta =
						childBucketMetricNumber(right, "event_count") - childBucketMetricNumber(left, "event_count");
					if (eventDelta !== 0) {
						return eventDelta;
					}

					return String(left.name || "").localeCompare(String(right.name || ""));
				});
			}
