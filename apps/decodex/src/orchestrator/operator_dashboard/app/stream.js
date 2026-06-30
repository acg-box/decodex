
			function unixEpochSeconds(value) {
				const unixEpochSeconds = Number(value);
				if (!Number.isFinite(unixEpochSeconds)) {
					return null;
				}

				return unixEpochSeconds;
			}

			function unixEpochSecondsToIso(value) {
				const seconds = unixEpochSeconds(value);
				if (seconds == null) {
					return null;
				}

				const parsed = new Date(seconds * 1000);
				if (Number.isNaN(parsed.getTime())) {
					return null;
				}

				return parsed.toISOString();
			}

			function isoTimestampUnixEpoch(value) {
				const timestamp = Date.parse(value || "");
				if (!Number.isFinite(timestamp)) {
					return null;
				}

				return Math.floor(timestamp / 1000);
			}

			function parseDashboardSocketMessage(message) {
				try {
					return JSON.parse(message.data);
				} catch (_error) {
					return null;
				}
			}

			function updateDashboardStreamState(patch, shouldRender = true) {
				dashboardStreamState = {
					...dashboardStreamState,
					...patch,
				};

				if (shouldRender && lastDashboardRender) {
					renderDashboardState(lastDashboardRender);
				}
			}

			function scheduleDashboardSocketReconnect() {
				if (dashboardSocketReconnectTimer) {
					return;
				}

				dashboardSocketReconnectTimer = window.setTimeout(() => {
					dashboardSocketReconnectTimer = null;
					connectDashboardSocket();
				}, 2000);
			}

			function renderDashboardLocalClockTick() {
				if (document.hidden || !lastDashboardRender) {
					return;
				}

				refreshAccountApiSnapshot();
				renderDashboardState(lastDashboardRender, { refreshAccounts: false });
			}

			function startDashboardLocalClock() {
				if (dashboardLocalClockTimer) {
					return;
				}

				dashboardLocalClockTimer = window.setInterval(
					renderDashboardLocalClockTick,
					DASHBOARD_LOCAL_CLOCK_INTERVAL_MS,
				);
			}

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

			function applyDashboardSnapshotEvent(payload) {
				if (!payload?.snapshot) {
					return;
				}

				clearDashboardLiveRunActivityOverlay();
				lastDashboardRender = {
					snapshot: payload.snapshot,
					snapshotError: "",
					snapshotPublishedAt:
						unixEpochSecondsToIso(payload.snapshotPublishedAtUnixEpoch) ||
						new Date().toISOString(),
				};
				updateDashboardStreamState(
					{
						connected: true,
						error: false,
						lastEventAt: new Date().toISOString(),
					},
					false,
				);
				renderDashboardState(lastDashboardRender);
			}

			function dashboardSocketIsOpen() {
				return (
					dashboardStreamState.supported &&
					dashboardSocket &&
					dashboardSocket.readyState === window.WebSocket.OPEN
				);
			}

			function nextDashboardControlRequestId() {
				dashboardControlRequestCounter += 1;
				return `dash-${Date.now()}-${dashboardControlRequestCounter}`;
			}

			function sendDashboardSocketMessage(message) {
				if (!dashboardSocketIsOpen()) {
					recordDashboardControlEvent({
						accepted: false,
						action: message.action || message.type || "control",
						status: "offline",
						message: "Dashboard WebSocket is not connected.",
					});
					return false;
				}

				dashboardSocket.send(JSON.stringify(message));
				return true;
			}

			function syncDashboardSubscriptionToSocket() {
				sendDashboardSocketMessage({
					type: "subscribe",
					requestId: nextDashboardControlRequestId(),
					...dashboardSubscription,
				});
			}

			function sendDashboardControl(action, payload = {}) {
				return sendDashboardSocketMessage({
					type: "control",
					requestId: nextDashboardControlRequestId(),
					action,
					...payload,
				});
			}

			function dashboardControlActionLabel(action) {
				return displayToken(action);
			}

			function recordDashboardControlEvent(event) {
				const recorded = {
					key: `${event.requestId || event.action || "control"}:${Date.now()}`,
					at: new Date().toISOString(),
					accepted: Boolean(event.accepted),
					action: event.action || "control",
					status: event.status || (event.accepted ? "accepted" : "failed"),
					message: event.message || "Dashboard control response received.",
				};
				dashboardControlEvents = [recorded, ...dashboardControlEvents].slice(0, 4);
				if (lastDashboardRender) {
					renderDashboardState(lastDashboardRender);
				}
			}

			function applyDashboardControlAck(payload) {
				if (payload?.action === "ack" || payload?.action === "subscribe") {
					return;
				}
				recordDashboardControlEvent(payload || {});
			}

			function applyDashboardControlReady(payload) {
				void payload;
			}

			function connectDashboardSocket() {
				if (!dashboardStreamState.supported) {
					updateDashboardStreamState({
						connected: false,
						error: true,
					});
					return;
				}

				if (dashboardSocketReconnectTimer) {
					window.clearTimeout(dashboardSocketReconnectTimer);
					dashboardSocketReconnectTimer = null;
				}
				if (dashboardSocket) {
					dashboardSocket.onclose = null;
					dashboardSocket.onerror = null;
					dashboardSocket.close();
				}

				dashboardSocket = new WebSocket(dashboardSocketUrl());
				dashboardSocket.onopen = () => {
					updateDashboardStreamState({
						connected: true,
						error: false,
					});
					syncDashboardSubscriptionToSocket();
					};
					dashboardSocket.onclose = () => {
						dashboardLivePresentation = null;
						dashboardLiveRunActivitySeen = false;
						dashboardLiveAccountControl = null;
						updateDashboardStreamState({
						connected: false,
						error: true,
					});
					scheduleDashboardSocketReconnect();
					};
					dashboardSocket.onerror = () => {
						dashboardLivePresentation = null;
						dashboardLiveRunActivitySeen = false;
						dashboardLiveAccountControl = null;
						updateDashboardStreamState({
						connected: false,
						error: true,
					});
				};
				dashboardSocket.onmessage = (message) => {
					const event = parseDashboardSocketMessage(message);
					if (event?.type === "snapshot") {
						applyDashboardSnapshotEvent(event.payload);
					} else if (event?.type === "runActivity") {
						applyDashboardRunActivity(event.payload);
					} else if (event?.type === "controlAck") {
						applyDashboardControlAck(event.payload);
					} else if (event?.type === "controlReady") {
						applyDashboardControlReady(event.payload);
					}
				};
			}

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

			function startDashboardStream() {
				applyTheme(themeSelection, false);
				renderAccountPrivacyToggle();
				renderProjectFilterToggle();
				renderProjectLocationToggle();
				renderProjectWorkInfoState();
				applyDashboardLayout();
					lastDashboardRender = {
						snapshot: null,
						snapshotError: "",
						snapshotPublishedAt: null,
				};
				renderDashboardState(lastDashboardRender);
				connectDashboardSocket();
				startDashboardLocalClock();
			}

			for (const button of nodes.themeButtons) {
				button.addEventListener("click", () => {
					applyTheme(button.dataset.themeChoice);
				});
			}

			nodes.projectFilterToggle.addEventListener("click", () => {
				projectFilterMode = projectFilterMode === "all" ? "active" : "all";
				persistProjectFilterMode();
				if (lastDashboardRender) {
					renderDashboardState(lastDashboardRender);
					return;
				}
				renderProjectFilterToggle();
			});

			nodes.accountPool.addEventListener("click", (event) => {
				if (!(event.target instanceof Element)) {
					return;
				}

				const privacyButton = event.target.closest("[data-account-privacy-toggle]");
				if (privacyButton) {
					event.preventDefault();
					accountEmailsHidden = !accountEmailsHidden;
					persistAccountPrivacy(accountEmailsHidden);
					renderAccountPrivacyToggle();
					if (lastDashboardRender) {
						renderDashboardState(lastDashboardRender);
					}
					return;
				}

				const sortButton = event.target.closest("[data-account-sort-key]");
				if (sortButton) {
					event.preventDefault();
					const key = sortButton.dataset.accountSortKey;
					if (!isAccountPoolSortKey(key)) {
						return;
					}

					accountPoolSort = {
						key,
						direction:
							accountPoolSort.key === key && accountPoolSort.direction === "asc"
								? "desc"
								: "asc",
					};
					persistAccountPoolSort();
					if (lastDashboardRender) {
						renderDashboardState(lastDashboardRender);
					}
					return;
				}

				const profileToggle = event.target.closest("[data-account-profile-toggle]");
				if (profileToggle) {
					event.preventDefault();
					toggleCodexAccountProfileKey(profileToggle.dataset.accountProfileToggle);
					return;
				}

				const confirmButton = event.target.closest("[data-account-confirm-action]");
				if (confirmButton) {
					event.preventDefault();
					const action = confirmButton.dataset.accountConfirmAction;
					const accountSelector = confirmButton.dataset.accountSelector || null;
					handleAccountSelectionConfirmation(action, accountSelector);
					return;
				}

				const button = event.target.closest("[data-account-name-reroll]");
				if (button) {
					event.preventDefault();
					const key = button.dataset.accountNameReroll;
					if (!key) {
						return;
					}

					const account = lastDashboardRender
						? codexAccountPoolAccounts(lastDashboardRender.snapshot).find(
								(candidate) => codexAccountRandomNameKey(candidate) === key,
							)
						: null;
					const current = account
						? codexAccountRandomNameOffset(account)
						: normalizeAccountNameOffset(pendingAccountNameOffsets[key]);
					const next = normalizeAccountNameOffset(current + 1);
					pendingAccountNameOffsets = {
						...pendingAccountNameOffsets,
						[key]: next,
					};
					if (lastDashboardRender) {
						renderDashboardState(lastDashboardRender);
					}
					if (account) {
						postAccountNameOffset(account, next)
							.then((updated) => {
								if (updated) {
									delete pendingAccountNameOffsets[key];
									if (lastDashboardRender) {
										renderDashboardState(lastDashboardRender);
									}
								}
							})
							.catch(() => {});
					}
					return;
				}

				const profileRow = event.target.closest("[data-account-profile-row-toggle]");
				if (profileRow && !accountProfileRowClickIsSuppressed(event.target)) {
					event.preventDefault();
					toggleCodexAccountProfileKey(profileRow.dataset.accountProfileRowToggle);
				}
			});

			nodes.projectOverview.addEventListener("click", (event) => {
				if (!(event.target instanceof Element)) {
					return;
				}

				const sortButton = event.target.closest("[data-project-sort-key]");
				if (sortButton) {
					event.preventDefault();
					const key = sortButton.dataset.projectSortKey;
					if (!isProjectSortKey(key)) {
						return;
					}

					projectSort = {
						key,
						direction:
							projectSort.key === key
								? projectSort.direction === "asc"
									? "desc"
									: "asc"
								: projectSortDefaultDirection(key),
					};
					persistProjectSort();
					if (lastDashboardRender) {
						renderDashboardState(lastDashboardRender);
					}
					return;
				}

				const locationToggle = event.target.closest("[data-project-location-toggle]");
				if (locationToggle) {
					event.preventDefault();
					projectLocationsHidden = !projectLocationsHidden;
					persistProjectLocationPrivacy(projectLocationsHidden);
					if (lastDashboardRender) {
						renderDashboardState(lastDashboardRender);
						return;
					}
					renderProjectLocationToggle();
					return;
				}

				const workInfo = event.target.closest("[data-project-work-info]");
				if (!workInfo) {
					return;
				}

				event.preventDefault();
				projectWorkInfoOpen = !projectWorkInfoOpen;
				renderProjectWorkInfoState();
			});

			if (typeof themeMediaQuery.addEventListener === "function") {
				themeMediaQuery.addEventListener("change", () => {
					if (themeSelection === "system") {
						applyTheme("system", false);
					}
				});
			} else if (typeof themeMediaQuery.addListener === "function") {
				themeMediaQuery.addListener(() => {
					if (themeSelection === "system") {
						applyTheme("system", false);
					}
				});
			}

			document.addEventListener("click", (event) => {
				if (!(event.target instanceof Element)) {
					return;
				}

				if (
					accountSelectionConfirmation &&
					!event.target.closest("[data-account-confirm-action]")
				) {
					clearAccountSelectionConfirmation(true);
				}

				if (!event.target.closest("[data-project-work-info]")) {
					projectWorkInfoOpen = false;
					renderProjectWorkInfoState();
				}

				const noticeAck = event.target.closest("[data-notice-ack]");
				if (noticeAck) {
					event.preventDefault();
					const ackKey = noticeAck.dataset.noticeAck;
					dashboardControlEvents = dashboardControlEvents.filter(
						(controlEvent) => controlEvent.key !== ackKey,
					);
					sendDashboardControl("ack", { key: ackKey });
					if (lastDashboardRender) {
						renderDashboardState(lastDashboardRender);
					}
					return;
				}

				const summary = event.target.closest("summary");
				if (!summary) {
					return;
				}

				const details = summary.parentElement;
				if (!(details instanceof HTMLDetailsElement)) {
					return;
				}

				event.preventDefault();
				animateDetail(details, !details.open);
			});

			document.addEventListener("visibilitychange", () => {
				if (document.hidden) {
					return;
				}

				if (lastDashboardRender) {
					renderDashboardState(lastDashboardRender);
				}
				if (!dashboardSocketIsOpen()) {
					connectDashboardSocket();
				}
			});

			startDashboardStream();
