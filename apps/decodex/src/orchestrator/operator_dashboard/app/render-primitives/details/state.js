			function runDetailKey(run) {
				return `${currentLaneRenderKey(run)}:more-fields`;
			}

			function currentLaneRenderKey(run) {
				const projectKey = run?.project_id || "unknown-project";
				const issueKey =
					canonicalIssueIdentityKey(run?.issue_id) ||
					canonicalIssueIdentityKey(issueDisplayKey(run));
				return `current-lane:${projectKey}:${issueKey || run?.run_id || "unknown"}`;
			}

			function detailsOpenAttribute(detailKey) {
				return detailDisclosureState.get(detailKey) ? ' open data-detail-state="open"' : "";
			}

			function detailStateKey(details) {
				return details.dataset.detailKey || details.dataset.foldKey || "";
			}

			function detailContent(details) {
				return details.querySelector(":scope > .panel-body, :scope > .grid, :scope > .phase-list");
			}

			function rememberDetailOpenState(details, isOpen) {
				const detailKey = detailStateKey(details);
				if (detailKey) {
					detailDisclosureState.set(detailKey, isOpen);
				}
			}

			function setDetailVisualState(details, isOpen) {
				if (isOpen) {
					details.dataset.detailState = "open";
				} else {
					delete details.dataset.detailState;
				}
			}

			function syncDefaultDetailOpenState(details, shouldOpen) {
				const detailKey = detailStateKey(details);
				if (!detailKey || detailDisclosureState.has(detailKey) || details.classList.contains("is-animating")) {
					return;
				}

				details.open = shouldOpen;
				setDetailVisualState(details, shouldOpen);
			}
