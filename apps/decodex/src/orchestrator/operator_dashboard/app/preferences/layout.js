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
