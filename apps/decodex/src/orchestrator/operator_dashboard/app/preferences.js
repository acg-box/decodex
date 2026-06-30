
			function applyDashboardLayout() {
				const layout = DASHBOARD_LAYOUT;
				const primaryVisible = new Set(layout.primary);
				const visibleMarkers = new Set();

				for (const panelKey of layout.primary) {
					const markerKey = sectionMarkerForPanel(panelKey);
					if (markerKey && !visibleMarkers.has(markerKey)) {
						nodes.primaryStack.appendChild(nodes.sectionMarkers[markerKey]);
						visibleMarkers.add(markerKey);
					}
					nodes.primaryStack.appendChild(nodes.panels[panelKey]);
				}
				for (const [panelKey, panelNode] of Object.entries(nodes.panels)) {
					panelNode.hidden = !primaryVisible.has(panelKey);
				}
				for (const [markerKey, markerNode] of Object.entries(nodes.sectionMarkers)) {
					markerNode.hidden = !visibleMarkers.has(markerKey);
				}

				nodes.primaryStack.hidden = layout.primary.length === 0;
			}

			function sectionMarkerForPanel(panelKey) {
				for (const group of DASHBOARD_SECTION_GROUPS) {
					if (group.panels.includes(panelKey)) {
						return group.marker;
					}
				}
				return null;
			}

			function loadThemeSelection() {
				try {
					const stored = window.localStorage.getItem(THEME_STORAGE_KEY);
					return ["system", "light", "dark"].includes(stored) ? stored : "system";
				} catch (_error) {
					return "system";
				}
			}

			function loadAccountPrivacy() {
				try {
					return window.localStorage.getItem(ACCOUNT_PRIVACY_STORAGE_KEY) === "hidden";
				} catch (_error) {
					return false;
				}
			}

			function persistAccountPrivacy(hidden) {
				try {
					window.localStorage.setItem(
						ACCOUNT_PRIVACY_STORAGE_KEY,
						hidden ? "hidden" : "visible",
					);
				} catch (_error) {
					/* Ignore storage failures and continue with the in-memory choice. */
				}
			}

			function normalizeAccountNameOffset(value) {
				const number = Number(value);
				if (!Number.isInteger(number)) {
					return 0;
				}

				return (
					(number % ACCOUNT_RANDOM_NAMES.length) + ACCOUNT_RANDOM_NAMES.length
				) % ACCOUNT_RANDOM_NAMES.length;
			}

			function accountApiAccounts() {
				return Array.isArray(accountApiSnapshot?.accounts)
					? accountApiSnapshot.accounts.filter(Boolean)
					: [];
			}

			function noteAccountApiSnapshot(response) {
				if (!response || !Array.isArray(response.accounts)) {
					return false;
				}

				accountApiSnapshot = response;
				accountApiRefreshedAt = Date.now();

				return true;
			}

			async function refreshAccountApiSnapshot(force = false) {
				const now = Date.now();
				if (
					accountApiRefreshInFlight ||
					(!force &&
						accountApiSnapshot &&
						now - accountApiRefreshedAt < ACCOUNT_API_REFRESH_INTERVAL_MS)
				) {
					return;
				}

				accountApiRefreshInFlight = true;
				try {
					const response = await fetch("/api/accounts?refresh=1", {
						headers: { Accept: "application/json" },
					});
					if (!response.ok) {
						return;
					}
					if (noteAccountApiSnapshot(await response.json()) && lastDashboardRender) {
						renderDashboardState(lastDashboardRender);
					}
				} catch (_error) {
					/* The websocket snapshot remains usable if the account API is unavailable. */
				} finally {
					accountApiRefreshInFlight = false;
				}
			}

			async function postAccountNameOffset(account, offset = null) {
				const selector = codexAccountControlSelector(account);
				if (!selector) {
					return false;
				}

				const body = { selector };
				if (offset != null) {
					body.random_name_offset = normalizeAccountNameOffset(offset);
				}

				const response = await fetch("/api/accounts/reroll-name", {
					method: "POST",
					headers: {
						Accept: "application/json",
						"Content-Type": "application/json",
					},
					body: JSON.stringify(body),
				});
				if (!response.ok) {
					return false;
				}

				return noteAccountApiSnapshot(await response.json());
			}

			function isAccountPoolSortKey(value) {
				return ACCOUNT_POOL_SORT_COLUMNS.some(([key]) => key === value);
			}

			function loadAccountPoolSort() {
				try {
					const stored = window.localStorage.getItem(ACCOUNT_POOL_SORT_STORAGE_KEY);
					const parsed = stored ? JSON.parse(stored) : {};
					const key = String(parsed?.key || "");
					const direction = String(parsed?.direction || "asc");
					if (
						isAccountPoolSortKey(key) &&
						(direction === "asc" || direction === "desc")
					) {
						return { key, direction };
					}
				} catch (_error) {
					/* Ignore invalid storage and use the stable default order. */
				}

				return { key: "", direction: "asc" };
			}

			function persistAccountPoolSort() {
				try {
					window.localStorage.setItem(
						ACCOUNT_POOL_SORT_STORAGE_KEY,
						JSON.stringify(accountPoolSort),
					);
				} catch (_error) {
					/* Ignore storage failures and continue with the in-memory choice. */
				}
			}

			function loadProjectFilterMode() {
				try {
					return window.localStorage.getItem(PROJECT_FILTER_STORAGE_KEY) === "all"
						? "all"
						: "active";
				} catch (_error) {
					return "active";
				}
			}

			function persistProjectFilterMode() {
				try {
					window.localStorage.setItem(PROJECT_FILTER_STORAGE_KEY, projectFilterMode);
				} catch (_error) {
					/* Ignore storage failures and continue with the in-memory choice. */
				}
			}

			function isProjectSortKey(value) {
				return PROJECT_SORT_COLUMNS.some(([key]) => key === value);
			}

			function projectSortDefaultDirection(key) {
				return ["activity", "work"].includes(key) ? "desc" : "asc";
			}

			function loadProjectSort() {
				try {
					const stored = window.localStorage.getItem(PROJECT_SORT_STORAGE_KEY);
					const parsed = stored ? JSON.parse(stored) : {};
					const key = String(parsed?.key || "");
					const direction = String(parsed?.direction || projectSortDefaultDirection(key));
					if (
						isProjectSortKey(key) &&
						(direction === "asc" || direction === "desc")
					) {
						return { key, direction };
					}
				} catch (_error) {
					/* Ignore invalid storage and use the operational default order. */
				}

				return { key: "", direction: "asc" };
			}

			function persistProjectSort() {
				try {
					window.localStorage.setItem(
						PROJECT_SORT_STORAGE_KEY,
						JSON.stringify(projectSort),
					);
				} catch (_error) {
					/* Ignore storage failures and continue with the in-memory choice. */
				}
			}

			function loadProjectLocationPrivacy() {
				try {
					return window.localStorage.getItem(PROJECT_LOCATION_PRIVACY_STORAGE_KEY) === "hidden";
				} catch (_error) {
					return false;
				}
			}

			function persistProjectLocationPrivacy(hidden) {
				try {
					window.localStorage.setItem(
						PROJECT_LOCATION_PRIVACY_STORAGE_KEY,
						hidden ? "hidden" : "visible",
					);
				} catch (_error) {
					/* Ignore storage failures and continue with the in-memory choice. */
				}
			}

			function normalizeDashboardSubscription(subscription = {}) {
				const clean = (value) => {
					const text = String(value || "").trim();
					return text ? text : null;
				};

				return {
					projectId: clean(subscription.projectId),
					issueId: clean(subscription.issueId),
					runId: clean(subscription.runId),
				};
			}

			function eyeToggleIconMarkup() {
				return `
					<svg class="account-eye account-eye-open" aria-hidden="true" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round">
						<path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12Z"></path>
						<circle cx="12" cy="12" r="3"></circle>
					</svg>
					<svg class="account-eye account-eye-off" aria-hidden="true" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round">
						<path d="M10.6 5.1A10.8 10.8 0 0 1 12 5c6.5 0 10 7 10 7a17.4 17.4 0 0 1-2.1 3.1"></path>
						<path d="M6.6 6.7C3.7 8.6 2 12 2 12s3.5 7 10 7a10.9 10.9 0 0 0 5.4-1.4"></path>
						<path d="M9.9 9.9a3 3 0 0 0 4.2 4.2"></path>
						<path d="m3 3 18 18"></path>
					</svg>
				`;
			}

			function accountPrivacyToggleMarkup() {
				return `<button class="account-privacy-toggle" type="button" data-account-privacy-toggle role="switch" aria-checked="false" aria-label="Show account emails">${eyeToggleIconMarkup()}</button>`;
			}

			function projectLocationToggleMarkup() {
				return `<button class="project-location-toggle" type="button" data-project-location-toggle role="switch" aria-checked="false" aria-label="Show project locations">${eyeToggleIconMarkup()}</button>`;
			}

			function renderAccountPrivacyToggle() {
				const visible = !accountEmailsHidden;
				for (const toggle of document.querySelectorAll("[data-account-privacy-toggle]")) {
					toggle.classList.toggle("is-on", visible);
					toggle.setAttribute("aria-checked", visible ? "true" : "false");
					toggle.setAttribute(
						"aria-label",
						visible ? "Hide account emails" : "Show account emails",
					);
					toggle.title = visible ? "Hide account emails" : "Show account emails";
				}
			}

			function renderProjectLocationToggle() {
				const visible = !projectLocationsHidden;
				for (const toggle of document.querySelectorAll("[data-project-location-toggle]")) {
					toggle.classList.toggle("is-on", visible);
					toggle.setAttribute("aria-checked", visible ? "true" : "false");
					toggle.setAttribute(
						"aria-label",
						visible ? "Hide project locations" : "Show project locations",
					);
					toggle.title = visible ? "Hide project locations" : "Show project locations";
				}
			}

			function renderProjectWorkInfoState() {
				for (const button of document.querySelectorAll("[data-project-work-info]")) {
					button.classList.toggle("is-open", projectWorkInfoOpen);
					button.setAttribute("aria-expanded", projectWorkInfoOpen ? "true" : "false");
				}
			}

			function renderProjectFilterToggle(projects = []) {
				const showingAll = projectFilterMode === "all";
				const title = showingAll ? "Show active projects" : "Show all projects";
				nodes.projectFilterToggle.classList.toggle("is-on", showingAll);
				nodes.projectFilterToggle.disabled = projects.length === 0;
				nodes.projectFilterToggle.setAttribute("aria-checked", showingAll ? "true" : "false");
				nodes.projectFilterToggle.setAttribute("aria-label", title);
				nodes.projectFilterToggle.title = title;
			}

			function setPanelMeta(node, text, tone = "") {
				setMetricText(node, text);
				if (tone) {
					node.dataset.tone = tone;
					return;
				}

				delete node.dataset.tone;
			}
