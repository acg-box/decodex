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
