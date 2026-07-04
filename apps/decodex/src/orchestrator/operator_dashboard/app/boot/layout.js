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
