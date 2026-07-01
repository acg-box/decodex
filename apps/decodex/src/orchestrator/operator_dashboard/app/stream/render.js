			function renderDashboardState({
				snapshot,
				snapshotError,
				snapshotPublishedAt,
			}, options = {}) {
				const readiness = summarizeReadiness(snapshotError, snapshot);
				const notices = dashboardNotices(readiness, snapshotError, snapshot);
				const derived = buildDerivedState(snapshot);
				const reviewItems = reviewLaneItems(derived);
				const shouldRefreshAccounts = options.refreshAccounts !== false;

				if (shouldRefreshAccounts) {
					refreshAccountApiSnapshot();
				}
				renderHeader(snapshot, readiness, notices, snapshotPublishedAt, snapshotError);
				renderFlow(snapshot, derived);
				renderProjects(snapshot, derived);
				renderAccountPool(snapshot);
				renderCurrentLanes(snapshot, derived);
				renderExecutionPrograms(snapshot, derived);
				renderQueuedCandidates(
					nodes.queuedCandidates,
					derived.queueBacklogCandidates,
				);
				setPanelMeta(nodes.queuedMeta, backlogMetaText(snapshot, derived));
				renderRecentRuns(snapshot);
				renderActionCards(
					nodes.reviewQueue,
					reviewItems,
				);
				setPanelMeta(
					nodes.reviewLanesMeta,
					snapshot
						? `${pluralize(derived.postReviewLanes.length, "PR")} · ${pluralize(derived.reviewBlockerCount, "needs attention", "need attention")} · ${derived.readyItems.length} ready · ${derived.reviewWaitingCount} waiting · ${derived.cleanupCount} cleanup`
						: "0 PRs · 0 need attention · 0 ready · 0 waiting · 0 cleanup",
				);
				renderWorktrees(snapshot);
			}
