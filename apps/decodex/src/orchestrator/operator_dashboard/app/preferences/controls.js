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
