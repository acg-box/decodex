			function renderChildAgentBreakdown(run) {
				const summary = childAgentActivity(run);

				if (!summary) {
					return "";
				}

				const lifecycle = currentLaneLifecycleMetrics(run, summary);
				const buckets = (lifecycle.buckets || []).length ? lifecycle.buckets : childAgentBuckets(summary);
				const totalWall = Math.max(
					1,
					Number(lifecycle.wall_seconds || 0),
					buckets.reduce((total, bucket) => total + Number(bucket.wall_seconds || 0), 0),
				);
				const current = childAgentCurrentSummary(summary) || "none";
				const contextRows = childAgentContextRows(run, summary, lifecycle);
				const shareBuckets = buckets.filter(
					(bucket) =>
						childBucketIsPrimaryShareBucket(bucket) &&
						childBucketHasMeaningfulWallShare(bucket, totalWall),
				);
				const diagnosticBuckets = buckets
					.filter(
						(bucket) =>
							!childBucketIsPrimaryShareBucket(bucket) &&
							!childBucketIsLifecycleTotalBucket(bucket) &&
							!childBucketHasMeaningfulWallShare(bucket, totalWall),
					)
					.sort(
						(left, right) =>
							childDiagnosticBucketRank(left) - childDiagnosticBucketRank(right) ||
							String(left.name || "").localeCompare(String(right.name || "")),
					);

				return `
					<div class="child-activity" aria-label="Child agent timing breakdown">
						<div class="child-activity-head is-project">
							<span>Project</span>
							<strong>${escapeHtml(runProjectSummary(run))}</strong>
						</div>
						<div class="child-activity-head">
							<span>Activity</span>
							<strong>${escapeHtml(current)}</strong>
						</div>
						<div class="child-activity-body">
							${
								shareBuckets.length
									? `<div class="child-share-list">
											${shareBuckets
												.slice(0, 3)
												.map((bucket) => {
													const width = childBucketWidth(bucket, totalWall);

														return `
															<div class="child-bucket is-share" data-render-key="${escapeHtml(childBucketRenderKey(bucket))}" data-duration="wall-share">
																<span class="child-bucket-name">${escapeHtml(childBucketDisplayName(bucket))}</span>
																<span class="child-bucket-bar" aria-hidden="true" style="--bucket-width: ${width}%"><span></span></span>
																<span class="child-bucket-value">${escapeHtml(childBucketShareLabel(bucket, totalWall))}</span>
															</div>
													`;
												})
												.join("")}
										</div>`
									: ""
							}
							${
								diagnosticBuckets.length
									? `<div class="child-diagnostic-grid">
											${diagnosticBuckets
												.slice(0, 4)
												.map((bucket) => {
													const eventOnly = childBucketIsEventOnly(bucket);
													const bucketClass = eventOnly
														? "child-bucket is-event-only"
														: "child-bucket is-diagnostic";
													const bucketState = eventOnly
														? ' data-duration="event-diagnostics"'
														: ' data-duration="diagnostic"';
													const signalSummary = childBucketDiagnosticSummary(bucket);

													return `
														<div class="${bucketClass}" data-render-key="${escapeHtml(childBucketRenderKey(bucket))}"${bucketState}>
															<span class="child-bucket-name">${escapeHtml(displayToken(bucket.name))}</span>
															<span class="child-bucket-signals" aria-label="${escapeHtml(signalSummary)}">
																${renderChildBucketDiagnosticSignals(bucket)}
															</span>
														</div>
													`;
												})
												.join("")}
										</div>`
									: ""
							}
							${
								contextRows.length
									? `<div class="child-context-group" aria-label="Context lifecycle metrics">
											${contextRows.join("")}
										</div>`
									: ""
							}
						</div>
					</div>
				`;
			}
