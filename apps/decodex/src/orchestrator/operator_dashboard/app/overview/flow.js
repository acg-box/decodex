			function setFlowCounts(queue, run, review, land) {
				setMetricText(nodes.flowCounts.queue, queue);
				setMetricText(nodes.flowCounts.run, run);
				setMetricText(nodes.flowCounts.review, review);
				setMetricText(nodes.flowCounts.land, land);
			}

			function setFlowActivity(activity) {
				for (const label of nodes.flowStepLabels) {
					label.classList.toggle("active", Boolean(activity[label.dataset.flowStep]));
				}
			}

			function renderFlow(snapshot, derived) {
				if (!snapshot) {
					setFlowCounts("0 issues", "0 lanes", "0 PRs", "0 PRs");
					setFlowActivity({});
					return;
				}

				const retainedCount = (snapshot.post_review_lanes ?? []).length;
				setFlowCounts(
					pluralize(derived.queueBacklogCandidates.length, "issue"),
					pluralize(derived.currentLaneCount, "lane"),
					pluralize(retainedCount, "PR"),
					pluralize(derived.readyCount, "PR"),
				);
				setFlowActivity({
					queue: derived.queueBacklogCandidates.length > 0,
					run: derived.currentLaneCount > 0,
					review:
						derived.reviewBlockerCount > 0 ||
						derived.reviewWaitingCount > 0 ||
						derived.postReviewLanes.length > 0,
					land: derived.readyCount > 0,
				});
			}

			function runningLaneMetaText(derived) {
				const parts = [`${derived.liveRuns ?? 0} running`];
				const attentionCount = derived.runningAttentionCount;

				if (attentionCount) {
					parts.push(
						attentionCount === 1
							? "1 needs attention"
							: `${attentionCount} need attention`,
					);
				}

				return parts.join(" · ");
			}

			function backlogMetaText(snapshot, derived) {
				if (!snapshot) {
					return "0 queued";
				}

				const parts = [`${derived.queueBacklogCandidates.length} queued`];

				if (derived.queuedReady) {
					parts.push(`${derived.queuedReady} ready`);
				}
				if (derived.queuedWaiting) {
					parts.push(`${derived.queuedWaiting} waiting`);
				}
				if (derived.queuedBlocked) {
					parts.push(`${derived.queuedBlocked} blocked`);
				}
				if (derived.queuedActiveOwned) {
					parts.push(
						pluralize(
							derived.queuedActiveOwned,
							COPY.runningInlineMeta,
							COPY.runningInlineMetaPlural,
						),
					);
				}
				if (derived.queuedClosed) {
					parts.push(`${derived.queuedClosed} ${COPY.staleClosed}`);
				}
				if (derived.reviewOwnedQueueCount) {
					parts.push(`${derived.reviewOwnedQueueCount} in review`);
				}

				return parts.join(" · ");
			}
