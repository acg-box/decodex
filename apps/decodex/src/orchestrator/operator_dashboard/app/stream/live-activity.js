			function presentationCurrentLaneCards(presentation) {
				return Array.isArray(presentation?.current_lane_cards)
					? presentation.current_lane_cards.filter((card) => card && typeof card === "object")
					: [];
			}

			function snapshotCurrentLaneCards(snapshot) {
				return presentationCurrentLaneCards(snapshot?.presentation);
			}

			function currentLaneRunsFromCards(cards) {
				return cards
					.map((card) => card?.run)
					.filter((run) => run && typeof run === "object");
			}

			function snapshotCurrentLaneRuns(snapshot) {
				return currentLaneRunsFromCards(snapshotCurrentLaneCards(snapshot));
			}

			function currentLaneCardToneClass(card) {
				const run = card?.run || {};
				if (card?.needs_attention === true || card?.tone === "attention") {
					return "tone-blocked";
				}
				if (card?.is_waiting === true || card?.tone === "waiting") {
					return "tone-wait";
				}
				if (card?.counts_as_running === true || runCountsAsRunning(run)) {
					return "tone-run";
				}

				return toneForRun(run);
			}

			function dashboardRunActivityIsStale(payload) {
				const emittedAt = unixEpochSeconds(payload?.emittedAtUnixEpoch);
				const snapshotPublishedAt = isoTimestampUnixEpoch(lastDashboardRender?.snapshotPublishedAt);

				return emittedAt != null && snapshotPublishedAt != null && emittedAt < snapshotPublishedAt;
			}

			function dashboardLiveRunActivityHasOverlay({ includeCompletedEmpty = false } = {}) {
				if (!dashboardLiveRunActivitySeen) {
					return false;
				}
				if (presentationCurrentLaneCards(dashboardLivePresentation).length) {
					return true;
				}

				return includeCompletedEmpty;
			}

			function clearDashboardLiveRunActivityOverlayIfCompleteEmpty() {
				if (
					dashboardLiveRunActivitySeen &&
					!presentationCurrentLaneCards(dashboardLivePresentation).length
				) {
					dashboardLiveRunActivitySeen = false;
					dashboardLivePresentation = null;
					dashboardLiveAccountControl = null;
				}
			}

			function clearDashboardLiveRunActivityOverlay() {
				dashboardLiveRunActivitySeen = false;
				dashboardLivePresentation = null;
				dashboardLiveAccountControl = null;
			}

			function snapshotWithLiveRunActivity(snapshot, options = {}) {
				if (!snapshot || !dashboardSocketIsOpen()) {
					return snapshot;
				}

				if (!dashboardLiveRunActivityHasOverlay(options)) {
					return snapshot;
				}

				const liveRuns = currentLaneRunsFromCards(
					presentationCurrentLaneCards(dashboardLivePresentation),
				);
				return {
					...snapshot,
					account_control:
						dashboardLiveAccountControl && typeof dashboardLiveAccountControl === "object"
							? { ...(snapshot.account_control || {}), ...dashboardLiveAccountControl }
							: snapshot.account_control,
					current_lanes: liveRuns,
					presentation: dashboardLivePresentation,
				};
			}

			function applyDashboardRunActivity(payload) {
				if (!payload?.presentation || typeof payload.presentation !== "object") {
					updateDashboardStreamState({
						connected: true,
						error: false,
						lastEventAt: new Date().toISOString(),
					});

					return;
				}

				if (dashboardRunActivityIsStale(payload)) {
					updateDashboardStreamState(
						{
							connected: true,
							error: false,
							lastEventAt:
								unixEpochSecondsToIso(payload.emittedAtUnixEpoch) ||
								new Date().toISOString(),
						},
						false,
					);
					return;
				}

				dashboardLivePresentation = {
					...payload.presentation,
					current_lane_cards: presentationCurrentLaneCards(payload.presentation),
				};
				dashboardLiveRunActivitySeen = true;
				dashboardLiveAccountControl =
					payload.accountControl && typeof payload.accountControl === "object"
						? { ...payload.accountControl }
						: null;

				if (!lastDashboardRender?.snapshot) {
					updateDashboardStreamState({
						connected: true,
						error: false,
						lastEventAt: new Date().toISOString(),
					});
					clearDashboardLiveRunActivityOverlayIfCompleteEmpty();

					return;
				}

				lastDashboardRender = {
					...lastDashboardRender,
					snapshot: snapshotWithLiveRunActivity(lastDashboardRender.snapshot, {
						includeCompletedEmpty: true,
					}),
				};
				updateDashboardStreamState(
					{
						connected: true,
						error: false,
						lastEventAt: unixEpochSecondsToIso(payload.emittedAtUnixEpoch) || new Date().toISOString(),
					},
					false,
				);
				renderDashboardState(lastDashboardRender);
				clearDashboardLiveRunActivityOverlayIfCompleteEmpty();
			}
