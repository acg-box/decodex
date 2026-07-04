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
