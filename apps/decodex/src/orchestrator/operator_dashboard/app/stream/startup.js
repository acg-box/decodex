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
