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
