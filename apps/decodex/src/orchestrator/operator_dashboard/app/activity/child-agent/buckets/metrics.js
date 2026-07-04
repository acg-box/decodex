			function childBucketMetricNumber(bucket, field) {
				const value = Number(bucket?.[field] || 0);

				return Number.isFinite(value) ? Math.max(0, value) : 0;
			}

			function childBucketWallSeconds(bucket) {
				return childBucketMetricNumber(bucket, "wall_seconds");
			}
