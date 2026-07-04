			const DETAILS_ANIMATION_MS = 380;
			const FOLD_PANEL_ANIMATION_MS = 220;
			const THEME_STORAGE_KEY = "decodex.operator.theme";
			const ACCOUNT_PRIVACY_STORAGE_KEY = "decodex.operator.accountPrivacy";
			const ACCOUNT_POOL_SORT_STORAGE_KEY = "decodex.operator.accountSort";
			const PROJECT_FILTER_STORAGE_KEY = "decodex.operator.projectFilter";
			const PROJECT_LOCATION_PRIVACY_STORAGE_KEY = "decodex.operator.projectLocationPrivacy";
			const PROJECT_SORT_STORAGE_KEY = "decodex.operator.projectSort";
			const ACCOUNT_POOL_SORT_COLUMNS = [
				["account", "Account"],
				["plan", "Weight"],
				["primary", "5h"],
				["secondary", "7d"],
				["credits", "Credits"],
				["status", "Status"],
			];
			const PROJECT_SORT_COLUMNS = [
				["project", "Project"],
				["location", "Location"],
				["activity", "Activity"],
				["work", "Work"],
			];
			const FIELD_LABEL_ACRONYMS = new Map([
				["api", "API"],
				["codex", "Codex"],
				["cwd", "CWD"],
				["id", "ID"],
				["pr", "PR"],
				["prs", "PRs"],
				["url", "URL"],
			]);
			const ACCOUNT_IDENTITY_EDGE_CHARS = 6;
			const ACCOUNT_IDENTITY_MIN_EDGE_CHARS = 3;
			const DASHBOARD_WEBSOCKET_ENDPOINT = "/dashboard/control";
			const DASHBOARD_LOCAL_CLOCK_INTERVAL_MS = 5_000;
			const ACCOUNT_API_REFRESH_INTERVAL_MS = 15_000;
			const RUN_ATTENTION_IDLE_SECONDS = 60;
			const RUN_STALE_NO_PROCESS_SECONDS = 300;
