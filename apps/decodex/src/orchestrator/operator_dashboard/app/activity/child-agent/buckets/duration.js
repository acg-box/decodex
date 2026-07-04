			function childBucketIsSubsecond(bucket) {
				const seconds = childBucketWallSeconds(bucket);

				return seconds > 0 && seconds < 1;
			}

			function childBucketIsEventOnly(bucket) {
				return childBucketWallSeconds(bucket) === 0 && childBucketHasRecordedActivity(bucket);
			}

			function childBucketWallShare(bucket, totalWall) {
				return childBucketWallSeconds(bucket) / Math.max(1, totalWall);
			}

			function childBucketSharePercent(bucket, totalWall) {
				return Math.round(childBucketWallShare(bucket, totalWall) * 100);
			}

			function childBucketHasMeaningfulWallShare(bucket, totalWall) {
				return childBucketWallSeconds(bucket) >= 5 && childBucketWallShare(bucket, totalWall) >= 0.02;
			}

			function childBucketShareLabel(bucket, totalWall) {
				const seconds = childBucketWallSeconds(bucket);

				return `${childBucketSharePercent(bucket, totalWall)}% · ${formatDuration(seconds)} / ${formatDuration(totalWall)}`;
			}

			function childBucketDuration(bucket) {
				const seconds = childBucketWallSeconds(bucket);

				if (childBucketIsEventOnly(bucket)) {
					return childBucketEventSummary(bucket);
				}

				if (childBucketIsSubsecond(bucket)) {
					return "<1s";
				}

				if (seconds > 0) {
					return formatDuration(seconds);
				}

				return "0s";
			}

			function childBucketWidth(bucket, totalWall) {
				if (!childBucketHasMeaningfulWallShare(bucket, totalWall)) {
					return 0;
				}

				const seconds = childBucketWallSeconds(bucket);

				return seconds > 0 ? Math.max(1, Math.min(100, Math.round((seconds / totalWall) * 100))) : 0;
			}
