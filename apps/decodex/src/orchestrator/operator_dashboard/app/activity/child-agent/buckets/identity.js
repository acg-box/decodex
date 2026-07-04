			function childBucketDisplayName(bucket) {
				return childBucketIsPrimaryShareBucket(bucket) ? "Inference" : displayToken(bucket?.name);
			}

			function childBucketRenderKey(bucket) {
				return `child-bucket:${bucket?.name || "unknown"}`;
			}

			function childBucketNormalizedName(bucket) {
				return String(bucket?.name || "").toLowerCase();
			}

			function childBucketIsPrimaryShareBucket(bucket) {
				return childBucketNormalizedName(bucket) === "model";
			}

			function childBucketIsLifecycleTotalBucket(bucket) {
				const name = childBucketNormalizedName(bucket);

				return name === "protocol" || name === "tracker";
			}

			function childDiagnosticBucketRank(bucket) {
				const name = childBucketNormalizedName(bucket);
				if (name.includes("protocol")) {
					return 0;
				}
				if (name.includes("tracker")) {
					return 1;
				}
				if (name.includes("tool")) {
					return 2;
				}

				return 3;
			}
