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
			const ACCOUNT_RANDOM_NAMES = [
				"Alex",
				"Avery",
				"Bailey",
				"Blake",
				"Casey",
				"Charlie",
				"Clara",
				"Dana",
				"Drew",
				"Eden",
				"Elliot",
				"Emery",
				"Evan",
				"Finley",
				"Harper",
				"Hayden",
				"Iris",
				"Jamie",
				"Jordan",
				"Kai",
				"Kendall",
				"Lane",
				"Liam",
				"Logan",
				"Mason",
				"Maya",
				"Mia",
				"Morgan",
				"Noah",
				"Nora",
				"Owen",
				"Paige",
				"Parker",
				"Quinn",
				"Reese",
				"Remy",
				"Riley",
				"Rowan",
				"Sage",
				"Sasha",
				"Sidney",
				"Taylor",
				"Theo",
				"Val",
			];
			const DASHBOARD_WEBSOCKET_ENDPOINT = "/dashboard/control";
			const DASHBOARD_LOCAL_CLOCK_INTERVAL_MS = 5_000;
			const ACCOUNT_API_REFRESH_INTERVAL_MS = 15_000;
			const RUN_ATTENTION_IDLE_SECONDS = 60;
			const RUN_STALE_NO_PROCESS_SECONDS = 300;

			const nodes = {
				projectTitle: document.getElementById("project-title"),
				flowStepLabels: [...document.querySelectorAll("[data-flow-step]")],
				flowCounts: {
					queue: document.getElementById("flow-queue"),
					run: document.getElementById("flow-run"),
					review: document.getElementById("flow-review"),
					land: document.getElementById("flow-land"),
				},
				transportHealth: document.getElementById("transport-health"),
				noticeDock: document.getElementById("notice-dock"),
				noticeCount: document.getElementById("notice-count"),
				noticeLabel: document.getElementById("notice-label"),
				noticeList: document.getElementById("notice-list"),
				themeButtons: [...document.querySelectorAll("[data-theme-choice]")],
				workspace: document.getElementById("workspace"),
				primaryStack: document.getElementById("primary-stack"),
				accountModeMeta: document.getElementById("account-mode-meta"),
				panels: {
					projects: document.getElementById("projects-panel"),
					accountPool: document.getElementById("account-pool-panel"),
					currentLanes: document.getElementById("current-lanes-panel"),
					programs: document.getElementById("programs-panel"),
					queue: document.getElementById("queue-panel"),
					recent: document.getElementById("recent-panel"),
					review: document.getElementById("review-panel"),
					worktrees: document.getElementById("worktrees-panel"),
				},
				sectionMarkers: {
					control: document.getElementById("section-marker-control"),
					projects: document.getElementById("section-marker-projects"),
					execution: document.getElementById("section-marker-execution"),
					aftercare: document.getElementById("section-marker-aftercare"),
				},
				projectOverview: document.getElementById("project-overview"),
				projectFilterToggle: document.getElementById("project-filter-toggle"),
				accountPool: document.getElementById("account-pool"),
				queuedCandidates: document.getElementById("queued-candidates"),
				queuedMeta: document.getElementById("queued-meta"),
				currentLanes: document.getElementById("current-lanes"),
				currentLanesMeta: document.getElementById("current-lanes-meta"),
				executionPrograms: document.getElementById("execution-programs"),
				programsMeta: document.getElementById("programs-meta"),
				recentRuns: document.getElementById("recent-runs"),
				recentRunsMeta: document.getElementById("recent-runs-meta"),
				reviewQueue: document.getElementById("review-queue"),
				reviewLanesMeta: document.getElementById("review-lanes-meta"),
				worktrees: document.getElementById("worktrees"),
				worktreesMeta: document.getElementById("worktrees-meta"),
			};

			let lastDashboardRender = null;
			let dashboardSocket = null;
			let dashboardSocketReconnectTimer = null;
			let dashboardLocalClockTimer = null;
			let dashboardLivePresentation = null;
			let dashboardLiveRunActivitySeen = false;
			let dashboardLiveAccountControl = null;
			let dashboardStreamState = {
				supported: typeof window.WebSocket === "function",
				connected: false,
				error: false,
				lastEventAt: null,
			};
			let dashboardSubscription = normalizeDashboardSubscription();
			let dashboardControlEvents = [];
			let dashboardControlRequestCounter = 0;
			const themeMediaQuery = window.matchMedia("(prefers-color-scheme: dark)");
			const reducedMotionQuery = window.matchMedia("(prefers-reduced-motion: reduce)");
			let themeSelection = loadThemeSelection();
			let accountEmailsHidden = loadAccountPrivacy();
			let pendingAccountNameOffsets = {};
			let accountApiSnapshot = null;
			let accountApiRefreshInFlight = false;
			let accountApiRefreshedAt = 0;
			let accountPoolSort = loadAccountPoolSort();
			let accountSelectionConfirmation = null;
			let expandedAccountProfileKeys = new Set();
			let projectFilterMode = loadProjectFilterMode();
			let projectLocationsHidden = loadProjectLocationPrivacy();
			let projectSort = loadProjectSort();
			let projectWorkInfoOpen = false;
			const detailDisclosureState = new Map();
			const detailAnimationTimers = new WeakMap();

			const DASHBOARD_LAYOUT = {
				primary: ["accountPool", "projects", "currentLanes", "programs", "queue", "review", "worktrees", "recent"],
			};
			const DASHBOARD_SECTION_GROUPS = [
				{ marker: "control", panels: ["accountPool"] },
				{ marker: "projects", panels: ["projects"] },
				{ marker: "execution", panels: ["currentLanes", "programs", "queue"] },
				{ marker: "aftercare", panels: ["review", "worktrees", "recent"] },
			];
			const COPY = {
				currentLane: "Current Lanes",
				runningInline: "Running here",
				runningInlineMeta: "already running",
				runningInlineMetaPlural: "already running",
				protocolEvent: "Protocol event",
				staleClosed: "closed labels",
			};
